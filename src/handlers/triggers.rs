use std::collections::HashMap;
use std::sync::OnceLock;

use teloxide::prelude::*;
use teloxide::types::{Message, ParseMode};

use crate::database::queries;
use crate::texts::messages as txt;

static DIALOGS: OnceLock<std::sync::RwLock<HashMap<i64, String>>> = OnceLock::new();
static DIALOG_STATES: OnceLock<std::sync::RwLock<HashMap<String, String>>> = OnceLock::new();

fn dialogs() -> &'static std::sync::RwLock<HashMap<i64, String>> {
    DIALOGS.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

fn dialog_states() -> &'static std::sync::RwLock<HashMap<String, String>> {
    DIALOG_STATES.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

const MAX_TRIGGERS: i64 = 50;
const MAX_TRIGGER_LENGTH: usize = 64;
const MAX_SMS: i64 = 50;

fn is_private(msg: &Message) -> bool {
    matches!(msg.chat.kind, teloxide::types::ChatKind::Private(_))
}

fn first_word_eq(msg: &Message, word: &str) -> bool {
    msg.text()
        .map(|t| t.trim_start().split_whitespace().next().map_or(false, |p| p.eq_ignore_ascii_case(word)))
        .unwrap_or(false)
}

pub async fn on_private_message(bot: Bot, msg: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = msg.text().unwrap_or("");
    let low = text.to_lowercase();

    // /сms or /sms command
    if low.starts_with("/сms") || low.starts_with("/сms@") || low.starts_with("/sms") || low.starts_with("/sms@") {
        return cmd_sms_list(bot, msg).await.map_err(|e| e.into());
    }
    // /команды or /commands
    if low.starts_with("/команды") || low.starts_with("/команды@") || low.starts_with("/commands") || low.starts_with("/commands@") {
        return cmd_list_commands(bot, msg).await.map_err(|e| e.into());
    }

    // Dialog state handlers
    if is_private(&msg) {
        let uid = msg.from.as_ref().map_or(0, |u| u.id.0 as i64);
        let dialogs_guard = dialogs().read().unwrap();
        let state = dialogs_guard.get(&uid).cloned();
        drop(dialogs_guard);

        if state.as_deref() == Some("sms_count") {
            return on_sms_count_input(bot, msg).await.map_err(|e| e.into());
        }
        if state.as_deref() == Some("sms_response") {
            return on_sms_response_message(bot, msg).await.map_err(|e| e.into());
        }
        if state.as_deref() == Some("sms_delete") {
            return on_sms_delete_input(bot, msg).await.map_err(|e| e.into());
        }
        if state.as_deref() == Some("trigger") {
            return on_trigger_input(bot, msg).await.map_err(|e| e.into());
        }
        if state.as_deref() == Some("response") {
            return on_response_message(bot, msg).await.map_err(|e| e.into());
        }
        if state.as_deref() == Some("delete") {
            return on_delete_input(bot, msg).await.map_err(|e| e.into());
        }
    }

    // Command starters
    if first_word_eq(&msg, "+команда") || first_word_eq(&msg, "+command") {
        return on_add_command(bot, msg).await.map_err(|e| e.into());
    }
    if first_word_eq(&msg, "-команда") || first_word_eq(&msg, "-command") {
        return on_delete_command(bot, msg).await.map_err(|e| e.into());
    }
    if first_word_eq(&msg, "+sms") || first_word_eq(&msg, "+сms") {
        return on_sms_add(bot, msg).await.map_err(|e| e.into());
    }
    if first_word_eq(&msg, "-sms") || first_word_eq(&msg, "-сms") {
        return on_sms_delete(bot, msg).await.map_err(|e| e.into());
    }

    Ok(())
}

// ... (rest of the handler functions remain the same as before)
async fn on_add_command(bot: Bot, msg: Message) -> ResponseResult<()> {
    if !is_private(&msg) {
        return Ok(());
    }
    let user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let uid = user.id.0 as i64;

    let count = match queries::count_custom_commands().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("count_custom_commands error: {}", e);
            return Ok(());
        }
    };
    if count >= MAX_TRIGGERS {
        let _ = bot.send_message(teloxide::types::ChatId(uid), txt::trigger_limit()).await;
        return Ok(());
    }

    dialogs().write().unwrap().insert(uid, "trigger".to_string());
    let _ = bot.send_message(teloxide::types::ChatId(uid), txt::trigger_add_start())
        .parse_mode(ParseMode::Html)
        .await;
    Ok(())
}

async fn on_delete_command(bot: Bot, msg: Message) -> ResponseResult<()> {
    if !is_private(&msg) {
        return Ok(());
    }
    let user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let uid = user.id.0 as i64;

    let arg = msg.text().map(|t| t.splitn(2, ' ').nth(1).unwrap_or("").trim().to_string());

    if let Some(arg) = arg.filter(|a| !a.is_empty()) {
        return delete_by_trigger(bot, msg, &arg).await;
    }

    let commands = match queries::get_all_custom_commands().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("get_all_custom_commands error: {}", e);
            return Ok(());
        }
    };
    if commands.is_empty() {
        let _ = bot.send_message(teloxide::types::ChatId(uid), txt::trigger_empty())
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    dialogs().write().unwrap().insert(uid, "delete".to_string());
    let list = format_command_list(&commands);
    let _ = bot.send_message(teloxide::types::ChatId(uid), txt::trigger_delete_prompt(&list))
        .parse_mode(ParseMode::Html)
        .await;
    Ok(())
}

async fn delete_by_trigger(bot: Bot, msg: Message, trigger: &str) -> ResponseResult<()> {
    let deleted = match queries::delete_custom_command_by_trigger(trigger).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("delete_custom_command_by_trigger error: {}", e);
            return Ok(());
        }
    };
    match deleted {
        Some(cmd) => {
            let _ = bot.send_message(msg.chat.id, txt::trigger_deleted(&cmd.trigger_text))
                .parse_mode(ParseMode::Html)
                .await;
            tracing::info!("-команда: удалён триггер '{}'", cmd.trigger_text);
        }
        None => {
            let _ = bot.send_message(msg.chat.id, txt::trigger_not_found())
                .parse_mode(ParseMode::Html)
                .await;
        }
    }
    Ok(())
}

async fn on_sms_add(bot: Bot, msg: Message) -> ResponseResult<()> {
    if !is_private(&msg) {
        return Ok(());
    }
    let user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let uid = user.id.0 as i64;

    let count = match queries::count_custom_sms().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("count_custom_sms error: {}", e);
            return Ok(());
        }
    };
    if count >= MAX_SMS {
        let _ = bot.send_message(teloxide::types::ChatId(uid), txt::sms_limit()).await;
        return Ok(());
    }

    dialogs().write().unwrap().insert(uid, "sms_count".to_string());
    let _ = bot.send_message(teloxide::types::ChatId(uid), txt::sms_add_start())
        .parse_mode(ParseMode::Html)
        .await;
    Ok(())
}

async fn on_sms_delete(bot: Bot, msg: Message) -> ResponseResult<()> {
    if !is_private(&msg) {
        return Ok(());
    }
    let user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let uid = user.id.0 as i64;

    let arg = msg.text().map(|t| t.splitn(2, ' ').nth(1).unwrap_or("").trim().to_string());

    if let Some(arg) = arg.filter(|a| !a.is_empty()) {
        return delete_sms_by_count(bot, msg, &arg).await;
    }

    let entries = match queries::get_all_custom_sms().await {
        Ok(e) => e,
        Err(err) => {
            tracing::error!("get_all_custom_sms error: {}", err);
            return Ok(());
        }
    };
    if entries.is_empty() {
        let _ = bot.send_message(msg.chat.id, txt::sms_empty())
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    dialogs().write().unwrap().insert(uid, "sms_delete".to_string());
    let list = format_sms_list(&entries);
    let _ = bot.send_message(msg.chat.id, txt::sms_delete_prompt(&list))
        .parse_mode(ParseMode::Html)
        .await;
    Ok(())
}

async fn delete_sms_by_count(bot: Bot, msg: Message, raw: &str) -> ResponseResult<()> {
    let raw = raw.split_whitespace().next().unwrap_or("");
    if !raw.chars().all(|c| c.is_ascii_digit()) || raw.parse::<i32>().map_or(true, |n| n <= 0) {
        let _ = bot.send_message(msg.chat.id, txt::sms_bad_count())
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    let count: i32 = raw.parse().unwrap();
    let deleted = match queries::delete_custom_sms_by_count(count).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("delete_custom_sms_by_count error: {}", e);
            return Ok(());
        }
    };
    match deleted {
        Some(sms) => {
            let _ = bot.send_message(msg.chat.id, txt::sms_deleted(sms.people_count))
                .parse_mode(ParseMode::Html)
                .await;
            tracing::info!("-sms: удалено sms при {} людях", sms.people_count);
        }
        None => {
            let _ = bot.send_message(msg.chat.id, txt::sms_not_found())
                .parse_mode(ParseMode::Html)
                .await;
        }
    }
    Ok(())
}

async fn cmd_sms_list(bot: Bot, msg: Message) -> ResponseResult<()> {
    let entries = match queries::get_all_custom_sms().await {
        Ok(e) => e,
        Err(err) => {
            tracing::error!("get_all_custom_sms error: {}", err);
            return Ok(());
        }
    };
    let _ = bot.send_message(msg.chat.id, txt::sms_list_text(&entries))
        .parse_mode(ParseMode::Html)
        .await;
    Ok(())
}

async fn on_sms_count_input(bot: Bot, msg: Message) -> ResponseResult<()> {
    let user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let uid = user.id.0 as i64;

    let dialogs_guard = dialogs().read().unwrap();
    let state = dialogs_guard.get(&uid).cloned();
    drop(dialogs_guard);

    if state.as_deref() != Some("sms_count") {
        return Ok(());
    }

    let raw = msg.text().unwrap_or("").trim();
    if !raw.chars().all(|c| c.is_ascii_digit()) || raw.parse::<i32>().map_or(true, |n| n <= 0) {
        let _ = bot.send_message(teloxide::types::ChatId(uid), txt::sms_bad_count())
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    let count: i32 = raw.parse().unwrap();
    let existing = match queries::get_custom_sms_by_count(count).await {
        Ok(e) => e,
        Err(err) => {
            tracing::error!("get_custom_sms_by_count error: {}", err);
            return Ok(());
        }
    };
    if existing.is_some() {
        let _ = bot.send_message(teloxide::types::ChatId(uid), txt::sms_exists())
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    dialogs().write().unwrap().insert(uid, "sms_response".to_string());
    dialog_states().write().unwrap().insert(uid.to_string(), count.to_string());
    let _ = bot.send_message(teloxide::types::ChatId(uid), txt::sms_ask_response())
        .parse_mode(ParseMode::Html)
        .await;
    Ok(())
}

async fn on_sms_response_message(bot: Bot, msg: Message) -> ResponseResult<()> {
    let user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let uid = user.id.0 as i64;

    let dialogs_guard = dialogs().read().unwrap();
    let state = dialogs_guard.get(&uid).cloned();
    drop(dialogs_guard);

    if state.as_deref() != Some("sms_response") {
        return Ok(());
    }

    let states_guard = dialog_states().read().unwrap();
    let raw = states_guard.get(&uid.to_string()).cloned();
    drop(states_guard);

    let Some(raw) = raw else {
        dialogs().write().unwrap().remove(&uid);
        return Ok(());
    };

    let count: i32 = raw.parse().unwrap();
    dialogs().write().unwrap().remove(&uid);
    dialog_states().write().unwrap().remove(&uid.to_string());

    let sms_id = match queries::add_custom_sms(count, msg.chat.id.0, msg.id.0 as i64, Some(uid)).await {
        Ok(id) => id,
        Err(err) => {
            tracing::error!("add_custom_sms error: {}", err);
            return Ok(());
        }
    };

    match sms_id {
        Some(_) => {
            let _ = bot.send_message(teloxide::types::ChatId(uid), txt::sms_saved(count))
                .parse_mode(ParseMode::Html)
                .await;
            tracing::info!("+sms сохранено: при {} людях (msg {}/{})", count, msg.chat.id.0, msg.id.0);
        }
        None => {
            let _ = bot.send_message(teloxide::types::ChatId(uid), txt::sms_exists())
                .parse_mode(ParseMode::Html)
                .await;
        }
    }
    Ok(())
}

async fn on_sms_delete_input(bot: Bot, msg: Message) -> ResponseResult<()> {
    let user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let uid = user.id.0 as i64;

    let dialogs_guard = dialogs().read().unwrap();
    let state = dialogs_guard.get(&uid).cloned();
    drop(dialogs_guard);

    if state.as_deref() != Some("sms_delete") {
        return Ok(());
    }

    dialogs().write().unwrap().remove(&uid);
    let raw = msg.text().unwrap_or("").to_string();
    delete_sms_by_count(bot, msg, &raw).await
}

async fn cmd_list_commands(bot: Bot, msg: Message) -> ResponseResult<()> {
    let commands = match queries::get_all_custom_commands().await {
        Ok(c) => c,
        Err(err) => {
            tracing::error!("get_all_custom_commands error: {}", err);
            return Ok(());
        }
    };
    let _ = bot.send_message(msg.chat.id, txt::triggers_list_text(&commands))
        .parse_mode(ParseMode::Html)
        .await;
    Ok(())
}

async fn on_trigger_input(bot: Bot, msg: Message) -> ResponseResult<()> {
    let user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let uid = user.id.0 as i64;

    let dialogs_guard = dialogs().read().unwrap();
    let state = dialogs_guard.get(&uid).cloned();
    drop(dialogs_guard);

    if state.as_deref() != Some("trigger") {
        return Ok(());
    }

    let trigger = msg.text().unwrap_or("").trim().to_string();
    if trigger.is_empty() {
        return Ok(());
    }

    if trigger.len() > MAX_TRIGGER_LENGTH {
        let _ = bot.send_message(teloxide::types::ChatId(uid), txt::trigger_too_long())
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    let existing = match queries::get_custom_command_by_trigger(&trigger).await {
        Ok(e) => e,
        Err(err) => {
            tracing::error!("get_custom_command_by_trigger error: {}", err);
            return Ok(());
        }
    };
    if existing.is_some() {
        let _ = bot.send_message(teloxide::types::ChatId(uid), txt::trigger_exists())
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    dialogs().write().unwrap().insert(uid, "response".to_string());
    dialog_states().write().unwrap().insert(uid.to_string(), trigger.clone());
    let _ = bot.send_message(teloxide::types::ChatId(uid), txt::trigger_ask_response())
        .parse_mode(ParseMode::Html)
        .await;
    Ok(())
}

async fn on_response_message(bot: Bot, msg: Message) -> ResponseResult<()> {
    let user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let uid = user.id.0 as i64;

    let dialogs_guard = dialogs().read().unwrap();
    let state = dialogs_guard.get(&uid).cloned();
    drop(dialogs_guard);

    if state.as_deref() != Some("response") {
        return Ok(());
    }

    let states_guard = dialog_states().read().unwrap();
    let trigger = states_guard.get(&uid.to_string()).cloned();
    drop(states_guard);

    let Some(trigger) = trigger else {
        dialogs().write().unwrap().remove(&uid);
        return Ok(());
    };

    dialogs().write().unwrap().remove(&uid);
    dialog_states().write().unwrap().remove(&uid.to_string());

    let command_id = match queries::add_custom_command(&trigger, msg.chat.id.0, msg.id.0 as i64, Some(uid)).await {
        Ok(id) => id,
        Err(err) => {
            tracing::error!("add_custom_command error: {}", err);
            return Ok(());
        }
    };

    match command_id {
        Some(_) => {
            let _ = bot.send_message(teloxide::types::ChatId(uid), txt::trigger_saved(&trigger))
                .parse_mode(ParseMode::Html)
                .await;
            tracing::info!("+команда создана: '{}' (msg {}/{})", trigger, msg.chat.id.0, msg.id.0);
        }
        None => {
            let _ = bot.send_message(teloxide::types::ChatId(uid), txt::trigger_exists())
                .parse_mode(ParseMode::Html)
                .await;
        }
    }
    Ok(())
}

async fn on_delete_input(bot: Bot, msg: Message) -> ResponseResult<()> {
    let user = match msg.from.as_ref() {
        Some(u) => u,
        None => return Ok(()),
    };
    let uid = user.id.0 as i64;

    let dialogs_guard = dialogs().read().unwrap();
    let state = dialogs_guard.get(&uid).cloned();
    drop(dialogs_guard);

    if state.as_deref() != Some("delete") {
        return Ok(());
    }

    dialogs().write().unwrap().remove(&uid);
    let trigger = msg.text().unwrap_or("").to_string();
    if trigger.is_empty() {
        return Ok(());
    }
    delete_by_trigger(bot, msg, &trigger).await
}

fn format_command_list(commands: &[crate::database::models::CustomCommand]) -> String {
    if commands.is_empty() {
        return "• (пусто)".to_string();
    }
    commands
        .iter()
        .map(|c| format!("• <code>{}</code>", c.trigger_text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_sms_list(entries: &[crate::database::models::CustomSms]) -> String {
    if entries.is_empty() {
        return "• (пусто)".to_string();
    }
    entries
        .iter()
        .map(|s| format!("• при <code>{}</code> людях", s.people_count))
        .collect::<Vec<_>>()
        .join("\n")
}
