//! Тесты бота.
//!
//! Обычные тесты не трогают Telegram. Живой интеграционный тест помечен
//! `#[ignore]` и запускается вручную: `cargo test -- --ignored`.

use crate::handlers::inline::parse_deal_arg;
use crate::texts::messages::{plain, strip_html};

#[test]
fn парсинг_аргументов_рег() {
    assert_eq!(parse_deal_arg("1 тон").as_deref(), Some("1 тон"));
    assert_eq!(parse_deal_arg("10 ton").as_deref(), Some("10 ton"));
    assert_eq!(parse_deal_arg("1,5 usdt").as_deref(), Some("1.5 usdt"));
    assert_eq!(parse_deal_arg("  500 ₽  ").as_deref(), Some("500 ₽"));
    assert_eq!(parse_deal_arg("тон"), None);
    assert_eq!(parse_deal_arg(""), None);
    assert_eq!(parse_deal_arg("10"), None);
}

#[test]
fn разбор_команды_не_путает_рег_и_ресет() {
    use crate::handlers::command_token;

    assert_eq!(command_token("/рег 1 тон"), "/рег");
    assert_eq!(command_token("/reg 1 тон"), "/reg");
    assert_eq!(command_token("/reg@AlyaskaGarantBot 1 тон"), "/reg");
    assert_eq!(command_token("/reset"), "/reset");
    assert_eq!(command_token("/RESET"), "/reset");
    assert_eq!(command_token("/reset@bot"), "/reset");
    assert_eq!(command_token("привет"), "");
    assert_eq!(command_token(""), "");
}

#[test]
fn фолбэк_без_unicode_эмодзи() {
    let text = "<tg-emoji emoji-id=\"6037249452824072506\">🔐</tg-emoji> Сделка";
    let out = plain(text);
    assert!(!out.contains("🔐"), "unicode-эмодзи не должно попадать в текст: {}", out);
    assert!(out.contains("Сделка"));
    assert!(!strip_html("<b>жирный</b>").contains('<'));
}

/// Живой тест: создание сделки в пуловой группе, отправка карточки с
/// цветной кнопкой и освобождение группы обратно в пул.
#[tokio::test]
#[ignore]
async fn живое_создание_сделки_и_отправка_карточки() {
    use teloxide::prelude::*;

    let cfg = crate::config::config();
    let bot = teloxide::Bot::new(cfg.bot_token.clone());

    let deal_display = parse_deal_arg("1 тон").expect("разбор аргумента");
    let creator_id = 8610581845i64;

    let (deal_id, entry, link) =
        match crate::services::deal_manager::start_deal_in_pool(&bot, &deal_display, creator_id)
            .await
        {
            Ok(v) => v,
            Err(crate::services::deal_manager::StartDealError::NoFreeGroup) => {
                println!("Нет свободных сделочных — тест пропущен");
                return;
            }
            Err(e) => panic!("сделка должна создаться: {}", e),
        };

    println!("Сделка #{} создана, группа {} ссылка {}", deal_id, entry.group_id, link);
    assert!(link.starts_with("https://t.me/"));

    let caption = crate::texts::messages::deal_ready_caption(&deal_display, "tester");
    let rows = crate::keyboards::inline_kb::deal_go_keyboard(&link);
    let message_id = crate::services::safe_send::send_message_cascade(
        &cfg.bot_token,
        entry.group_id,
        &caption,
        &rows,
    )
    .await
    .expect("карточка сделки должна отправиться");
    println!("Карточка отправлена, message_id={}", message_id);

    // Группа должна быть переименована ровно один раз.
    let chat = bot.get_chat(teloxide::types::ChatId(entry.group_id)).await.expect("чат");
    assert_eq!(chat.title(), Some(&format!("Сделочная ({})", deal_display)[..]));

    crate::services::deal_manager::release_pool_group(&bot, entry.group_id, Default::default())
        .await
        .expect("группа должна освободиться");

    let chat = bot.get_chat(teloxide::types::ChatId(entry.group_id)).await.expect("чат");
    assert_eq!(chat.title(), Some(&format!("Сделочная №{}", entry.number)[..]));

    // Убираем тестовое сообщение из группы.
    let _ = bot
        .delete_message(
            teloxide::types::ChatId(entry.group_id),
            teloxide::types::MessageId(message_id as i32),
        )
        .await;
}

/// Живой тест inline-разметки: Telegram проверяет `reply_markup` результата
/// ДО проверки id запроса. Поэтому «query ID is invalid» означает, что
/// кнопки, цвета и кастом-эмодзи разобраны корректно.
#[tokio::test]
#[ignore]
async fn живая_проверка_inline_разметки_reset_all_и_pull_groups() {
    let cfg = crate::config::config();
    let client = reqwest::Client::new();

    let cases = [
        ("reset all", crate::handlers::inline::reset_all_results(7544069555, 1)),
        (
            "pull groups",
            crate::handlers::inline::pull_groups_results(
                vec!["• Сделочная (10 ton) — <a href=\"https://t.me/+abc\">ссылка</a>".to_string()],
                vec!["• Сделочная №2".to_string()],
            ),
        ),
    ];

    for (label, results) in cases {
        let payload = serde_json::json!({
            "inline_query_id": "fake-inline-query-id",
            "results": results,
            "cache_time": 0,
            "is_personal": true,
        });
        let resp: serde_json::Value = client
            .post(format!("https://api.telegram.org/bot{}/answerInlineQuery", cfg.bot_token))
            .json(&payload)
            .send()
            .await
            .expect("ответ API")
            .json()
            .await
            .expect("json API");

        let description = resp
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        println!("{}: {}", label, description);

        assert!(
            description.contains("query ID is invalid"),
            "{}: разметка не прошла проверку Telegram: {}",
            label,
            description
        );
    }
}

/// Живой тест формата сообщения от имени бизнес-аккаунта.
///
/// С неизвестным `business_connection_id` Telegram отвечает «business
/// connection not found» — значит сам payload (имя параметра, текст,
/// `reply_parameters`) разобран верно и дошёл до проверки подключения.
#[tokio::test]
#[ignore]
async fn живая_отправка_от_имени_бизнес_аккаунта() {
    let cfg = crate::config::config();

    let err = crate::services::safe_send::send_business_message(
        &cfg.bot_token,
        7544069555,
        "fake-business-connection-id",
        "<tg-emoji emoji-id=\"6037249452824072506\">🔐</tg-emoji> Проверка",
        Some(1),
        &[],
    )
    .await
    .expect_err("с неизвестным подключением отправка обязана провалиться");

    println!("ответ Telegram: {}", err);
    assert!(
        err.to_lowercase().contains("business connection"),
        "Telegram должен ругаться именно на подключение, а не на формат: {}",
        err
    );
}

/// Живой тест карточки сделки в чате бизнес-аккаунта.
///
/// Telegram принимает `reply_markup` вместе с `business_connection_id` — значит
/// синяя кнопка со ссылкой на группу рисуется в чужом личном чате. Тест
/// отправляет карточку тем же payload, что и бот, проверяет разметку в ответе
/// и удаляет тестовое сообщение.
#[tokio::test]
#[ignore]
async fn живая_карточка_с_синей_кнопкой_в_бизнес_чате() {
    use sqlx::Row;
    use teloxide::requests::Requester;

    let cfg = crate::config::config();
    let pool = crate::database::connection::get_pool().await.expect("БД");

    let key: String = sqlx::query("SELECT key FROM bot_state WHERE key LIKE 'biz:%' LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("бизнес-подключение должно быть сохранено")
        .try_get("key")
        .expect("ключ");
    let conn_id = key.trim_start_matches("biz:").to_string();

    // Пока пользователь не включил бота в Telegram Business, отправлять нечего.
    let bot = teloxide::Bot::new(cfg.bot_token.clone());
    match bot
        .get_business_connection(teloxide::types::BusinessConnectionId(conn_id.clone()))
        .await
    {
        Ok(conn) if conn.is_enabled => {}
        Ok(conn) => {
            println!(
                "Бизнес-подключение {} выключено — тест пропущен",
                conn.id.0
            );
            return;
        }
        Err(e) => {
            println!("Бизнес-подключение недоступно ({}), тест пропущен", e);
            return;
        }
    }

    let chat_id: i64 = 1251776180;
    let link = "https://t.me/+test";
    let rows = crate::keyboards::inline_kb::deal_go_keyboard(link);
    let text = crate::texts::messages::business_deal_ready_text("тест кнопки", "@tester", link);

    let mut builder = reqwest::Client::builder();
    if let Some(proxy) = cfg.proxy_url.as_deref() {
        if let Ok(p) = reqwest::Proxy::all(proxy) {
            builder = builder.proxy(p);
        }
    }
    let client = builder.build().expect("клиент");

    let payload = serde_json::json!({
        "chat_id": chat_id,
        "business_connection_id": conn_id,
        "text": text,
        "parse_mode": "HTML",
        "reply_markup": crate::services::safe_send::markup_json(&rows),
    });

    let resp: serde_json::Value = client
        .post(format!("https://api.telegram.org/bot{}/sendMessage", cfg.bot_token))
        .json(&payload)
        .send()
        .await
        .expect("ответ API")
        .json()
        .await
        .expect("json API");

    assert!(resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false), "{}", resp);

    let button = &resp["result"]["reply_markup"]["inline_keyboard"][0][0];
    println!("кнопка: {}", button);
    assert_eq!(button["style"].as_str(), Some("primary"), "кнопка должна быть синей");
    assert_eq!(
        button["icon_custom_emoji_id"].as_str(),
        Some(crate::texts::messages::EMOJI_LINK_BTN),
        "иконка кнопки не сохранилась"
    );
    assert_eq!(button["url"].as_str(), Some(link), "ссылка кнопки не сохранилась");

    let message_id = resp["result"]["message_id"].as_i64().expect("message_id");

    let deleted: serde_json::Value = client
        .post(format!(
            "https://api.telegram.org/bot{}/deleteBusinessMessages",
            cfg.bot_token
        ))
        .json(&serde_json::json!({
            "business_connection_id": conn_id,
            "message_ids": [message_id],
        }))
        .send()
        .await
        .expect("ответ API")
        .json()
        .await
        .expect("json API");

    assert!(deleted.get("ok").and_then(|v| v.as_bool()).unwrap_or(false), "{}", deleted);
    println!("тестовое сообщение {} удалено", message_id);
}

/// Живой тест отзыва пригласительной ссылки.
///
/// После сброса сделки ссылка, по которой участники заходили в группу,
/// должна перестать работать. Основную ссылку сделки тест не трогает:
/// создаёт дополнительную и отзывает именно её.
#[tokio::test]
#[ignore]
async fn живое_отзывание_пригласительной_ссылки() {
    use teloxide::prelude::*;

    let cfg = crate::config::config();
    let bot = teloxide::Bot::new(cfg.bot_token.clone());
    let group_id: i64 = -1004337890159;

    let link = bot
        .create_chat_invite_link(teloxide::types::ChatId(group_id))
        .await
        .expect("дополнительная ссылка должна создаться")
        .invite_link;
    println!("создана ссылка {}", link);
    assert!(link.starts_with("https://t.me/"));

    bot.revoke_chat_invite_link(teloxide::types::ChatId(group_id), &link)
        .await
        .expect("ссылка должна отзываться");

    let again = bot
        .revoke_chat_invite_link(teloxide::types::ChatId(group_id), &link)
        .await
        .expect("повторный отзыв возвращает состояние ссылки");
    assert!(
        again.is_revoked,
        "отозванная ссылка должна оставаться отозванной: {}",
        again.invite_link
    );
    println!("ссылка {} отозвана, is_revoked={}", again.invite_link, again.is_revoked);
}

/// Живой тест разбора бизнес-сообщения.
///
/// Подделываем `business_message` с несуществующим подключением: бот обязан
/// спокойно выйти, не создав сделку и не заняв сделочную.
#[tokio::test]
#[ignore]
async fn живое_бизнес_сообщение_без_подключения_не_создаёт_сделку() {
    use teloxide::prelude::*;

    let cfg = crate::config::config();
    let bot = teloxide::Bot::new(cfg.bot_token.clone());

    let before: Vec<i64> = crate::database::queries::get_all_pool_entries()
        .await
        .expect("пул")
        .into_iter()
        .filter(|p| p.status == "occupied")
        .map(|p| p.group_id)
        .collect();

    let raw = serde_json::json!({
        "message_id": 9_100_001,
        "date": 1_700_000_000,
        "chat": { "id": 7544069555i64, "type": "private", "first_name": "Тест" },
        "from": { "id": 7544069555i64, "is_bot": false, "first_name": "Тест", "username": "tester" },
        "business_connection_id": "fake-business-connection-id",
        "text": "/рег 1 тон"
    });
    let msg: teloxide::types::Message =
        serde_json::from_value(raw).expect("бизнес-сообщение должно разбираться");

    crate::handlers::business::on_business_message(bot, msg, false)
        .await
        .expect("обработчик не должен падать");

    let after: Vec<i64> = crate::database::queries::get_all_pool_entries()
        .await
        .expect("пул")
        .into_iter()
        .filter(|p| p.status == "occupied")
        .map(|p| p.group_id)
        .collect();

    assert_eq!(
        before, after,
        "без валидного бизнес-подключения сделка создаваться не должна"
    );
}

/// Живой тест команды «/рег 1 тон» в том виде, в каком её шлёт пользователь:
/// подделываем входящее сообщение Telegram и прогоняем через весь путь
/// `on_message` → `cmd_reg` → создание сделки → ответ в чат.
#[tokio::test]
#[ignore]
async fn живая_команда_рег_отвечает_в_чат() {
    use teloxide::prelude::*;

    let cfg = crate::config::config();
    let bot = teloxide::Bot::new(cfg.bot_token.clone());
    let user_id: i64 = 7544069555;

    let has_free = crate::database::queries::get_all_pool_entries()
        .await
        .expect("пул")
        .iter()
        .any(|p| p.status == "free");
    if !has_free {
        println!("Нет свободных сделочных — тест пропущен");
        return;
    }

    let before: Vec<i64> = crate::database::queries::get_all_pool_entries()
        .await
        .expect("пул")
        .into_iter()
        .filter(|p| p.status == "occupied")
        .map(|p| p.group_id)
        .collect();

    // Пробное сообщение: по нему мы поймём, что бот ответил ровно один раз —
    // id сообщений в чате идут подряд.
    let probe = bot
        .send_message(teloxide::types::ChatId(user_id), "проверка ответа бота")
        .await
        .expect("пробное сообщение");
    let probe_id = probe.id.0;

    let raw = serde_json::json!({
        "message_id": 9_000_001,
        "date": 1_700_000_000,
        "chat": { "id": user_id, "type": "private", "first_name": "Тест" },
        "from": { "id": user_id, "is_bot": false, "first_name": "Тест", "username": "tester" },
        "text": "/рег 1 тон"
    });
    let msg: teloxide::types::Message = serde_json::from_value(raw).expect("сообщение должно разбираться");

    crate::handlers::on_message(bot.clone(), msg)
        .await
        .expect("команда не должна падать");

    let probe2 = bot
        .send_message(teloxide::types::ChatId(user_id), "проверка ответа бота 2")
        .await
        .expect("пробное сообщение");
    let probe2_id = probe2.id.0;
    println!(
        "пробные id: {} и {} — между ними ответ бота: {}",
        probe_id,
        probe2_id,
        probe2_id - probe_id
    );

    // Новая занятая сделочная — та, что не была занята до команды.
    let entries = crate::database::queries::get_all_pool_entries().await.expect("пул");
    let new_group = entries
        .into_iter()
        .find(|p| p.status == "occupied" && !before.contains(&p.group_id))
        .expect("сделочная должна быть занята");

    let deal = crate::database::queries::get_deal_by_group(new_group.group_id)
        .await
        .expect("сделка")
        .expect("сделка должна быть зарегистрирована");
    let title = deal.title.clone();
    let group_id = deal.group_id;

    let chat = bot.get_chat(teloxide::types::ChatId(group_id)).await.expect("чат");
    let group_title = chat.title().map(|t| t.to_string());

    // Возвращаем сделочную в пул и убираем за собой сообщения.
    crate::services::deal_manager::release_pool_group(&bot, group_id, Default::default())
        .await
        .expect("группа должна освободиться");
    for id in [probe_id, probe_id + 1, probe2_id] {
        let _ = bot
            .delete_message(
                teloxide::types::ChatId(user_id),
                teloxide::types::MessageId(id),
            )
            .await;
    }

    assert_eq!(title, "1 тон");
    assert_eq!(group_title.as_deref(), Some("Сделочная (1 тон)"));
    assert_eq!(
        probe2_id - probe_id,
        2,
        "бот должен был ответить ровно одним сообщением"
    );
}
