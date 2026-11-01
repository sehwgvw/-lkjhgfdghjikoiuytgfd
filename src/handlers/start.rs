use teloxide::prelude::*;
use teloxide::types::ParseMode;

use crate::texts::messages;

pub async fn cmd_start(bot: Bot, msg: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    bot.send_message(msg.chat.id, messages::start_text())
        .parse_mode(ParseMode::Html)
        .await?;
    Ok(())
}

pub async fn cmd_help(bot: Bot, msg: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    bot.send_message(msg.chat.id, messages::help_text())
        .parse_mode(ParseMode::Html)
        .await?;
    Ok(())
}

pub async fn cmd_id(bot: Bot, msg: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = crate::config::config();
    let user_id = msg.from.as_ref().map_or(0, |u| u.id.0 as i64);
    if !config.is_allowed(user_id) {
        return Ok(());
    }

    let mut lines = vec![format!("{} <b>Ваш ID:</b> <code>{}</code>", messages::e(messages::EMOJI_ID, "🆔"), user_id)];

    if let Some(reply) = msg.reply_to_message() {
        if let Some(ru) = reply.from.as_ref() {
            let label = ru.username.as_deref().map_or("".to_string(), |u| format!(" (@{})", u));
            lines.push(format!(
                "{} <b>ID пользователя в reply:</b> <code>{}</code>{}",
                messages::e(messages::EMOJI_ID, "🆔"),
                ru.id.0 as i64, label
            ));
        }
    }

    lines.push("".to_string());
    lines.push(
        "<i>Добавьте нужные ID в ALLOWED_USER_IDS в .env, чтобы дать доступ.</i>".to_string(),
    );

    bot.send_message(msg.chat.id, lines.join("\n"))
        .parse_mode(ParseMode::Html)
        .await?;
    Ok(())
}
