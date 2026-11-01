use std::collections::HashMap;
use std::sync::OnceLock;

use teloxide::prelude::*;
use teloxide::requests::Requester;
use teloxide::types::{CallbackQuery, ChatMemberUpdated, Message, ParseMode};

use crate::database::queries;
use crate::services::deal_manager;
use crate::services::safe_send;
use crate::keyboards::inline_kb;
use crate::texts::messages as txt;

static JOIN_MESSAGE_TO_USER_ID: OnceLock<std::sync::RwLock<HashMap<i32, i64>>> = OnceLock::new();
static DEAL_CARDS_SENT: OnceLock<std::sync::RwLock<HashMap<i64, bool>>> = OnceLock::new();

fn join_msg_map() -> &'static std::sync::RwLock<HashMap<i32, i64>> {
    JOIN_MESSAGE_TO_USER_ID.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

fn deal_cards_sent() -> &'static std::sync::RwLock<HashMap<i64, bool>> {
    DEAL_CARDS_SENT.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

const RESET_COMMANDS: &[&str] = &["/ресет", "/reset", ".ресет", ".reset"];
const CANCEL_COMMANDS: &[&str] = &["/отмена", "/cancel", ".отмена", ".cancel"];

fn user_display_link(user_id: i64, username: Option<&str>, first_name: Option<&str>) -> String {
    if let Some(username) = username {
        let clean = username.trim_start_matches('@');
        return format!(r#"<a href="https://t.me/{}">@{}</a>"#, clean, clean);
    }
    let name = first_name.unwrap_or("Unknown");
    format!(r#"<a href="tg://user?id={}">{}</a>"#, user_id, name)
}

fn parse_ruchenik_command(text: &str, _bot_username: Option<&str>) -> (bool, Option<String>) {
    let cleaned = text.trim();
    let text_without_mentions = regex::Regex::new(r"@([a-zA-Z0-9_]{4,32})")
        .unwrap()
        .replace_all(&cleaned, "")
        .to_lowercase();
    let text_without_mentions = regex::Regex::new(r"\s+")
        .unwrap()
        .replace_all(&text_without_mentions, " ")
        .trim_matches(|c| " .!?,".contains(c))
        .to_string();
    let ruch_keywords = ["руч", "рученик", "/руч", "/ruchenik"];
    let mut is_ruch = false;
    for kw in &ruch_keywords {
        if text_without_mentions == *kw
            || text_without_mentions.starts_with(&format!("{} ", kw))
            || text_without_mentions.ends_with(&format!(" {}", kw))
            || text_without_mentions.is_empty()
        {
            is_ruch = true;
            break;
        }
    }
    (is_ruch, None)
}

fn is_group_or_supergroup(msg: &Message) -> bool {
    matches!(msg.chat.kind, teloxide::types::ChatKind::Public(_))
}

pub async fn on_group_message(bot: Bot, msg: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let user = match msg.from.as_ref() {
        Some(u) if !u.is_bot => u,
        _ => return Ok(()),
    };

    let group_id = msg.chat.id.0;
    let user_id = user.id.0 as i64;
    let text = (msg.text().or(msg.caption())).unwrap_or("").trim();

    // Сначала проверяем команды управления сделкой (работают даже без активной сделки)
    let low_text = text.to_lowercase();

    if RESET_COMMANDS.contains(&low_text.as_str()) {
        return handle_reset(bot, msg, None).await.map_err(|e| e.into());
    }

    if CANCEL_COMMANDS.contains(&low_text.as_str()) {
        return handle_cancel_prompt(bot, msg, None).await.map_err(|e| e.into());
    }

    // Проверяем, есть ли активная сделка в этой группе
    let deal_opt = queries::get_deal_by_group(group_id).await;

    let deal = match deal_opt {
        Ok(Some(d)) => d,
        _ => {
            // Нет активной сделки — проверяем триггеры глобально
            if !text.is_empty() {
                if let Ok(cmd) = queries::get_custom_command_by_trigger(text).await {
                    if let Some(cmd) = cmd {
                        let _ = bot.copy_message(msg.chat.id, msg.chat.id, teloxide::types::MessageId(cmd.source_message_id as i32)).await;
                    }
                }
            }
            return Ok(());
        }
    };

    deal_manager::add_human_member(group_id, user_id);
    let _ = queries::upsert_user(user_id, user.username.as_deref(), Some(&user.first_name)).await;

    if !text.is_empty() {
        let _ = queries::log_message(deal.id, user_id, user.username.as_deref(), text).await;
    }

    if RESET_COMMANDS.contains(&low_text.as_str()) {
        return handle_reset(bot, msg, Some(deal.clone())).await.map_err(|e| e.into());
    }

    if CANCEL_COMMANDS.contains(&low_text.as_str()) {
        return handle_cancel_prompt(bot, msg, Some(deal.clone())).await.map_err(|e| e.into());
    }

    let bot_me = bot.get_me().await?;
    let (is_ruch, _target) = parse_ruchenik_command(text, bot_me.username.as_deref());
    if is_ruch {
        return handle_ruch_command(bot, msg, deal, None).await.map_err(|e| e.into());
    }

    if !text.is_empty() {
        if let Ok(cmd) = queries::get_custom_command_by_trigger(text).await {
            if let Some(cmd) = cmd {
                let _ = bot.copy_message(msg.chat.id, msg.chat.id, teloxide::types::MessageId(cmd.source_message_id as i32)).await;
            }
        }
    }

    Ok(())
}

async fn handle_reset(bot: Bot, msg: Message, deal: Option<crate::database::models::Deal>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = crate::config::config();
    let user_id = msg.from.as_ref().map_or(0, |u| u.id.0 as i64);

    let guarantor_id = deal.as_ref().and_then(|d| d.guarantor_id.or(Some(d.buyer_id)));

    if let Some(gid) = guarantor_id {
        if user_id != gid && !config.is_allowed(user_id) {
            let _ = bot.send_message(msg.chat.id, txt::reset_denied_text())
                .parse_mode(ParseMode::Html)
                .await;
            return Ok(());
        }
    } else if !config.is_allowed(user_id) {
        let _ = bot.send_message(msg.chat.id, txt::reset_denied_text())
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    let keep_ids: std::collections::HashSet<i64> = std::iter::once(user_id)
        .chain(guarantor_id)
        .chain(config.allowed_user_ids.iter().copied())
        .collect();

    let gid = msg.chat.id.0;
    let number = deal_manager::release_pool_group(&bot, gid, keep_ids).await?;

    if let Some(d) = deal {
        let _ = queries::update_deal_status(gid, "completed").await;
        let _ = queries::update_deal_status_by_id(d.id, "completed").await;
    }

    let _ = bot.send_message(msg.chat.id, txt::reset_done_text(number))
        .parse_mode(ParseMode::Html)
        .await;
    tracing::info!("Сделка сброшена (группа {})", gid);
    Ok(())
}

async fn handle_cancel_prompt(bot: Bot, msg: Message, deal: Option<crate::database::models::Deal>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = crate::config::config();
    let user_id = msg.from.as_ref().map_or(0, |u| u.id.0 as i64);

    let guarantor_id = deal.as_ref().and_then(|d| d.guarantor_id.or(Some(d.buyer_id)));

    if let Some(gid) = guarantor_id {
        if user_id != gid && !config.is_allowed(user_id) {
            let _ = bot.send_message(msg.chat.id, txt::reset_denied_text())
                .parse_mode(ParseMode::Html)
                .await;
            return Ok(());
        }
    } else if !config.is_allowed(user_id) {
        let _ = bot.send_message(msg.chat.id, txt::reset_denied_text())
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    let photo = safe_send::banner_photo("otmena_banner", None);
    let _ = safe_send::send_banner(
        bot,
        msg.chat.id.0,
        photo,
        &txt::cancel_prompt_caption(),
        &inline_kb::cancel_deal_keyboard(msg.chat.id.0),
    ).await;
    Ok(())
}

async fn handle_ruch_command(
    bot: Bot,
    msg: Message,
    deal: crate::database::models::Deal,
    _target: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let guarantor_id = deal.guarantor_id.unwrap_or(deal.buyer_id);
    let user_id = msg.from.as_ref().map_or(0, |u| u.id.0 as i64);

    if user_id != guarantor_id {
        return Ok(());
    }

    let bot_me = bot.get_me().await?;
    let others = deal_manager::get_other_human_members(msg.chat.id.0, guarantor_id);

    let target_user_id = if others.len() == 1 {
        others[0]
    } else {
        return Ok(());
    };

    if target_user_id == bot_me.id.0 as i64 {
        let _ = bot.send_message(msg.chat.id, format!("<b>{} Бот не может быть ручеником.</b>", txt::e(txt::EMOJI_ERROR, "❌")))
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    if target_user_id == guarantor_id {
        let _ = bot.send_message(msg.chat.id, format!("<b>{} Гарант не может назначить ручеником самого себя.</b>", txt::e(txt::EMOJI_ERROR, "❌")))
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    let db_u = queries::get_user_by_id(target_user_id).await?;
    let target_link = match db_u {
        Some(u) => user_display_link(target_user_id, u.username.as_deref(), u.first_name.as_deref()),
        None => user_display_link(target_user_id, None, None),
    };

    let prompt_text = txt::ruch_confirm_caption(&target_link);
    let photo = safe_send::banner_photo("ruch_banner", None);
    let _ = safe_send::send_banner(
        bot,
        msg.chat.id.0,
        photo,
        &prompt_text,
        &inline_kb::ruchenik_keyboard(deal.id, target_user_id),
    ).await;

    Ok(())
}

pub async fn on_member_join(event: ChatMemberUpdated, bot: Bot) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let group_id = event.chat.id.0;
    let user = event.new_chat_member.user;
    let user_id = user.id.0 as i64;

    if user.is_bot {
        return Ok(());
    }

    let deal = match queries::get_deal_by_group(group_id).await {
        Ok(Some(d)) => d,
        _ => return Ok(()),
    };

    let _ = queries::upsert_user(user_id, user.username.as_deref(), Some(&user.first_name)).await;

    let role = deal_manager::on_member_joined(group_id, user_id, &deal).await;
    if role.is_none() {
        return Ok(());
    }

    let display_name = user.username.as_deref().unwrap_or(&user.first_name).to_string();

    match role.as_deref() {
        Some("guarantor") => {
            deal_manager::ensure_guarantor_present(&bot, group_id, user_id).await;
            let (ready, _, _, _) = deal_manager::check_deal_participants(group_id, &deal);
            if ready {
                let _ = queries::update_deal_status(group_id, "active").await;
                send_deal_card(bot.clone(), group_id, &deal).await?;
            }
            check_sms(&bot, group_id, &deal).await?;
        }
        _ => {
            let sent_msg = bot.send_message(
                teloxide::types::ChatId(group_id),
                txt::MEMBER_JOINED.replace("{name}", &display_name),
            ).parse_mode(ParseMode::Html).await?;
            join_msg_map().write().unwrap().insert(sent_msg.id.0 as i32, user_id);
            let guarantor_id = deal.guarantor_id.unwrap_or(deal.buyer_id);
            if guarantor_id > 0 {
                deal_manager::ensure_guarantor_present(&bot, group_id, guarantor_id).await;
            }
            let (ready, _, _, _) = deal_manager::check_deal_participants(group_id, &deal);
            if ready {
                let _ = queries::update_deal_status(group_id, "active").await;
                send_deal_card(bot.clone(), group_id, &deal).await?;
            }
            check_sms(&bot, group_id, &deal).await?;
        }
    }
    Ok(())
}

async fn send_deal_card(bot: Bot, group_id: i64, _deal: &crate::database::models::Deal) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    {
        let mut sent = deal_cards_sent().write().unwrap();
        if sent.contains_key(&group_id) {
            return Ok(());
        }
        sent.insert(group_id, true);
    }

    let deal = match queries::get_deal_by_group(group_id).await {
        Ok(Some(d)) => d,
        _ => return Ok(()),
    };

    let guarantor_name = if let Some(gid) = deal.guarantor_id.or(Some(deal.buyer_id)) {
        if let Ok(Some(u)) = queries::get_user_by_id(gid).await {
            u.username.as_deref().unwrap_or(u.first_name.as_deref().unwrap_or("")).to_string()
        } else {
            "Гарант".to_string()
        }
    } else {
        "Гарант".to_string()
    };

    let caption = txt::deal_ready_caption(&deal.title, &guarantor_name);
    let photo = safe_send::banner_photo("group_banner", None);

    let _ = safe_send::send_banner(
        bot,
        group_id,
        photo,
        &caption,
        &inline_kb::rules_keyboard(),
    ).await;
    Ok(())
}

async fn check_sms(bot: &Bot, group_id: i64, deal: &crate::database::models::Deal) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let member_count = match bot.get_chat_member_count(teloxide::types::ChatId(group_id)).await {
        Ok(c) => c as i32,
        Err(e) => {
            tracing::debug!("Не удалось получить число участников {}: {}", group_id, e);
            return Ok(());
        }
    };

    let entries = match queries::get_sms_to_fire(deal.id, member_count).await {
        Ok(e) => e,
        Err(err) => {
            tracing::error!("get_sms_to_fire error: {}", err);
            return Ok(());
        }
    };
    for sms in entries {
        if !queries::mark_sms_fired(deal.id, sms.id).await? {
            continue;
        }
        let _ = bot.copy_message(
            teloxide::types::ChatId(group_id),
            teloxide::types::ChatId(sms.source_chat_id),
            teloxide::types::MessageId(sms.source_message_id as i32),
        ).await;
    }
    Ok(())
}

pub async fn on_ruchenik_confirm(bot: Bot, callback: CallbackQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = callback.data.unwrap_or_default();
    let parts: Vec<&str> = data.splitn(4, ':').collect();
    if parts.len() < 4 {
        let _ = bot.answer_callback_query(callback.id.clone()).await;
        return Ok(());
    }
    let deal_id: i32 = parts[2].parse().unwrap_or(0);
    let target_user_id: i64 = parts[3].parse().unwrap_or(0);

    let deal = match queries::get_deal_by_id(deal_id).await {
        Ok(Some(d)) => d,
        _ => {
            let _ = bot.answer_callback_query(callback.id.clone()).await;
            return Ok(());
        }
    };

    let config = crate::config::config();
    let guarantor_id = deal.guarantor_id.unwrap_or(deal.buyer_id);
    let user_id = callback.from.id.0 as i64;

    if user_id != guarantor_id && !config.is_allowed(user_id) {
        let _ = bot.answer_callback_query(callback.id.clone()).await;
        return Ok(());
    }

    deal_manager::set_deal_ruchenik_inner(deal.id, target_user_id).await;

    let db_u = queries::get_user_by_id(target_user_id).await?;
    let target_link = match db_u {
        Some(u) => user_display_link(target_user_id, u.username.as_deref(), u.first_name.as_deref()),
        None => user_display_link(target_user_id, None, None),
    };

    let _ = bot.answer_callback_query(callback.id.clone()).await;

    if let Some(msg) = callback.message {
        let chat_id = msg.chat().id.0;
        let msg_id = msg.id().0 as i32;
        let _ = safe_send::edit_caption(
            chat_id,
            msg_id,
            &txt::ruch_success_caption(&target_link),
            &inline_kb::rules_keyboard(),
        ).await;
    }
    Ok(())
}

pub async fn on_ruchenik_cancel(bot: Bot, callback: CallbackQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = callback.data.unwrap_or_default();
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 {
        let _ = bot.answer_callback_query(callback.id.clone()).await;
        return Ok(());
    }
    let deal_id: i32 = parts[2].parse().unwrap_or(0);
    let deal = match queries::get_deal_by_id(deal_id).await {
        Ok(Some(d)) => d,
        _ => {
            let _ = bot.answer_callback_query(callback.id.clone()).await;
            return Ok(());
        }
    };
    let config = crate::config::config();
    let guarantor_id = deal.guarantor_id.unwrap_or(deal.buyer_id);
    let user_id = callback.from.id.0 as i64;
    if user_id != guarantor_id && !config.is_allowed(user_id) {
        let _ = bot.answer_callback_query(callback.id.clone()).await;
        return Ok(());
    }
    let _ = bot.answer_callback_query(callback.id.clone()).await;
    if let Some(msg) = callback.message {
        let chat_id = msg.chat().id.0;
        let msg_id = msg.id().0 as i32;
        let _ = safe_send::edit_caption(
            chat_id,
            msg_id,
            &txt::ruch_rejected_caption(),
            &[],
        ).await;
    }
    Ok(())
}

pub async fn on_deal_cancel_confirm(bot: Bot, callback: CallbackQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = callback.data.unwrap_or_default();
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 {
        let _ = bot.answer_callback_query(callback.id.clone()).await;
        return Ok(());
    }
    let group_id: i64 = parts[2].parse().unwrap_or(0);
    let deal = match queries::get_deal_by_group(group_id).await {
        Ok(Some(d)) => d,
        _ => {
            let _ = bot.answer_callback_query(callback.id.clone()).await;
            return Ok(());
        }
    };
    let config = crate::config::config();
    let user_id = callback.from.id.0 as i64;
    let guarantor_id = deal.guarantor_id.unwrap_or(deal.buyer_id);
    if user_id != guarantor_id && !config.is_allowed(user_id) {
        let _ = bot.answer_callback_query(callback.id.clone()).await;
        return Ok(());
    }
    let _ = bot.answer_callback_query(callback.id.clone()).await;
    if let Some(msg) = callback.message {
        let chat_id = msg.chat().id.0;
        let msg_id = msg.id().0 as i32;
        let _ = safe_send::edit_caption(
            chat_id,
            msg_id,
            &txt::deleting_group_caption(),
            &[],
        ).await;
    }
    let _ = queries::update_deal_status_by_id(deal.id, "cancelled").await;
    let keep_ids: std::collections::HashSet<i64> = config.allowed_user_ids.iter().copied().collect();
    let _ = deal_manager::release_pool_group(&bot, group_id, keep_ids).await;
    tracing::info!("Сделка #{} отменена (группа {})", deal.id, group_id);
    Ok(())
}

pub async fn on_deal_cancel_reject(bot: Bot, callback: CallbackQuery) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = callback.data.unwrap_or_default();
    let parts: Vec<&str> = data.splitn(3, ':').collect();
    if parts.len() < 3 {
        let _ = bot.answer_callback_query(callback.id.clone()).await;
        return Ok(());
    }
    let group_id: i64 = parts[2].parse().unwrap_or(0);
    let deal = match queries::get_deal_by_group(group_id).await {
        Ok(Some(d)) => d,
        _ => {
            let _ = bot.answer_callback_query(callback.id.clone()).await;
            return Ok(());
        }
    };
    let config = crate::config::config();
    let user_id = callback.from.id.0 as i64;
    let guarantor_id = deal.guarantor_id.unwrap_or(deal.buyer_id);
    if user_id != guarantor_id && !config.is_allowed(user_id) {
        let _ = bot.answer_callback_query(callback.id.clone()).await;
        return Ok(());
    }
    let _ = bot.answer_callback_query(callback.id.clone()).await;
    if let Some(msg) = callback.message {
        let chat_id = msg.chat().id.0;
        let msg_id = msg.id().0 as i32;
        let _ = safe_send::edit_caption(
            chat_id,
            msg_id,
            &txt::cancel_prevented_caption(),
            &[],
        ).await;
    }
    Ok(())
}
