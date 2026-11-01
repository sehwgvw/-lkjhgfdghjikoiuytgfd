mod config;
mod database;
mod handlers;
mod keyboards;
mod services;
mod texts;

#[cfg(test)]
mod tests;

use std::time::Duration;

use teloxide::adaptors::DefaultParseMode;
use teloxide::prelude::*;
use teloxide::types::ParseMode;

use crate::database::{connection, queries};
use crate::services::{deal_manager, safe_send};
use crate::texts::messages as txt;

/// Команды бота. Их регистрация обязательна: без неё бот с включённым
/// privacy mode не получает команды в группах.
///
/// Telegram разрешает только `[a-z0-9_]{1,32}`, поэтому кириллические
/// варианты (/рег, /ресет) в группах работают лишь при выключенном privacy
/// mode. Латинские алиасы доставляются всегда.
const BOT_COMMANDS: &[(&str, &str)] = &[
    ("start", "Запустить бота"),
    ("help", "Список команд"),
    ("id", "Мой Telegram ID"),
    ("reg", "Создать сделку: /reg 10 ton"),
    ("reset", "Сбросить сделку в этой группе"),
    ("cancel", "Отменить сделку"),
    ("ruchenik", "Назначить рученика"),
    ("add", "Добавить команду-триггер"),
    ("commands", "Список команд-триггеров"),
    ("sms", "Список SMS-рассылок"),
];

fn build_stamp() -> String {
    let ts: i64 = env!("BUILD_TIMESTAMP").parse().unwrap_or(0);
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "неизвестно".to_string())
}

async fn on_startup(bot: DefaultParseMode<teloxide::Bot>) {
    if let Err(e) = connection::init_db().await {
        tracing::error!("Ошибка инициализации БД: {}", e);
        panic!("Без базы данных бот работать не может: {}", e);
    }
    tracing::info!("БД инициализирована ✓");

    if let Err(e) = deal_manager::restore_active_deals().await {
        tracing::error!("Ошибка восстановления активных сделок: {}", e);
    }

    // Освобождаем «залипшие» сделочные: помечены занятыми, но активной сделки нет.
    match queries::get_stale_occupied_pool_groups().await {
        Ok(stale) => {
            for entry in stale {
                let _ = bot
                    .set_chat_title(teloxide::types::ChatId(entry.group_id), &txt::pool_group_title(entry.number))
                    .await;
                let _ = queries::set_pool_status_by_group(entry.group_id, "free").await;
                tracing::warn!(
                    "Освобождена залипшая сделочная №{} (группа {}) — активной сделки не было",
                    entry.number,
                    entry.group_id
                );
            }
        }
        Err(e) => tracing::error!("Не удалось проверить залипшие сделочные: {}", e),
    }

    match bot.get_me().await {
        Ok(me) => tracing::info!("Бот: @{} (id {})", me.username(), me.id.0),
        Err(e) => tracing::warn!("Не удалось получить getMe: {}", e),
    }

    // Регистрация команд: иначе в группах (privacy mode) бот их не увидит.
    let token = config::config().bot_token.clone();
    match safe_send::set_my_commands(&token, BOT_COMMANDS).await {
        Ok(()) => tracing::info!("Команды зарегистрированы: {:?}", BOT_COMMANDS),
        Err(e) => tracing::error!("Не удалось зарегистрировать команды: {}", e),
    }

    tracing::info!("═══ Alyaska Garant готов к работе (сборка {}) ═══", build_stamp());
    tracing::info!(
        "Telegram Business: чтобы /рег работал в личном чате между двумя людьми, \
         подключите бота в Telegram → Настройки → Telegram Business → Чат-боты."
    );
}

/// Создаёт Bot, используя встроенный в teloxide прокси-механизм.
///
/// teloxide читает адрес прокси из переменной окружения `TELOXIDE_PROXY`
/// (см. `teloxide::net::client_from_env`). Мы перекладываем туда значение
/// из конфига (`PROXY_URL` / `HTTPS_PROXY` из .env) — это избавляет от
/// необходимости тащить собственный `reqwest` и ловить конфликт версий.
fn build_bot(token: &str, proxy_url: Option<&str>) -> teloxide::Bot {
    if let Some(proxy) = proxy_url {
        match url::Url::parse(proxy) {
            Ok(_) => {
                std::env::set_var("TELOXIDE_PROXY", proxy);
                tracing::info!("Используется прокси для Telegram API: {}", proxy);
            }
            Err(e) => {
                tracing::warn!("Некорректный URL прокси '{}': {} — работаем напрямую", proxy, e);
            }
        }
    }

    let client = teloxide::net::client_from_env();
    teloxide::Bot::with_client(token, client)
}

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cfg = config::config();
    tracing::info!("═══ Alyaska Garant запускается (сборка {}) ═══", build_stamp());
    match cfg.proxy_url.as_deref() {
        Some(p) => tracing::info!("Прокси Telegram API: {}", p),
        None => tracing::info!("Прокси Telegram API: не используется (прямое подключение)"),
    }

    let proxy = cfg.proxy_url.as_deref();
    let bot_dp = build_bot(&cfg.bot_token, proxy).parse_mode(ParseMode::Html);
    let bot = build_bot(&cfg.bot_token, proxy);

    on_startup(bot_dp).await;

    // Offset сохраняем в БД: иначе после перезапуска бот заново обработает
    // все неподтверждённые команды и создаст дубликаты сделок.
    let mut offset: i32 = match queries::get_bot_state("update_offset").await {
        Ok(Some(v)) => v.parse().unwrap_or(0),
        Ok(None) => 0,
        Err(e) => {
            tracing::error!("Не удалось прочитать offset: {}", e);
            0
        }
    };
    tracing::info!("Начинаем polling с offset={}", offset);
    let mut backoff_secs: u64 = 1;

    loop {
        match bot.get_updates().offset(offset).limit(100u8).timeout(10).await {
            Ok(updates) => {
                backoff_secs = 1;
                for upd in updates {
                    offset = upd.id.0 as i32 + 1;
                    if let Err(e) = handle_update(bot.clone(), upd).await {
                        tracing::error!("Ошибка обработки обновления: {}", e);
                    }
                    if let Err(e) = queries::set_bot_state("update_offset", &offset.to_string()).await {
                        tracing::error!("Не удалось сохранить offset: {}", e);
                    }
                }
            }
            Err(e) => {
                let text = e.to_string();
                if text.contains("terminated by other getUpdates") {
                    tracing::error!(
                        "Конфликт: обновления читает другой экземпляр бота. Закройте лишние копии \
                         (taskkill /IM alyaska_garant.exe /F) — иначе сообщения будут теряться."
                    );
                } else {
                    tracing::error!(
                        "Ошибка получения обновлений: {} (повтор через {}с)",
                        text,
                        backoff_secs
                    );
                }
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                // Пауза остаётся короткой: обрыв связи с Telegram — это секундная
                // помеха, а не повод молчать минутами. Длинная пауза означала бы,
                // что бот «не слышит» сообщения, и пользователь не получает ответа.
                backoff_secs = (backoff_secs * 2).min(5);
            }
        }
    }
}

async fn handle_update(
    bot: teloxide::Bot,
    upd: teloxide::types::Update,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match upd.kind {
        teloxide::types::UpdateKind::Message(msg) => {
            handlers::on_message(bot.clone(), msg).await?;
        }
        teloxide::types::UpdateKind::CallbackQuery(cb) => {
            handlers::on_callback_query(bot.clone(), cb).await?;
        }
        teloxide::types::UpdateKind::InlineQuery(query) => {
            handlers::on_inline_query(bot.clone(), query).await?;
        }
        teloxide::types::UpdateKind::ChosenInlineResult(chosen) => {
            handlers::on_chosen_inline_result(bot.clone(), chosen).await?;
        }
        teloxide::types::UpdateKind::ChatMember(event) => {
            handlers::on_chat_member_updated(bot.clone(), event).await?;
        }
        teloxide::types::UpdateKind::BusinessConnection(conn) => {
            handlers::business::on_business_connection(conn).await;
        }
        teloxide::types::UpdateKind::BusinessMessage(msg) => {
            handlers::business::on_business_message(bot.clone(), msg, false).await?;
        }
        teloxide::types::UpdateKind::EditedBusinessMessage(msg) => {
            handlers::business::on_business_message(bot.clone(), msg, true).await?;
        }
        _ => {}
    }
    Ok(())
}
