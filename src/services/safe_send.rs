use once_cell::sync::Lazy;
use teloxide::payloads::SendPhotoSetters;
use teloxide::requests::Requester;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardButtonKind, InlineKeyboardMarkup, InputFile, Message, ParseMode};
use teloxide::Bot;

use crate::texts::messages::{plain, strip_html};

/// Единый HTTP-клиент для сырых запросов к Telegram API.
/// Учитывает прокси из конфига (teloxide проксирует только свои запросы).
static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy) = crate::config::config().proxy_url.as_deref() {
        match reqwest::Proxy::all(proxy) {
            Ok(p) => {
                builder = builder.proxy(p);
            }
            Err(e) => tracing::warn!("Некорректный прокси '{}': {} — raw-запросы пойдут напрямую", proxy, e),
        }
    }
    builder.build().unwrap_or_default()
});

fn is_emoji_error_text(s: &str) -> bool {
    let s = s.to_lowercase();
    s.contains("emoji")
        || s.contains("entity")
        || s.contains("entiti")
        || s.contains("can't parse")
        || s.contains("unsupported start tag")
        || s.contains("unsupported end tag")
}

/// Проверяет, вызвана ли ошибка проблемой с custom emoji / entities.
fn is_emoji_error(err: &teloxide::RequestError) -> bool {
    is_emoji_error_text(&err.to_string())
}

/// Сырой POST к Telegram Bot API. Возвращает Err с описанием ошибки API.
async fn tg_post(token: &str, method: &str, payload: &serde_json::Value) -> Result<(), String> {
    tg_post_raw(token, method, payload).await.map(|_| ())
}

/// То же, но возвращает поле `result` ответа (например, message_id).
async fn tg_post_raw(
    token: &str,
    method: &str,
    payload: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = format!("https://api.telegram.org/bot{}/{}", token, method);
    let resp = HTTP
        .post(&url)
        .json(payload)
        .send()
        .await
        .map_err(|e| format!("{}: сетевая ошибка: {}", method, e))?;
    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("{} ({}): не удалось прочитать ответ: {}", method, status, e))?;
    if body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        Ok(body.get("result").cloned().unwrap_or(serde_json::Value::Null))
    } else {
        Err(format!(
            "{}: {}",
            method,
            body.get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("неизвестная ошибка")
        ))
    }
}

/// Отправляет фото-баннер с подписью.
///
/// Фото уходит через teloxide (multipart, plain-кнопки), затем сырым
/// `editMessageReplyMarkup` прикрепляется разметка с `icon_custom_emoji_id` —
/// teloxide это поле (Bot API 9.4) не поддерживает.
pub async fn send_banner(
    bot: Bot,
    chat_id: i64,
    photo: InputFile,
    caption: &str,
    rows: &[Vec<KbButton>],
) -> Result<Message, teloxide::RequestError> {
    let kb = teloxide_markup(rows);

    let sent = match bot
        .send_photo(teloxide::types::ChatId(chat_id), photo.clone())
        .caption(caption.to_string())
        .parse_mode(ParseMode::Html)
        .reply_markup(kb.clone())
        .await
    {
        Ok(msg) => msg,
        Err(e) if is_emoji_error(&e) => {
            tracing::warn!("Баннер не принят с custom emoji, повтор без них: {}", e);
            bot.send_photo(teloxide::types::ChatId(chat_id), photo)
                .caption(plain(caption))
                .parse_mode(ParseMode::Html)
                .reply_markup(kb)
                .await?
        }
        Err(e) => return Err(e),
    };

    attach_icon_markup(chat_id, sent.id.0, rows).await;
    Ok(sent)
}

/// Прикрепляет к уже отправленному сообщению кнопки с иконками custom emoji.
async fn attach_icon_markup(chat_id: i64, message_id: i32, rows: &[Vec<KbButton>]) {
    let token = &crate::config::config().bot_token;
    let payload = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "reply_markup": markup_json(rows),
    });
    if let Err(e) = tg_post(&token, "editMessageReplyMarkup", &payload).await {
        tracing::warn!("Не удалось установить иконки кнопок: {} (остались кнопки без иконок)", e);
    }
}

/// Общая правка сообщения с каскадом фолбэков (иконки → plain → обычный текст).
async fn tg_edit(
    token: &str,
    method: &str,
    base: &serde_json::Value,
    text_key: &str,
    text: &str,
    rows: &[Vec<KbButton>],
) -> Result<(), String> {
    let mut attempts: Vec<(&str, serde_json::Value)> = Vec::new();

    let mut p = base.clone();
    p[text_key] = serde_json::json!(text);
    p["parse_mode"] = serde_json::json!("HTML");
    p["reply_markup"] = markup_json(rows);
    attempts.push(("с кастом-эмодзи", p));

    let mut p = base.clone();
    p[text_key] = serde_json::json!(text);
    p["parse_mode"] = serde_json::json!("HTML");
    p["reply_markup"] = markup_json_plain(rows);
    attempts.push(("без иконок кнопок", p));

    let mut p = base.clone();
    p[text_key] = serde_json::json!(plain(text));
    p["parse_mode"] = serde_json::json!("HTML");
    p["reply_markup"] = markup_json_plain(rows);
    attempts.push(("без кастом-эмодзи", p));

    let mut p = base.clone();
    p[text_key] = serde_json::json!(strip_html(&plain(text)));
    p["reply_markup"] = markup_json_minimal(rows);
    attempts.push(("обычным текстом", p));

    let mut last_err = String::new();
    for (label, payload) in attempts {
        match tg_post(token, method, &payload).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::error!("{} не удался ({}): {}", method, label, e);
                last_err = e;
            }
        }
    }
    Err(last_err)
}

/// Редактирует подпись к фото в обычном чате.
pub async fn edit_caption(
    chat_id: i64,
    message_id: i32,
    caption: &str,
    rows: &[Vec<KbButton>],
) -> Result<(), String> {
    let token = &crate::config::config().bot_token;
    let base = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
    });
    tg_edit(token, "editMessageCaption", &base, "caption", caption, rows).await
}

/// Редактирует подпись inline-сообщения (результат inline-запроса).
pub async fn edit_inline_caption(
    inline_message_id: &str,
    caption: &str,
    rows: &[Vec<KbButton>],
) -> Result<(), String> {
    let token = &crate::config::config().bot_token;
    let base = serde_json::json!({ "inline_message_id": inline_message_id });
    tg_edit(token, "editMessageCaption", &base, "caption", caption, rows).await
}

/// Редактирует текст inline-сообщения (для результата без фото).
pub async fn edit_inline_text(
    inline_message_id: &str,
    text: &str,
    rows: &[Vec<KbButton>],
) -> Result<(), String> {
    let token = &crate::config::config().bot_token;
    let base = serde_json::json!({ "inline_message_id": inline_message_id });
    tg_edit(token, "editMessageText", &base, "text", text, rows).await
}

/// Возвращает InputFile для баннера по ключу.
pub fn banner_photo(key: &str, file_id: Option<&str>) -> InputFile {
    if let Some(fid) = file_id {
        return InputFile::file_id(teloxide::types::FileId(fid.to_string()));
    }
    let path = crate::texts::messages::banner_path(key);
    if std::path::Path::new(&path).exists() {
        InputFile::file(path)
    } else {
        let fallback = format!("{}/Group.png", crate::texts::messages::ASSETS_DIR);
        InputFile::file(fallback)
    }
}

/// Кнопка с кастом-эмодзи и/или ссылкой.
#[derive(Clone, Debug)]
pub struct KbButton {
    pub text: String,
    pub emoji_id: Option<String>, // icon_custom_emoji_id (Bot API 9.4)
    pub style: Option<String>,    // "primary" (синяя) / "success" (зелёная) / "danger" (красная)
    pub url: Option<String>,
    pub callback_data: Option<String>,
}

impl KbButton {
    pub fn callback(text: &str, emoji_id: Option<&str>, callback_data: &str) -> Self {
        KbButton {
            text: text.to_string(),
            emoji_id: emoji_id.map(|s| s.to_string()),
            style: None,
            url: None,
            callback_data: Some(callback_data.to_string()),
        }
    }

    pub fn url_btn(text: &str, emoji_id: Option<&str>, url: &str) -> Self {
        KbButton {
            text: text.to_string(),
            emoji_id: emoji_id.map(|s| s.to_string()),
            style: None,
            url: Some(url.to_string()),
            callback_data: None,
        }
    }

    /// Цвет кнопки: "primary" (синяя), "success" (зелёная), "danger" (красная).
    pub fn with_style(mut self, style: &str) -> Self {
        self.style = Some(style.to_string());
        self
    }

    /// JSON кнопки для сырых запросов (с icon_custom_emoji_id или без).
    fn to_json(&self, with_icon: bool) -> serde_json::Value {
        let mut b = serde_json::json!({ "text": self.text });
        if let Some(st) = self.style.as_deref() {
            b["style"] = serde_json::json!(st);
        }
        if with_icon {
            if let Some(eid) = self.emoji_id.as_deref() {
                b["icon_custom_emoji_id"] = serde_json::json!(eid);
            }
        }
        if let Some(u) = self.url.as_deref() {
            b["url"] = serde_json::json!(u);
        } else if let Some(cb) = self.callback_data.as_deref() {
            b["callback_data"] = serde_json::json!(cb);
        }
        b
    }

    /// teloxide-кнопка без иконки (используется как первичная разметка).
    fn to_teloxide(&self) -> InlineKeyboardButton {
        if let Some(u) = self.url.as_deref() {
            InlineKeyboardButton::new(
                self.text.clone(),
                InlineKeyboardButtonKind::Url(
                    url::Url::parse(u).unwrap_or_else(|_| url::Url::parse("https://t.me").unwrap()),
                ),
            )
        } else {
            InlineKeyboardButton::new(
                self.text.clone(),
                InlineKeyboardButtonKind::CallbackData(self.callback_data.clone().unwrap_or_default()),
            )
        }
    }
}

pub fn btn_url(text: &str, emoji_id: Option<&str>, url: &str) -> KbButton {
    KbButton::url_btn(text, emoji_id, url)
}

/// InlineKeyboardMarkup в JSON (с иконками).
pub fn markup_json(rows: &[Vec<KbButton>]) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": rows
            .iter()
            .map(|row| row.iter().map(|b| b.to_json(true)).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    })
}

/// InlineKeyboardMarkup в JSON без иконок (фолбэк).
pub fn markup_json_plain(rows: &[Vec<KbButton>]) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": rows
            .iter()
            .map(|row| row.iter().map(|b| b.to_json(false)).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    })
}

/// InlineKeyboardMarkup в JSON без иконок и без цветов (последний фолбэк).
pub fn markup_json_minimal(rows: &[Vec<KbButton>]) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|b| {
                        let mut j = b.to_json(false);
                        if let Some(obj) = j.as_object_mut() {
                            obj.remove("style");
                        }
                        j
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    })
}

fn teloxide_markup(rows: &[Vec<KbButton>]) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(
        rows.iter()
            .map(|row| row.iter().map(|b| b.to_teloxide()).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
    )
}

/// Отправляет сообщение с кастом-эмодзи кнопками через сырой HTTP-запрос к Telegram API.
///
/// Каскад фолбэков: сначала полный payload (HTML + custom emoji + иконки/цвета кнопок),
/// затем без иконок кнопок, затем без custom emoji в тексте и в конце обычный текст.
/// Так бот никогда не остаётся без ответа.
pub async fn send_message_cascade(
    bot_token: &str,
    chat_id: i64,
    text: &str,
    rows: &[Vec<KbButton>],
) -> Result<i64, String> {
    let attempts: Vec<(&str, serde_json::Value)> = vec![
        (
            "с кастом-эмодзи и цветными кнопками",
            serde_json::json!({
                "chat_id": chat_id,
                "text": text,
                "parse_mode": "HTML",
                "reply_markup": markup_json(rows),
            }),
        ),
        (
            "без иконок кнопок",
            serde_json::json!({
                "chat_id": chat_id,
                "text": text,
                "parse_mode": "HTML",
                "reply_markup": markup_json_plain(rows),
            }),
        ),
        (
            "без кастом-эмодзи в тексте",
            serde_json::json!({
                "chat_id": chat_id,
                "text": plain(text),
                "parse_mode": "HTML",
                "reply_markup": markup_json_plain(rows),
            }),
        ),
        (
            "обычным текстом",
            serde_json::json!({
                "chat_id": chat_id,
                "text": strip_html(&plain(text)),
                "reply_markup": markup_json_minimal(rows),
            }),
        ),
    ];

    let mut last_err = String::new();
    for (label, payload) in attempts {
        match tg_post_raw(bot_token, "sendMessage", &payload).await {
            Ok(result) => {
                return Ok(result.get("message_id").and_then(|v| v.as_i64()).unwrap_or(0))
            }
            Err(e) => {
                tracing::error!("sendMessage в чат {} не удался ({}): {}", chat_id, label, e);
                last_err = e;
            }
        }
    }
    Err(last_err)
}

/// Отправляет сообщение от имени бизнес-аккаунта (Telegram Business).
///
/// `business_connection_id` отправляет сообщение от лица владельца бизнес-
/// аккаунта. `reply_markup` Telegram принимает и в этом случае — кнопка со
/// ссылкой на сделку работает.
pub async fn send_business_message(
    bot_token: &str,
    chat_id: i64,
    business_connection_id: &str,
    text: &str,
    reply_to_message_id: Option<i32>,
    rows: &[Vec<KbButton>],
) -> Result<i64, String> {
    let mut attempts: Vec<(&str, serde_json::Value)> = Vec::new();

    if let Some(reply_id) = reply_to_message_id {
        attempts.push((
            "с кастом-эмодзи, кнопками и ответом на команду",
            serde_json::json!({
                "chat_id": chat_id,
                "business_connection_id": business_connection_id,
                "text": text,
                "parse_mode": "HTML",
                "reply_parameters": { "message_id": reply_id },
                "reply_markup": markup_json(rows),
            }),
        ));
    }
    attempts.push((
        "с кастом-эмодзи и кнопками",
        serde_json::json!({
            "chat_id": chat_id,
            "business_connection_id": business_connection_id,
            "text": text,
            "parse_mode": "HTML",
            "reply_markup": markup_json(rows),
        }),
    ));
    attempts.push((
        "без иконок кнопок",
        serde_json::json!({
            "chat_id": chat_id,
            "business_connection_id": business_connection_id,
            "text": text,
            "parse_mode": "HTML",
            "reply_markup": markup_json_plain(rows),
        }),
    ));
    attempts.push((
        "без кастом-эмодзи в тексте",
        serde_json::json!({
            "chat_id": chat_id,
            "business_connection_id": business_connection_id,
            "text": plain(text),
            "parse_mode": "HTML",
            "reply_markup": markup_json_plain(rows),
        }),
    ));
    attempts.push((
        "обычным текстом",
        serde_json::json!({
            "chat_id": chat_id,
            "business_connection_id": business_connection_id,
            "text": strip_html(&plain(text)),
            "reply_markup": markup_json_minimal(rows),
        }),
    ));

    let mut last_err = String::new();
    for (label, payload) in attempts {
        match tg_post_raw(bot_token, "sendMessage", &payload).await {
            Ok(result) => {
                return Ok(result.get("message_id").and_then(|v| v.as_i64()).unwrap_or(0))
            }
            Err(e) => {
                tracing::error!(
                    "sendMessage от имени бизнес-аккаунта в чат {} не удался ({}): {}",
                    chat_id, label, e
                );
                last_err = e;
            }
        }
    }
    Err(last_err)
}

/// Регистрирует команды бота. Без этого бот с включённым privacy mode
/// не получает команды в группах.
///
/// Bot API принимает только `[a-z0-9_]{1,32}` — кириллические команды
/// (например «рег») регистрировать нельзя, они работают в личных чатах.
pub async fn set_my_commands(bot_token: &str, commands: &[(&str, &str)]) -> Result<(), String> {
    let mut list: Vec<serde_json::Value> = Vec::new();
    for (cmd, desc) in commands {
        let valid = !cmd.is_empty()
            && cmd.len() <= 32
            && cmd
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !valid {
            tracing::warn!("Команда '{}' не регистрируется: Bot API принимает только a-z, 0-9 и _", cmd);
            continue;
        }
        list.push(serde_json::json!({ "command": cmd, "description": desc }));
    }
    let payload = serde_json::json!({ "commands": list });
    tg_post(bot_token, "setMyCommands", &payload).await
}

/// Убирает иконки кнопок из результатов inline-запроса (фолбэк для answerInlineQuery).
fn strip_result_icons(results: &[serde_json::Value]) -> Vec<serde_json::Value> {
    results
        .iter()
        .map(|r| {
            let mut r = r.clone();
            if let Some(kb) = r
                .get_mut("reply_markup")
                .and_then(|v| v.get_mut("inline_keyboard"))
                .and_then(|v| v.as_array_mut())
            {
                for row in kb.iter_mut() {
                    if let Some(row) = row.as_array_mut() {
                        for b in row.iter_mut() {
                            if let Some(obj) = b.as_object_mut() {
                                obj.remove("icon_custom_emoji_id");
                            }
                        }
                    }
                }
            }
            r
        })
        .collect()
}

/// Отправляет inline-запрос с кастом-эмодзи кнопками через сырой HTTP-запрос.
pub async fn answer_inline_query_with_custom_buttons(
    bot_token: &str,
    inline_query_id: &str,
    results: &[serde_json::Value],
) -> Result<(), String> {
    let payload = serde_json::json!({
        "inline_query_id": inline_query_id,
        "results": results,
        "cache_time": 0,
        "is_personal": true,
    });

    match tg_post(bot_token, "answerInlineQuery", &payload).await {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::error!("answerInlineQuery не удался: {}", e);
            let fallback = serde_json::json!({
                "inline_query_id": inline_query_id,
                "results": strip_result_icons(results),
                "cache_time": 0,
                "is_personal": true,
            });
            match tg_post(bot_token, "answerInlineQuery", &fallback).await {
                Ok(()) => Ok(()),
                Err(e2) => {
                    tracing::error!("answerInlineQuery (без иконок) не удался: {}", e2);
                    Err(e2)
                }
            }
        }
    }
}
