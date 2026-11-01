use teloxide::prelude::*;
use teloxide::types::{ChatId, ChatMemberStatus, ParseMode};

use crate::database::queries;
use crate::texts::messages as txt;

pub async fn cmd_add(bot: Bot, msg: Message) -> ResponseResult<()> {
    let config = crate::config::config();
    let user_id = msg.from.as_ref().map_or(0, |u| u.id.0 as i64);

    if !config.is_allowed(user_id) {
        let _ = bot.send_message(msg.chat.id, txt::access_denied_short())
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    let group_id = msg.chat.id.0;

    let bot_me = bot.get_me().await?;
    let member = match bot.get_chat_member(ChatId(group_id), teloxide::types::UserId(bot_me.id.0 as u64)).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Не удалось проверить права бота в группе {}: {}", group_id, e);
            let _ = bot.send_message(msg.chat.id, format!("<b>{} Бот не может проверить свои права в этой группе.</b>", txt::e(txt::EMOJI_ERROR, "❌")))
                .parse_mode(ParseMode::Html)
                .await;
            return Ok(());
        }
    };

    let is_admin = matches!(
        member.status(),
        ChatMemberStatus::Administrator | ChatMemberStatus::Owner
    );

    if !is_admin {
        let _ = bot.send_message(msg.chat.id, format!("<b>{} Бот должен быть администратором группы.</b>", txt::e(txt::EMOJI_ERROR, "❌")))
            .parse_mode(ParseMode::Html)
            .await;
        return Ok(());
    }

    let number = match queries::get_max_pool_number().await {
        Ok(n) => n + 1,
        Err(e) => {
            tracing::warn!("Не удалось получить максимальный номер пула: {}", e);
            1
        }
    };

    let invite_link = match bot.export_chat_invite_link(ChatId(group_id)).await {
        Ok(link) => Some(link),
        Err(e) => {
            tracing::warn!("Не удалось получить ссылку на группу {}: {}", group_id, e);
            None
        }
    };

    let _ = queries::create_pool_entry(
        group_id,
        number,
        None,
        invite_link.as_deref(),
    ).await;

    let entry = match queries::get_pool_entry_by_group(group_id).await {
        Ok(Some(e)) => e,
        _ => {
            let _ = bot.send_message(msg.chat.id, format!("<b>{} Не удалось зарегистрировать группу в пуле.</b>", txt::e(txt::EMOJI_ERROR, "❌")))
                .parse_mode(ParseMode::Html)
                .await;
            return Ok(());
        }
    };

    if !entry.avatar_set {
        match bot
            .set_chat_photo(ChatId(group_id), teloxide::types::InputFile::file("assets/Avatarka_Group.png"))
            .await
        {
            Ok(_) => {
                let _ = queries::set_pool_avatar_set(group_id, true).await;
                tracing::info!("✓ Аватарка установлена для группы {}", group_id);
            }
            Err(e) => tracing::warn!("Не удалось установить аватарку для группы {}: {}", group_id, e),
        }
    }

    let link_text = invite_link.unwrap_or_else(|| format!("https://t.me/{}", config.bot_username));

    let _ = bot.send_message(
        msg.chat.id,
        format!(
            "<b>{} Группа добавлена в пул сделочных!</b>\n\nНомер: Сделочная №{}\nСсылка: {}",
            txt::e(txt::EMOJI_DONE, "✅"),
            number, link_text
        ),
    )
    .parse_mode(ParseMode::Html)
    .await;

    tracing::info!("Группа {} добавлена в пул как Сделочная №{}", group_id, number);

    Ok(())
}
