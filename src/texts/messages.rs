use std::collections::HashMap;

use crate::config;

pub const ASSETS_DIR: &str = "assets";

    pub static BANNER_FILES: once_cell::sync::Lazy<HashMap<&'static str, &'static str>> =
        once_cell::sync::Lazy::new(|| {
            let mut m = HashMap::new();
            m.insert("group_banner", "Group.png");
            m.insert("ruch_banner", "ruch_banner.png");
            m.insert("otmena_banner", "Otmena.png");
            m
        });

    pub fn banner_path(key: &str) -> String {
        format!("{}/{}", ASSETS_DIR, BANNER_FILES.get(key).copied().unwrap_or("Group.png"))
    }

    pub const OWNER: &str = "@Valigonov";
    pub const DEVELOPER: &str = "@NyawkaCuteUwU";
    pub const RULES_URL: &str = "https://t.me/ValigonovRules/17";

    // ===== Custom emoji (IDs стикерпаков бота) =====
    pub const EMOJI_WARNING: &str = "5474252005654434538";
    pub const EMOJI_HAND: &str = "5474356111366724294";
    pub const EMOJI_SLASH: &str = "5474274223520260467";
    pub const EMOJI_PERSON: &str = "5474476473530225277";
    pub const EMOJI_SHIELD: &str = "5474192765170524523";
    pub const EMOJI_SCROLL: &str = "5474235895232111401";
    pub const EMOJI_RULES_BTN: &str = "5915892841183190548";
    #[allow(dead_code)] // резервный ID пакета, доступного для будущих иконок
    pub const EMOJI_CLOCK: &str = "5345988476716226868";
    pub const EMOJI_LOCK: &str = "6037249452824072506";
    pub const EMOJI_INFO: &str = "6028435952299413210";
    pub const EMOJI_GUARANTOR: &str = "5474194302768817087";
    pub const EMOJI_CONFIRM_BTN: &str = "5474246495211398595";
    pub const EMOJI_REJECT_BTN: &str = "5474282620181322728";
    pub const EMOJI_CREATE_BTN: &str = "5474521579276769692";
    pub const EMOJI_TRASH: &str = "6039522349517115015";
    pub const EMOJI_STOP: &str = "5474307548171508447";

    // Псевдонимы для эмодзи, которых нет в паках: повторяют ближайший по смыслу ID.
    // Если появятся отдельные иконки — замените значения здесь, весь бот подхватит.
    pub const EMOJI_DENY: &str = EMOJI_STOP; // ⛔
    pub const EMOJI_DONE: &str = EMOJI_CONFIRM_BTN; // ✅
    pub const EMOJI_ERROR: &str = EMOJI_REJECT_BTN; // ❌
    pub const EMOJI_DOC: &str = EMOJI_SCROLL; // 📖 📝 📋
    pub const EMOJI_ID: &str = EMOJI_INFO; // 🆔
    #[allow(dead_code)]
    pub const EMOJI_REFRESH: &str = EMOJI_CLOCK; // 🔄
    pub const EMOJI_RESET: &str = EMOJI_CREATE_BTN; // ♻️
    pub const EMOJI_LINK_BTN: &str = EMOJI_CREATE_BTN; // 🔗 (иконка CTA-кнопки)
    pub const EMOJI_MESSAGE: &str = EMOJI_SLASH; // 📨

    pub fn e(emoji_id: &str, fallback: &str) -> String {
        format!("<tg-emoji emoji-id=\"{}\">{}</tg-emoji>", emoji_id, fallback)
    }

    static PLAIN_EMOJI_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r#"(?s)<tg-emoji emoji-id="\d+">.*?</tg-emoji>"#).unwrap());

    /// Убирает кастом-эмодзи целиком вместе с юникодным фолбэком:
    /// в обычном тексте unicode-эмодзи показывать нельзя.
    pub fn plain(text: &str) -> String {
        PLAIN_EMOJI_RE.replace_all(text, "").to_string()
    }

    static HTML_TAG_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"<[^>]+>").unwrap());

    /// Превращает HTML-разметку в обычный текст — последний рубеж фолбэка.
    pub fn strip_html(text: &str) -> String {
        HTML_TAG_RE
            .replace_all(text, "")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
            .trim()
            .to_string()
    }

    pub fn access_denied_short() -> String {
        format!("{} Доступ закрыт. Напишите {}", e(EMOJI_DENY, "⛔"), OWNER)
    }

    pub const STANDARD_GROUP_DESCRIPTION: &str = "Сделочная предоставлена экосистемой @AlyaskaGarantBot | Владелец - @Valigonov | Разработчица @NyawkaCuteUwU";

    pub fn pool_group_title(number: i32) -> String {
        format!("Сделочная №{}", number)
    }

    pub fn occupied_group_title(deal_display: &str) -> String {
        format!("Сделочная ({})", deal_display)
    }

    pub fn ruch_confirm_caption(target_link: &str) -> String {
        format!(
            "<b>{} │Подтверждение назначения рученика на данную сделку:\n{} После подтверждения пользователь {} будет назначен ручеником на сделку.\n{}— Вы подтверждаете назначение на данную сделку?</b>",
            e(EMOJI_WARNING, "⚠️"),
            e(EMOJI_HAND, "🛡"),
            target_link,
            e(EMOJI_SLASH, "💬")
        )
    }

    pub fn ruch_success_caption(target_link: &str) -> String {
        format!(
            "<b>{} Пользователь {} назначен ручеником на сделку!\n{} В случае его скама с его стороны, вам будет предоставлен возврат средств.\n{} Важно, ознакомиться с правилами сделки - <a href=\"{}\">Ссылка...</a></b>",
            e(EMOJI_PERSON, "👤"),
            target_link,
            e(EMOJI_SHIELD, "🛡"),
            e(EMOJI_SCROLL, "📜"),
            RULES_URL
        )
    }

    pub fn ruch_rejected_caption() -> String {
        format!("<b>{} Назначение рученика отклонено.</b>", e(EMOJI_STOP, "🚫"))
    }

    pub fn deal_ready_caption(deal_display: &str, guarantor_display: &str) -> String {
        let guarantor = format!("@{}", guarantor_display.trim_start_matches('@'));
        format!(
            "<b>{} Sdelka │ Alyaska {}\n{} Сумма сделки: {}\n{} Гарант сделки: {}</b>",
            e(EMOJI_LOCK, "🔐"),
            e(EMOJI_LOCK, "🔐"),
            e(EMOJI_INFO, "ℹ️"),
            deal_display,
            e(EMOJI_GUARANTOR, "👤"),
            guarantor
        )
    }

    /// Карточка сделки для Telegram Business.
    ///
    /// Сообщения, отправленные от имени бизнес-аккаунта, не могут нести
    /// inline-кнопки, поэтому ссылка на сделку идёт прямо в тексте.
    pub fn business_deal_ready_text(
        deal_display: &str,
        guarantor_display: &str,
        link: &str,
    ) -> String {
        let guarantor = format!("@{}", guarantor_display.trim_start_matches('@'));
        format!(
            "<b>{} Sdelka │ Alyaska {}\n{} Сумма сделки: {}\n{} Гарант сделки: {}\n{} Перейти в сделку: {}</b>",
            e(EMOJI_LOCK, "🔐"),
            e(EMOJI_LOCK, "🔐"),
            e(EMOJI_INFO, "ℹ️"),
            deal_display,
            e(EMOJI_GUARANTOR, "👤"),
            guarantor,
            e(EMOJI_LINK_BTN, "🔗"),
            link
        )
    }

    pub fn inline_create_caption(deal_display: &str) -> String {
        format!(
            "<b>{} Alyaska — Сделка: {} {}\n{} Дабы создать группу для сделки, нажмите на кнопку:\nФиолетовая инлайн кнопка: {} Создать groupe: {}</b>",
            e(EMOJI_HAND, "🛡"),
            deal_display,
            e(EMOJI_HAND, "🛡"),
            e(EMOJI_INFO, "ℹ️"),
            e(EMOJI_SHIELD, "🛡"),
            deal_display
        )
    }

    pub fn cancel_prompt_caption() -> String {
        format!(
            "<b>{} │Вы уверены что хотите отменить сделку?\n{} после подтверждения в группе будет проведена очистка</b>",
            e(EMOJI_WARNING, "⚠️"),
            e(EMOJI_TRASH, "🗑")
        )
    }

    pub fn cancel_prevented_caption() -> String {
        format!(
            "<b>{} Отмена сделки предотвращена успешно!</b>",
            e(EMOJI_STOP, "🚫")
        )
    }

    pub fn deleting_group_caption() -> String {
        format!(
            "<b>{} Очищаю группу сделки, ожидайте...</b>",
            e(EMOJI_TRASH, "🗑")
        )
    }

    pub fn reset_done_text(number: Option<i32>) -> String {
        let num_part = match number {
            Some(n) => format!("Сделочная №{}", n),
            None => "сделочная".to_string(),
        };
        format!("<b>{} Сделка сброшена — {} снова свободна.</b>", e(EMOJI_RESET, "♻️"), num_part)
    }

    pub fn reset_denied_text() -> String {
        format!("<b>{} Сбросить сделку может только гарант этой сделки.</b>", e(EMOJI_DENY, "⛔"))
    }

    pub fn no_free_pool_text() -> String {
        format!(
            "<b>{} Сейчас нет свободных сделочных.\nПул пополняется автоматически — попробуйте через несколько минут.</b>",
            e(EMOJI_ERROR, "❌")
        )
    }

    pub const MEMBER_JOINED: &str = "<b>{name} присоединился к сделке</b>";

    // ===== Команды-триггеры =====

    pub fn trigger_add_start() -> String {
        format!("<b>{} Введите сообщение-триггер:</b>\n\n<b>Это фраза, по которой бот будет присылать заготовленное сообщение. Можно 2 и более слов — главное, чтобы совпадало 1 в 1 (регистр букв не важен).\n\nПример: Аляска крипто</b>", e(EMOJI_DOC, "📝"))
    }

    pub fn trigger_ask_response() -> String {
        format!("<b>{} Отправьте сообщение которое будет присылаться при сообщении-триггере:</b>", e(EMOJI_MESSAGE, "📨"))
    }

    pub fn trigger_saved(trigger: &str) -> String {
        format!("<b>{} Команда сохранена! Триггер:</b> <code>{}</code>", e(EMOJI_DONE, "✅"), trigger)
    }

    pub fn trigger_limit() -> String {
        format!("<b>{} Достигнут лимит в 50 команд. Удалите лишние через -команда.</b>", e(EMOJI_WARNING, "⚠️"))
    }

    pub fn trigger_exists() -> String {
        format!("<b>{} Такой триггер уже существует.</b>", e(EMOJI_WARNING, "⚠️"))
    }

    pub fn trigger_too_long() -> String {
        format!("<b>{} Триггер слишком длинный (максимум 64 символа). Попробуйте короче:</b>", e(EMOJI_WARNING, "⚠️"))
    }

    pub fn trigger_delete_prompt(list: &str) -> String {
        format!("<b>{} Отправьте триггер команды, которую нужно удалить:</b>\n{}", e(EMOJI_TRASH, "🗑"), list)
    }

    pub fn trigger_deleted(trigger: &str) -> String {
        format!("<b>{} Команда удалена! Триггер:</b> <code>{}</code>", e(EMOJI_DONE, "✅"), trigger)
    }

    pub fn trigger_not_found() -> String {
        format!("<b>{} Команда с таким триггером не найдена.</b>", e(EMOJI_WARNING, "⚠️"))
    }

    pub fn trigger_empty() -> String {
        format!("<b>{} У вас пока нет команд. Создайте первую через +команда</b>", e(EMOJI_INFO, "ℹ️"))
    }

    pub fn triggers_list_text(commands: &[crate::database::models::CustomCommand]) -> String {
        if commands.is_empty() {
            return trigger_empty();
        }
        let mut lines = vec![
            format!("<b>{} Ваши команды-триггеры ({}/50):</b>", e(EMOJI_DOC, "📋"), commands.len()),
            "".to_string(),
        ];
        for cmd in commands {
            lines.push(format!("• <code>{}</code>", cmd.trigger_text));
        }
        lines.push("".to_string());
        lines.push("<b>Удалить: -команда {триггер} (или просто -команда)</b>".to_string());
        lines.join("\n")
    }

    // ===== Sms =====

    pub fn sms_add_start() -> String {
        format!("<b>{} Введите количество людей (число) в сделке, при котором будет отправлено сообщение:</b>\n\n<b>Пример: 3</b>", e(EMOJI_DOC, "📝"))
    }

    pub fn sms_ask_response() -> String {
        format!("<b>{} Отправьте сообщение которое будет присылаться при достижении этого количества людей:</b>", e(EMOJI_MESSAGE, "📨"))
    }

    pub fn sms_saved(count: i32) -> String {
        format!("<b>{} Sms сохранено! Отправляется при <code>{}</code> людях в сделке</b>", e(EMOJI_DONE, "✅"), count)
    }

    pub fn sms_limit() -> String {
        format!("<b>{} Достигнут лимит в 50 sms. Удалите лишние через -sms.</b>", e(EMOJI_WARNING, "⚠️"))
    }

    pub fn sms_exists() -> String {
        format!("<b>{} Sms с таким количеством людей уже существует.</b>", e(EMOJI_WARNING, "⚠️"))
    }

    pub fn sms_bad_count() -> String {
        format!("<b>{} Нужно целое число больше 0.\n\nПопробуйте ещё раз:</b>", e(EMOJI_WARNING, "⚠️"))
    }

    pub fn sms_delete_prompt(list: &str) -> String {
        format!("<b>{} Отправьте число людей, при котором нужно удалить sms:</b>\n{}", e(EMOJI_TRASH, "🗑"), list)
    }

    pub fn sms_deleted(count: i32) -> String {
        format!("<b>{} Sms удалено! Было при <code>{}</code> людях</b>", e(EMOJI_DONE, "✅"), count)
    }

    pub fn sms_not_found() -> String {
        format!("<b>{} Sms с таким количеством людей не найдено.</b>", e(EMOJI_WARNING, "⚠️"))
    }

    pub fn sms_empty() -> String {
        format!("<b>{} У вас пока нет sms. Создайте первое через +sms</b>", e(EMOJI_INFO, "ℹ️"))
    }

    pub fn sms_list_text(entries: &[crate::database::models::CustomSms]) -> String {
        if entries.is_empty() {
            return sms_empty();
        }
        let mut lines = vec![format!("<b>{} Ваши sms ({}/50):</b>", e(EMOJI_DOC, "📋"), entries.len()), "".to_string()];
        for s in entries {
            lines.push(format!("• при <code>{}</code> людях", s.people_count));
        }
        lines.push("".to_string());
        lines.push("<b>Удалить: -sms {число} (или просто -sms)</b>".to_string());
        lines.join("\n")
    }

    // ===== /start и /help =====

    pub fn start_text() -> String {
        let bot_username = &config::config().bot_username;
        format!(
            "{} <b>Alyaska Garant</b> — бот-гарант для безопасных сделок\n\nЯ помогаю проводить сделки безопасно: держу наготове пул сделочных (групп) и выдаю их по команде.\n\n<b>Как использовать:</b>\nВведите в любом чате:\n<code>@{bot_username} 10 ton</code>\n<code>@{bot_username} 1500 руб</code>\n<code>@{bot_username} 1 аккаунт</code>\n\nИли создайте сделку командой:\n<code>/рег 10 ton</code>\n\nСделочная подготовится автоматически — останется только перейти в неё.\n\n━━━━━━━━━━━━━━━━━━━━\n<i>alyaska garant · {OWNER} · {DEVELOPER}</i>",
            e(EMOJI_SHIELD, "🛡"),
            bot_username = bot_username,
            OWNER = OWNER,
            DEVELOPER = DEVELOPER
        )
    }

    pub fn help_text() -> String {
        let bot_username = &config::config().bot_username;
        format!(
            "{} <b>Alyaska Garant — Справка</b>\n\n<b>Inline-режим:</b>\n<code>@{bot_username} [сумма] [валюта/товар]</code> — создать сделку\n<code>@{bot_username} pull groups</code> — список сделочных (админ)\n<code>@{bot_username} reset all</code> — сбросить все активные сделки (админ)\n\n<b>Примеры:</b>\n• <code>@{bot_username} 10 ton</code> — сделка на 10 TON\n• <code>@{bot_username} 500 руб</code> — сделка на 500 ₽\n• <code>@{bot_username} 1 аккаунт</code> — сделка на 1 аккаунт\n\n<b>Команды в чате с ботом:</b>\n• <code>/рег [название сделки]</code> — создать сделку\n• <code>/add</code> — добавить текущую группу в пул сделочных (в группе)\n• <code>+команда</code> — добавить команду-триггер\n• <code>-команда</code> — удалить команду-триггер\n• <code>/команды</code> — список ваших триггеров\n\n<b>/рег в переписке с человеком:</b> подключите бота в Telegram → Настройки → Telegram Business → Чат-боты. Тогда <code>/рег 10 ton</code> в личном чате создаёт сделку, а бот отвечает в этом чате от вашего имени.\n\n<b>Команды в группе сделки:</b>\n• <code>/ресет</code> — завершить сделку, очистить сделочную\n• <code>/отмена</code> — отменить сделку и удалить группу\n• <code>руч</code> / <code>руч @username</code> — назначить рученика\n\n━━━━━━━━━━━━━━━━━━━━\n<i>alyaska garant · {OWNER} · {DEVELOPER}</i>",
            e(EMOJI_DOC, "📖"),
            bot_username = bot_username,
            OWNER = OWNER,
            DEVELOPER = DEVELOPER
        )
    }

    // ===== Inline-режим =====

    pub fn inline_title(deal_display: &str) -> String {
        // В заголовках inline-результатов Telegram не поддерживает кастомные эмодзи,
        // поэтому без иконки.
        format!("Создать сделку: {}", deal_display)
    }

    pub const INLINE_DESCRIPTION: &str = "Нажмите, чтобы выдать сделочную Alyaska Garant";

    /// Экранирует текст для вставки в HTML-разметку сообщений.
    pub fn esc_html(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    // ===== /рег =====

    pub fn reg_usage_text() -> String {
        format!(
            "{} <b>Создание сделки:</b>\n<code>/рег [сумма] [валюта/товар]</code>\n\nПример: <code>/рег 10 ton</code>",
            e(EMOJI_INFO, "ℹ️")
        )
    }

    // ===== Inline: reset all =====

    pub fn reset_all_prompt_text(count: usize) -> String {
        format!(
            "<b>{} Вы уверены, что хотите сбросить ВСЕ активные сделки?</b>\n\n{} Будет сброшено сделочных: <b>{}</b>\n{} Участники будут удалены из групп, группы очистятся и вернутся в пул.\n\n{} Нажмите кнопку под этим сообщением для подтверждения.",
            e(EMOJI_WARNING, "⚠️"),
            e(EMOJI_RESET, "♻️"),
            count,
            e(EMOJI_TRASH, "🗑"),
            e(EMOJI_INFO, "ℹ️")
        )
    }

    pub fn reset_all_empty_text() -> String {
        format!("<b>{} Нет активных сделок — сбрасывать нечего.</b>", e(EMOJI_INFO, "ℹ️"))
    }

    pub fn reset_all_done_text(reset: usize, failed: usize) -> String {
        let mut s = format!("<b>{} Сброс завершён: сделочных очищено — {}.</b>", e(EMOJI_RESET, "♻️"), reset);
        if failed > 0 {
            s.push_str(&format!("\n{} Не удалось сбросить: {}.", e(EMOJI_ERROR, "❌"), failed));
        }
        s
    }

    // ===== Inline: pull groups =====

    pub fn pull_groups_text(active_lines: &[String], free_lines: &[String]) -> String {
        let total = active_lines.len() + free_lines.len();
        let mut out = vec![format!("<b>{} Сделочные Alyaska</b>", e(EMOJI_DOC, "📋"))];
        out.push(String::new());
        out.push(format!(
            "<b>{} Актуальные сделочные — идёт сделка ({}):</b>",
            e(EMOJI_LOCK, "🔐"),
            active_lines.len()
        ));
        if active_lines.is_empty() {
            out.push("• нет активных сделок".to_string());
        } else {
            out.extend(active_lines.iter().cloned());
        }
        out.push(String::new());
        out.push(format!(
            "<b>{} Свободные сделочные — готовы к сделкам ({}):</b>",
            e(EMOJI_CREATE_BTN, "🆓"),
            free_lines.len()
        ));
        if free_lines.is_empty() {
            out.push("• нет свободных сделочных".to_string());
        } else {
            out.extend(free_lines.iter().cloned());
        }
        out.push(String::new());
        out.push(format!(
            "<b>{} Всего сделочных: {} (активных: {}, свободных: {})</b>",
            e(EMOJI_INFO, "ℹ️"),
            total,
            active_lines.len(),
            free_lines.len()
        ));
        out.join("\n")
    }
