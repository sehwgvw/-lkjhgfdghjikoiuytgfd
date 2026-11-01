//! Telegram Business — команды в личных чатах между двумя людьми.
//!
//! Обычный бот не видит переписку между людьми: Telegram не отдаёт ему такие
//! сообщения. Единственный способ вмешаться — режим Telegram Business:
//! владелец подключает бота в настройках «Telegram Business», и тогда бот
//! получает `business_message` с сообщениями из чатов бизнес-аккаунта и
//! отвечает в них через `business_connection_id`.

use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

use teloxide::prelude::*;
use teloxide::requests::Requester;
use teloxide::types::{BusinessConnection, BusinessConnectionId, Message};

use crate::config;
use crate::database::queries;
use crate::handlers::command_token;
use crate::handlers::inline::parse_deal_arg;
use crate::keyboards::inline_kb;
use crate::services::deal_manager::{self, StartDealError};
use crate::services::safe_send::{self, KbButton};
use crate::texts::messages as txt;

/// Владелец каждого бизнес-подключения: connection_id → id пользователя.
static OWNERS: OnceLock<RwLock<HashMap<String, i64>>> = OnceLock::new();

/// Уже обработанные бизнес-сообщения: (connection_id, message_id).
/// Нужно, чтобы правка сообщения не создала вторую сделку.
static PROCESSED: OnceLock<RwLock<HashSet<(String, i32)>>> = OnceLock::new();

fn owners() -> &'static RwLock<HashMap<String, i64>> {
    OWNERS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn processed() -> &'static RwLock<HashSet<(String, i32)>> {
    PROCESSED.get_or_init(|| RwLock::new(HashSet::new()))
}

fn state_key(conn_id: &str) -> String {
    format!("biz:{}", conn_id)
}

/// Идентификатор бизнес-подключения, из которого пришло сообщение.
fn business_connection_id(msg: &Message) -> Option<String> {
    match &msg.kind {
        teloxide::types::MessageKind::Common(common) => {
            common.business_connection_id.as_ref().map(|c| c.0.clone())
        }
        _ => None,
    }
}

/// Владелец бизнес-подключения: из памяти, из БД, иначе — запросом к API.
///
/// Событие `business_connection` приходит один раз (и при смене прав), поэтому
/// после перезапуска бота карту приходится восстанавливать.
async fn resolve_owner(bot: &Bot, conn_id: &str) -> Option<i64> {
    if let Some(id) = owners().read().unwrap().get(conn_id).copied() {
        return Some(id);
    }

    if let Ok(Some(v)) = queries::get_bot_state(&state_key(conn_id)).await {
        if let Ok(id) = v.parse::<i64>() {
            owners().write().unwrap().insert(conn_id.to_string(), id);
            return Some(id);
        }
    }

    match bot
        .get_business_connection(BusinessConnectionId(conn_id.to_string()))
        .await
    {
        Ok(conn) => {
            let owner_id = conn.user.id.0 as i64;
            if !conn.is_enabled {
                tracing::warn!(
                    "Бизнес-подключение {} выключено — команда не выполнится. \
                     Включите бота: Telegram → Настройки → Telegram Business → Чат-боты.",
                    conn_id
                );
                return None;
            }
            owners().write().unwrap().insert(conn_id.to_string(), owner_id);
            let _ = queries::set_bot_state(&state_key(conn_id), &owner_id.to_string()).await;
            Some(owner_id)
        }
        Err(e) => {
            tracing::error!("Не удалось получить business_connection {}: {}", conn_id, e);
            None
        }
    }
}

/// Подключение или отключение бота к бизнес-аккаунту.
pub async fn on_business_connection(conn: BusinessConnection) {
    let conn_id = conn.id.0.clone();
    let owner_id = conn.user.id.0 as i64;
    let owner_name = conn.user.username.clone().unwrap_or_default();
    let can_reply = conn.rights.as_ref().map(|r| r.can_reply).unwrap_or(false);

    if conn.is_enabled {
        owners().write().unwrap().insert(conn_id.clone(), owner_id);
        let _ = queries::set_bot_state(&state_key(&conn_id), &owner_id.to_string()).await;
    } else {
        owners().write().unwrap().remove(&conn_id);
        let _ = queries::set_bot_state(&state_key(&conn_id), "off").await;
    }

    tracing::info!(
        "Бизнес-подключение {}: владелец {} (@{}), can_reply={}, включено={}",
        conn_id,
        owner_id,
        owner_name,
        can_reply,
        conn.is_enabled
    );
}

/// Сообщение из чата бизнес-аккаунта.
pub async fn on_business_message(
    bot: Bot,
    msg: Message,
    is_edit: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(conn_id) = business_connection_id(&msg) else {
        return Ok(());
    };

    let text = msg.text().or(msg.caption()).unwrap_or("").trim().to_string();
    let cmd = command_token(&text);
    let preview: String = text.chars().take(60).collect();
    tracing::info!(
        "Бизнес-сообщение{}: mid={} чат={} от={} cmd='{}' текст=\"{}\"",
        if is_edit { " (правка)" } else { "" },
        msg.id.0,
        msg.chat.id.0,
        msg.from.as_ref().map(|u| u.id.0).unwrap_or(0),
        cmd,
        preview
    );

    if cmd != "/рег" && cmd != "/reg" {
        return Ok(());
    }

    let key = (conn_id.clone(), msg.id.0);
    if processed().read().unwrap().contains(&key) {
        tracing::info!("Бизнес-сообщение {} уже обработано — повтор пропущен", msg.id.0);
        return Ok(());
    }

    let Some(owner_id) = resolve_owner(&bot, &conn_id).await else {
        tracing::warn!("Не удалось определить владельца бизнес-подключения {}", conn_id);
        return Ok(());
    };

    // Сделку создаёт владелец бизнес-аккаунта: сообщения собеседника
    // превращать в сделку нельзя.
    if let Some(sender_id) = msg.from.as_ref().map(|u| u.id.0 as i64) {
        if sender_id != owner_id {
            tracing::warn!(
                "Бизнес-команда от {} (владелец подключения {}) — игнорируем",
                sender_id,
                owner_id
            );
            return Ok(());
        }
    }

    if !config::config().is_allowed(owner_id) {
        tracing::warn!("Бизнес-команда от недопущенного ID {} — игнорируем", owner_id);
        return Ok(());
    }

    let arg = text.splitn(2, ' ').nth(1).unwrap_or("").trim();
    let Some(deal_display) = parse_deal_arg(arg) else {
        let usage = txt::reg_usage_text();
        if let Err(e) = safe_send::send_business_message(
            config::config().bot_token.as_str(),
            msg.chat.id.0,
            &conn_id,
            &usage,
            Some(msg.id.0),
            &[],
        )
        .await
        {
            tracing::error!("Не удалось отправить подсказку по /рег в бизнес-чат: {}", e);
        }
        return Ok(());
    };

    let creator_display = msg
        .from
        .as_ref()
        .and_then(|u| u.username.clone())
        .or_else(|| msg.from.as_ref().map(|u| u.first_name.clone()))
        .unwrap_or_else(|| owner_id.to_string());

    tracing::info!(
        "/рег (бизнес): '{}' от {} ({})",
        deal_display,
        owner_id,
        creator_display
    );

    match deal_manager::start_deal_in_pool(&bot, &deal_display, owner_id).await {
        Ok((_deal_id, entry, link)) => {
            processed().write().unwrap().insert(key);
            let card = txt::business_deal_ready_text(
                &txt::esc_html(&deal_display),
                &txt::esc_html(&creator_display),
                &link,
            );
            let rows = inline_kb::deal_go_keyboard(&link);
            match send_to_business_chat(&conn_id, msg.chat.id.0, &card, Some(msg.id.0), &rows).await {
                Ok(mid) => tracing::info!(
                    "Карточка сделки '{}' отправлена в бизнес-чат (message_id={})",
                    deal_display,
                    mid
                ),
                Err(e) => {
                    tracing::error!("Не удалось отправить карточку сделки в бизнес-чат: {}", e);
                    // Карточку не доставили — сделку оставлять нельзя, иначе
                    // сделочная останется занята, а клиент её не увидит.
                    let _ = deal_manager::release_pool_group(&bot, entry.group_id, HashSet::new())
                        .await;
                }
            }
        }
        Err(StartDealError::NoFreeGroup) => {
            let text = txt::no_free_pool_text();
            if let Err(e) =
                send_to_business_chat(&conn_id, msg.chat.id.0, &text, Some(msg.id.0), &[]).await
            {
                tracing::error!("Не удалось отправить ответ о свободных сделочных в бизнес-чат: {}", e);
            }
        }
        Err(StartDealError::Register(e)) => {
            tracing::error!("Ошибка регистрации сделки '{}' (бизнес): {}", deal_display, e);
            let error_text = format!(
                "<b>{} Ошибка при создании сделки:</b> {}",
                txt::e(txt::EMOJI_ERROR, "❌"),
                e
            );
            if let Err(send_err) =
                send_to_business_chat(&conn_id, msg.chat.id.0, &error_text, Some(msg.id.0), &[]).await
            {
                tracing::error!("Не удалось отправить сообщение об ошибке в бизнес-чат: {}", send_err);
            }
        }
    }

    Ok(())
}

/// Отправляет сообщение в чат от имени бизнес-аккаунта.
///
/// `chat_id` в личном чате бизнес-аккаунта — это собеседник, но если
/// Telegram отдаст id владельца, второй адрес всё равно стоит попробовать.
async fn send_to_business_chat(
    conn_id: &str,
    chat_id: i64,
    text: &str,
    reply_to: Option<i32>,
    rows: &[Vec<KbButton>],
) -> Result<i64, String> {
    let token = config::config().bot_token.clone();
    match safe_send::send_business_message(&token, chat_id, conn_id, text, reply_to, rows).await {
        Ok(mid) => Ok(mid),
        Err(first_err) => {
            tracing::warn!(
                "Отправка в бизнес-чат {} не удалась ({}), пробуем id владельца",
                chat_id,
                first_err
            );
            let owner_id = owners().read().unwrap().get(conn_id).copied().unwrap_or(0);
            if owner_id == 0 || owner_id == chat_id {
                return Err(first_err);
            }
            safe_send::send_business_message(&token, owner_id, conn_id, text, reply_to, rows).await
        }
    }
}
