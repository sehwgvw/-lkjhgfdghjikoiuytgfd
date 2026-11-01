pub mod add;
pub mod business;
pub mod group;
pub mod inline;
pub mod start;
pub mod triggers;

use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, ChatMemberUpdated, ChosenInlineResult, InlineQuery, Message};

/// Выделяет команду из текста: "/reset@Bot 1 тон" → "/reset".
/// Точное сравнение обязательно: "/reset" начинается с "/reg" и не должен
/// попадать в обработчик создания сделки.
pub fn command_token(text: &str) -> String {
    let t = text.trim().to_lowercase();
    if !t.starts_with('/') {
        return String::new();
    }
    let end = t.find(char::is_whitespace).unwrap_or(t.len());
    t[..end].split('@').next().unwrap_or("").to_string()
}

pub async fn on_message(bot: Bot, msg: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = msg.text().unwrap_or("");
    let cmd = command_token(text);
    let preview: String = text.chars().take(60).collect();
    tracing::info!(
        "Сообщение: чат={} вид={} от={} текст=\"{}\"",
        msg.chat.id.0,
        match msg.chat.kind {
            teloxide::types::ChatKind::Private(_) => "личный",
            teloxide::types::ChatKind::Public(_) => "группа",
        },
        msg.from.as_ref().map(|u| u.id.0).unwrap_or(0),
        preview
    );

    // /start, /help, /id commands
    if cmd == "/start" {
        return start::cmd_start(bot, msg).await.map_err(|e| e.into());
    }
    if cmd == "/help" {
        return start::cmd_help(bot, msg).await.map_err(|e| e.into());
    }
    if cmd == "/id" || cmd == "/my_id" || cmd == "/айди" {
        return start::cmd_id(bot, msg).await.map_err(|e| e.into());
    }

    // /add command
    if cmd == "/add" {
        return add::cmd_add(bot, msg).await.map_err(|e| e.into());
    }

    // /рег command — создание сделки без inline-режима
    if cmd == "/рег" || cmd == "/reg" {
        return inline::cmd_reg(bot, msg).await.map(|_| ()).map_err(|e| e.into());
    }

    // Неизвестная команда — сообщаем в лог, чтобы потеря запроса не была тихой.
    if !cmd.is_empty() {
        tracing::warn!("Неизвестная команда: '{}'", cmd);
    }

    // Group commands
    if matches!(msg.chat.kind, teloxide::types::ChatKind::Public(_)) {
        return group::on_group_message(bot, msg).await.map_err(|e| e.into());
    }

    // Private chat commands for triggers and sms
    if matches!(msg.chat.kind, teloxide::types::ChatKind::Private(_)) {
        return triggers::on_private_message(bot, msg).await.map_err(|e| e.into());
    }

    Ok(())
}

pub async fn on_callback_query(bot: Bot, cb: CallbackQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = cb.data.clone().unwrap_or_default();
    if data.starts_with("cd:") {
        return inline::on_create_deal_callback(bot, cb).await.map_err(|e| e.into());
    }
    if data.starts_with("ruchenik:confirm:") {
        return group::on_ruchenik_confirm(bot, cb).await.map_err(|e| e.into());
    }
    if data.starts_with("ruchenik:cancel:") {
        return group::on_ruchenik_cancel(bot, cb).await.map_err(|e| e.into());
    }
    if data.starts_with("deal_cancel:yes:") {
        return group::on_deal_cancel_confirm(bot, cb).await.map_err(|e| e.into());
    }
    if data.starts_with("deal_cancel:no:") {
        return group::on_deal_cancel_reject(bot, cb).await.map_err(|e| e.into());
    }
    if data.starts_with("resetall:confirm:") {
        return inline::on_reset_all_callback(bot, cb).await.map_err(|e| e.into());
    }
    Ok(())
}

pub async fn on_inline_query(bot: Bot, query: InlineQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    inline::handle_inline_query(bot, query).await.map_err(|e| e.into())
}

pub async fn on_chosen_inline_result(bot: Bot, chosen: ChosenInlineResult) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    inline::handle_chosen_inline_result(bot, chosen).await.map_err(|e| e.into())
}

pub async fn on_chat_member_updated(bot: Bot, event: ChatMemberUpdated) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    group::on_member_join(event, bot).await.map_err(|e| e.into())
}
