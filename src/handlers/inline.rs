use std::collections::HashMap;
use std::time::SystemTime;

use regex::Regex;
use teloxide::prelude::*;
use teloxide::requests::Requester;
use teloxide::types::InlineQuery;

use crate::config;
use crate::database::queries;
use crate::keyboards::inline_kb;
use crate::services::safe_send;
use crate::services::deal_manager::{self, StartDealError};
use crate::texts::messages as txt;

static DEAL_PATTERN: once_cell::sync::Lazy<Regex> =
    once_cell::sync::Lazy::new(|| Regex::new(r"^(\d+(?:[.,]\d+)?)\s+(.+)$").unwrap());

const MAX_DEAL_LENGTH: usize = 40;

fn truncate_deal_display(deal_display: String) -> String {
    if deal_display.chars().count() > MAX_DEAL_LENGTH {
        deal_display.chars().take(MAX_DEAL_LENGTH).collect()
    } else {
        deal_display
    }
}

/// Разбирает аргументы команды: "10 ton" → "10 ton", "1,5 usdt" → "1.5 usdt".
pub fn parse_deal_arg(arg: &str) -> Option<String> {
    let caps = DEAL_PATTERN.captures(arg.trim())?;
    let amount = caps.get(1).map_or("0", |m| m.as_str()).replace(',', ".");
    let currency = caps.get(2).map_or("", |m| m.as_str().trim());
    Some(truncate_deal_display(format!("{} {}", amount, currency)))
}

fn now_result_id(prefix: &str) -> String {
    format!(
        "{}_{}",
        prefix,
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    )
}

fn denied_inline_results() -> Vec<serde_json::Value> {
    vec![serde_json::json!({
        "type": "article",
        "id": "denied",
        "title": "Нет доступа",
        "description": "Команда доступна только администрации",
        "input_message_content": {
            "message_text": txt::access_denied_short(),
            "parse_mode": "HTML"
        }
    })]
}

pub async fn handle_inline_query(bot: Bot, query: InlineQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = query.query.trim();
    tracing::info!(
        "Inline query от {:?}: '{}'",
        query.from.username.as_deref().unwrap_or("?"),
        text
    );

    if text.is_empty() {
        let _ = bot.answer_inline_query(query.id, Vec::new()).cache_time(5).await;
        return Ok(());
    }

    let clean_text = text.to_lowercase();
    let status_keywords = [
        "test acc", "test accs", "test account", "test accounts", "testacc",
        "status", "status acc", "accs", "аккаунты", "статус",
    ];
    if status_keywords.iter().any(|k| clean_text == *k) {
        let _ = bot.answer_inline_query(query.id, Vec::new()).cache_time(1).await;
        return Ok(());
    }

    if clean_text == "reset all" || clean_text == "ресет all" {
        return handle_reset_all_inline(&query).await;
    }

    if clean_text == "pull groups" || clean_text == "pull group" {
        return handle_pull_groups_inline(&query).await;
    }

    if let Some(deal_display) = parse_deal_arg(text) {
        tracing::info!("Распарсено: {}", deal_display);

        let cb_deal = deal_display.replace(':', " ");
        let callback_data = format!("cd:p:{}", cb_deal);

        let results = vec![serde_json::json!({
            "type": "article",
            "id": now_result_id("deal"),
            "title": txt::inline_title(&deal_display),
            "description": txt::INLINE_DESCRIPTION,
            "input_message_content": {
                "message_text": txt::inline_create_caption(&txt::esc_html(&deal_display)),
                "parse_mode": "HTML"
            },
            "reply_markup": safe_send::markup_json(&inline_kb::inline_create_button(&deal_display, &callback_data))
        })];

        let _ = safe_send::answer_inline_query_with_custom_buttons(
            config::config().bot_token.as_str(),
            query.id.to_string().as_str(),
            &results,
        )
        .await
        .map_err(|e| tracing::error!("Не удалось ответить на inline-запрос: {}", e));
    } else {
        tracing::info!("Не удалось распарсить: '{}'", text);
        let _ = bot.answer_inline_query(query.id, Vec::new()).cache_time(5).await;
    }

    Ok(())
}

/// Результат inline-команды «reset all»: кнопка подтверждения сброса.
pub fn reset_all_results(uid: i64, active_count: usize) -> Vec<serde_json::Value> {
    vec![serde_json::json!({
        "type": "article",
        "id": now_result_id("reset_all"),
        "title": format!("Сбросить все сделки ({})", active_count),
        "description": "Подтвердите сброс всех активных сделочных",
        "input_message_content": {
            "message_text": txt::reset_all_prompt_text(active_count),
            "parse_mode": "HTML"
        },
        "reply_markup": safe_send::markup_json(&inline_kb::reset_all_confirm_button(uid))
    })]
}

/// Результат inline-команды «pull groups»: список сделочных и их количество.
pub fn pull_groups_results(
    active_lines: Vec<String>,
    free_lines: Vec<String>,
) -> Vec<serde_json::Value> {
    let body = txt::pull_groups_text(&active_lines, &free_lines);
    vec![serde_json::json!({
        "type": "article",
        "id": now_result_id("pull_groups"),
        "title": format!("Сделочные: {} активных, {} свободных", active_lines.len(), free_lines.len()),
        "description": txt::INLINE_DESCRIPTION,
        "input_message_content": {
            "message_text": body,
            "parse_mode": "HTML"
        }
    })]
}

/// Inline-команда «reset all»: сообщение с кнопкой подтверждения сброса всех сделок.
async fn handle_reset_all_inline(query: &InlineQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let uid = query.from.id.0 as i64;
    let token = config::config().bot_token.clone();

    let results = if !config::config().is_allowed(uid) {
        denied_inline_results()
    } else {
        let active_count = queries::get_active_deals().await.map(|d| d.len()).unwrap_or(0);
        reset_all_results(uid, active_count)
    };

    let _ = safe_send::answer_inline_query_with_custom_buttons(&token, query.id.to_string().as_str(), &results)
        .await
        .map_err(|e| tracing::error!("Не удалось ответить на inline «reset all»: {}", e));
    Ok(())
}

/// Inline-команда «pull groups»: список активных и свободных сделочных + итог.
async fn handle_pull_groups_inline(query: &InlineQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let uid = query.from.id.0 as i64;
    let token = config::config().bot_token.clone();

    if !config::config().is_allowed(uid) {
        let _ = safe_send::answer_inline_query_with_custom_buttons(&token, query.id.to_string().as_str(), &denied_inline_results()).await;
        return Ok(());
    }

    let deals = queries::get_active_deals().await.unwrap_or_default();
    let entries = queries::get_all_pool_entries().await.unwrap_or_default();

    let mut by_group: HashMap<i64, _> = deals.iter().map(|d| (d.group_id, d)).collect();
    let mut active_lines = Vec::new();
    let mut free_lines = Vec::new();

    for e in &entries {
        if e.status == "deleted" {
            continue;
        }
        let link_line = e
            .invite_link
            .as_deref()
            .map(|l| format!(" — <a href=\"{}\">ссылка</a>", l))
            .unwrap_or_default();
        match by_group.remove(&e.group_id) {
            Some(d) => active_lines.push(format!(
                "• Сделочная ({}){} — сделка: {}",
                txt::esc_html(&d.title),
                link_line,
                txt::esc_html(&d.title)
            )),
            None => free_lines.push(format!("• Сделочная №{}{}", e.number, link_line)),
        }
    }
    // Активные сделки, чья группа не состоит в пуле.
    for d in by_group.values() {
        let link_line = if d.group_link.is_empty() {
            String::new()
        } else {
            format!(" — <a href=\"{}\">ссылка</a>", d.group_link)
        };
        active_lines.push(format!(
            "• Сделочная ({}){} — сделка: {}",
            txt::esc_html(&d.title),
            link_line,
            txt::esc_html(&d.title)
        ));
    }

    let results = pull_groups_results(active_lines, free_lines);

    let _ = safe_send::answer_inline_query_with_custom_buttons(&token, query.id.to_string().as_str(), &results)
        .await
        .map_err(|e| tracing::error!("Не удалось ответить на inline «pull groups»: {}", e));
    Ok(())
}

/// Команда /рег [сумма] [валюта]: создаёт сделку без inline-режима.
/// Возвращает id отправленного ответа, чтобы вызывающий код мог убедиться,
/// что ответ действительно дошёл до чата.
pub async fn cmd_reg(bot: Bot, msg: Message) -> Result<Option<i64>, Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!(
        "cmd_reg: чат {}, пользователь {}",
        msg.chat.id.0,
        msg.from.as_ref().map(|u| u.id.0).unwrap_or(0)
    );

    let Some(user) = msg.from.as_ref().filter(|u| !u.is_bot) else {
        return Ok(None);
    };

    let arg = msg
        .text()
        .unwrap_or("")
        .trim()
        .splitn(2, ' ')
        .nth(1)
        .unwrap_or("")
        .trim();

    let Some(deal_display) = parse_deal_arg(arg) else {
        tracing::info!("cmd_reg: не разобраны аргументы '{}'", arg);
        let usage = txt::reg_usage_text();
        match safe_send::send_message_cascade(
            config::config().bot_token.as_str(),
            msg.chat.id.0,
            &usage,
            &[],
        )
        .await
        {
            Ok(message_id) => return Ok(Some(message_id)),
            Err(e) => {
                tracing::error!("Не удалось отправить подсказку по /рег: {}", e);
                let _ = bot.send_message(msg.chat.id, "Использование: /рег 10 ton").await;
                return Ok(None);
            }
        }
    };

    let creator_id = user.id.0 as i64;
    let creator_display = user
        .username
        .as_deref()
        .or(Some(user.first_name.as_str()))
        .unwrap_or(&creator_id.to_string())
        .to_string();

    tracing::info!("/рег: '{}' от {} ({})", deal_display, creator_id, creator_display);

    match deal_manager::start_deal_in_pool(&bot, &deal_display, creator_id).await {
        Ok((_deal_id, _entry, link)) => {
            let caption = txt::deal_ready_caption(&txt::esc_html(&deal_display), &creator_display);
            let rows = inline_kb::deal_go_keyboard(&link);
            match safe_send::send_message_cascade(
                config::config().bot_token.as_str(),
                msg.chat.id.0,
                &caption,
                &rows,
            )
            .await
            {
                Ok(message_id) => {
                    tracing::info!("Карточка сделки '{}' отправлена (message_id={})", deal_display, message_id);
                    Ok(Some(message_id))
                }
                Err(e) => {
                    tracing::error!("Не удалось отправить карточку сделки: {}", e);
                    let _ = bot
                        .send_message(msg.chat.id, format!("Сделка создана. Ссылка: {}", link))
                        .await;
                    Ok(None)
                }
            }
        }
        Err(StartDealError::NoFreeGroup) => {
            let text = txt::no_free_pool_text();
            match safe_send::send_message_cascade(
                config::config().bot_token.as_str(),
                msg.chat.id.0,
                &text,
                &[],
            )
            .await
            {
                Ok(message_id) => Ok(Some(message_id)),
                Err(e) => {
                    tracing::error!("Не удалось отправить ответ о свободных сделочных: {}", e);
                    let _ = bot
                        .send_message(msg.chat.id, "Сейчас нет свободных сделочных, попробуйте позже.")
                        .await;
                    Ok(None)
                }
            }
        }
        Err(StartDealError::Register(e)) => {
            tracing::error!("Ошибка регистрации сделки '{}': {}", deal_display, e);
            let error_text = format!(
                "<b>{} Ошибка при создании сделки:</b> {}",
                txt::e(txt::EMOJI_ERROR, "❌"),
                e
            );
            match safe_send::send_message_cascade(
                config::config().bot_token.as_str(),
                msg.chat.id.0,
                &error_text,
                &[],
            )
            .await
            {
                Ok(message_id) => Ok(Some(message_id)),
                Err(send_err) => {
                    tracing::error!("Не удалось отправить сообщение об ошибке: {}", send_err);
                    let _ = bot
                        .send_message(msg.chat.id, format!("Ошибка при создании сделки: {}", e))
                        .await;
                    Ok(None)
                }
            }
        }
    }
}

pub async fn handle_chosen_inline_result(bot: Bot, chosen: ChosenInlineResult) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!("Chosen inline result: {:?}", chosen.result_id);
    let _ = bot;
    let _ = chosen;
    Ok(())
}

pub async fn on_create_deal_callback(bot: Bot, callback: CallbackQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let inline_message_id = match callback.inline_message_id {
        Some(id) => id,
        None => {
            let _ = bot.answer_callback_query(callback.id.clone()).await;
            return Ok(());
        }
    };

    let data = callback.data.unwrap_or_default();
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 {
        let _ = bot.answer_callback_query(callback.id.clone()).await;
        return Ok(());
    }

    let _mode = parts[1];
    let deal_display = parts[2].trim();
    let buyer_id = callback.from.id.0 as i64;
    let buyer_username = callback.from.username.as_deref()
        .or(Some(callback.from.first_name.as_str()))
        .unwrap_or(&buyer_id.to_string())
        .to_string();

    tracing::info!(
        "Callback create deal: deal_display='{}', buyer_id={}, buyer_username='{}'",
        deal_display, buyer_id, buyer_username
    );

    let _ = bot
        .answer_callback_query(callback.id.clone())
        .await
        .map_err(|e| tracing::error!("Не удалось закрыть callback: {}", e));

    match deal_manager::start_deal_in_pool(&bot, deal_display, buyer_id).await {
        Ok((_deal_id, _entry, link)) => {
            let ready_caption = txt::deal_ready_caption(&txt::esc_html(deal_display), &buyer_username);
            let ready_kb = inline_kb::deal_go_keyboard(&link);

            let _ = safe_send::edit_inline_caption(&inline_message_id, &ready_caption, &ready_kb)
                .await
                .map_err(|e| tracing::error!("Не удалось обновить inline-сообщение: {}", e));

            // Также отправляем ссылку в чат как обычное сообщение
            if let Some(msg) = &callback.message {
                let chat_id = msg.chat().id.0;
                let link_text = format!(
                    "{} Сделка создана!\n{}\n{} Перейти в сделку: {}",
                    txt::e(txt::EMOJI_SHIELD, "🛡"),
                    txt::esc_html(deal_display),
                    txt::e(txt::EMOJI_LINK_BTN, "🔗"),
                    link
                );
                let rows = vec![vec![
                    safe_send::btn_url("Перейти в сделку", Some(txt::EMOJI_LINK_BTN), &link),
                ]];
                let _ = safe_send::send_message_cascade(
                    config::config().bot_token.as_str(),
                    chat_id,
                    &link_text,
                    &rows,
                )
                .await
                .map_err(|e| tracing::error!("Не удалось отправить ссылку на сделку: {}", e));
            }
        }
        Err(StartDealError::NoFreeGroup) => {
            let error_text = txt::no_free_pool_text();
            let _ = safe_send::edit_inline_text(&inline_message_id, &error_text, &[])
                .await
                .map_err(|e| tracing::error!("Не удалось обновить inline-сообщение: {}", e));
        }
        Err(StartDealError::Register(e)) => {
            tracing::error!("Ошибка создания сделки через inline: {}", e);
            let error_text = format!(
                "<b>{} Ошибка при подготовке сделочной:</b> {}",
                txt::e(txt::EMOJI_ERROR, "❌"),
                e
            );
            let _ = safe_send::edit_inline_text(&inline_message_id, &error_text, &[])
                .await
                .map_err(|e| tracing::error!("Не удалось обновить inline-сообщение: {}", e));
        }
    }

    Ok(())
}

/// Подтверждение сброса всех сделок (inline «reset all»).
pub async fn on_reset_all_callback(bot: Bot, callback: CallbackQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = callback.data.unwrap_or_default();
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 {
        let _ = bot.answer_callback_query(callback.id.clone()).await;
        return Ok(());
    }

    let creator_id: i64 = parts[2].parse().unwrap_or(0);
    let uid = callback.from.id.0 as i64;
    let cfg = config::config();

    if uid != creator_id || !cfg.is_allowed(uid) {
        let _ = bot
            .answer_callback_query(callback.id.clone())
            .text("Подтвердить может только автор команды.")
            .show_alert(true)
            .await;
        return Ok(());
    }

    let _ = bot
        .answer_callback_query(callback.id.clone())
        .await
        .map_err(|e| tracing::error!("Не удалось закрыть callback: {}", e));

    let keep: std::collections::HashSet<i64> = cfg.allowed_user_ids.iter().copied().collect();
    let (ok, failed) = perform_reset_all(&bot, &keep).await;

    if let Some(inline_message_id) = callback.inline_message_id.as_deref() {
        let text = if ok == 0 && failed == 0 {
            txt::reset_all_empty_text()
        } else {
            txt::reset_all_done_text(ok, failed)
        };
        let _ = safe_send::edit_inline_text(inline_message_id, &text, &[])
            .await
            .map_err(|e| tracing::error!("Не удалось обновить inline-сообщение: {}", e));
    }
    Ok(())
}

/// Сбрасывает все сделочные с активными сделками. Возвращает (успешно, с ошибками).
async fn perform_reset_all(bot: &Bot, keep: &std::collections::HashSet<i64>) -> (usize, usize) {
    let deals = queries::get_active_deals().await.unwrap_or_default();
    if deals.is_empty() {
        return (0, 0);
    }

    let mut ok = 0;
    let mut failed = 0;
    for d in &deals {
        let _ = queries::update_deal_status_by_id(d.id, "completed").await;
        match deal_manager::release_pool_group(bot, d.group_id, keep.clone()).await {
            Ok(_) => ok += 1,
            Err(e) => {
                failed += 1;
                tracing::error!("reset all: не удалось сбросить группу {}: {}", d.group_id, e);
            }
        }
    }
    tracing::info!("reset all: сброшено {}, ошибок {}", ok, failed);
    (ok, failed)
}
