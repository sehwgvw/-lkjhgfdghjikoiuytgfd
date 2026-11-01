use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

use sqlx::Row;
use teloxide::payloads::SetChatDescriptionSetters;
use teloxide::types::ChatId;
use teloxide::Bot;
use teloxide::requests::Requester;

use crate::database::connection::get_pool;
use crate::database::queries;
use crate::database::models::{Deal, PoolGroup};
use crate::texts::messages as txt;

static GROUP_MEMBERS: OnceLock<std::sync::RwLock<HashMap<i64, HashSet<i64>>>> = OnceLock::new();

fn group_members() -> &'static std::sync::RwLock<HashMap<i64, HashSet<i64>>> {
    GROUP_MEMBERS.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

pub fn set_bot_ids(_ub_id: i64, _b_id: i64) {
    // В новой версии без юзерботов — не требуется
}

pub fn is_bot_or_userbot(_user_id: i64) -> bool {
    false
}

pub async fn register_deal(
    group_id: i64,
    group_link: &str,
    amount: &str,
    currency: &str,
    creator_id: i64,
    title: Option<&str>,
) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let title = title.unwrap_or("");
    let deal_display = if !title.is_empty() {
        title.to_string()
    } else {
        format!("{} {}", amount, currency.to_uppercase())
    };

    if let Err(e) = queries::upsert_user(creator_id, None, None).await {
        tracing::warn!("upsert_user error: {}", e);
    }

    let _ = queries::delete_deal_by_group(group_id).await;

    let deal_id = queries::create_deal(
        group_id,
        group_link,
        &deal_display,
        amount,
        currency,
        creator_id,
    )
    .await?;

    if let Err(e) = queries::set_deal_guarantor(group_id, creator_id).await {
        tracing::warn!("set_deal_guarantor error: {}", e);
    }

    add_human_member(group_id, creator_id);

    tracing::info!(
        "Сделка #{} зарегистрирована: {} (группа {}, гарант {})",
        deal_id,
        deal_display,
        group_id,
        creator_id
    );
    Ok(deal_id)
}

pub fn add_human_member(group_id: i64, user_id: i64) {
    let mut gm = group_members().write().unwrap();
    gm.entry(group_id).or_default().insert(user_id);
}

pub fn clear_group_members(group_id: i64) {
    group_members().write().unwrap().remove(&group_id);
}

pub fn get_group_members(group_id: i64) -> Vec<i64> {
    let gm = group_members().read().unwrap();
    gm.get(&group_id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect()
}

pub async fn on_member_joined(
    group_id: i64,
    user_id: i64,
    deal: &Deal,
) -> Option<&'static str> {
    add_human_member(group_id, user_id);

    let guarantor_id = deal.guarantor_id.unwrap_or(deal.buyer_id);

    if user_id == guarantor_id {
        return Some("guarantor");
    }

    if let Some(ruchenik_id) = deal.ruchenik_id {
        if user_id == ruchenik_id {
            return Some("ruchenik");
        }
    }

    Some("member")
}

pub async fn ensure_guarantor_present(
    bot: &Bot,
    group_id: i64,
    guarantor_id: i64,
) -> bool {
    {
        let gm = group_members().read().unwrap();
        if gm.get(&group_id).map_or(false, |s| s.contains(&guarantor_id)) {
            return true;
        }
    }

    match bot.get_chat_member(ChatId(group_id), teloxide::types::UserId(guarantor_id as u64)).await {
        Ok(member) => {
            let status = format!("{:?}", member.status());
            if status.contains("Owner")
                || status.contains("Administrator")
                || status.contains("Member")
                || status.contains("Restricted")
            {
                add_human_member(group_id, guarantor_id);
                tracing::info!(
                    "✓ Гарант {} обнаружен в группе {} и добавлен в трекинг",
                    guarantor_id,
                    group_id
                );
                return true;
            }
        }
        Err(e) => {
            tracing::debug!(
                "ensure_guarantor_present({}, {}): {}",
                group_id,
                guarantor_id,
                e
            );
        }
    }
    false
}

pub async fn restore_active_deals() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = get_pool().await?;
    let rows = sqlx::query(
        "SELECT id, group_id, guarantor_id, buyer_id, seller_id, ruchenik_id FROM deals WHERE status IN ('created', 'active', 'waiting')",
    )
    .fetch_all(&pool)
    .await?;

    for row in rows {
        let gid: i64 = row.try_get("group_id").unwrap_or(0);
        if gid == 0 {
            continue;
        }
        let mut members = HashSet::new();
        for col in ["guarantor_id", "buyer_id", "seller_id", "ruchenik_id"] {
            if let Some(uid) = row.try_get::<Option<i64>, _>(col).ok().flatten() {
                if uid > 0 {
                    members.insert(uid);
                }
            }
        }
        let log_users = sqlx::query("SELECT DISTINCT user_id FROM messages_log WHERE deal_id = $1")
            .bind(row.try_get::<i32, _>("id").unwrap_or(0))
            .fetch_all(&pool)
            .await?;
        for lu in log_users {
            let uid: i64 = lu.try_get("user_id").unwrap_or(0);
            if uid > 0 {
                members.insert(uid);
            }
        }
        let members_clone = members.clone();
        group_members().write().unwrap().insert(gid, members_clone);
        tracing::info!(
            "✓ Восстановлена сделка #{} (группа {}): {} участников",
            row.try_get::<i32, _>("id").unwrap_or(0),
            gid,
            members.len()
        );
    }
    Ok(())
}

pub fn check_deal_participants(group_id: i64, deal: &Deal) -> (bool, i32, bool, i32) {
    let guarantor_id = deal.guarantor_id.unwrap_or(deal.buyer_id);
    let gm = group_members().read().unwrap();
    let raw_members = gm.get(&group_id).cloned().unwrap_or_default();
    let humans: Vec<i64> = raw_members.into_iter().filter(|uid| *uid > 0).collect();

    let guarantor_present = humans.contains(&guarantor_id);
    let other_count = humans.iter().filter(|uid| **uid != guarantor_id).count();

    let ready = guarantor_present && other_count >= 2;
    (ready, humans.len() as i32, guarantor_present, other_count as i32)
}

pub fn get_other_human_members(group_id: i64, guarantor_id: i64) -> Vec<i64> {
    let gm = group_members().read().unwrap();
    gm.get(&group_id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|uid| *uid > 0 && *uid != guarantor_id)
        .collect()
}

pub fn get_human_members_count(group_id: i64) -> i32 {
    let gm = group_members().read().unwrap();
    gm.get(&group_id)
        .map(|s| s.iter().filter(|uid| **uid > 0).count() as i32)
        .unwrap_or(0)
}

pub async fn set_deal_ruchenik_inner(deal_id: i32, ruchenik_id: i64) {
    if let Err(e) = queries::set_deal_ruchenik(deal_id, ruchenik_id).await {
        tracing::warn!("set_deal_ruchenik error: {}", e);
    }
    if let Err(e) = queries::upsert_user(ruchenik_id, None, None).await {
        tracing::warn!("upsert_user error: {}", e);
    }
    tracing::info!("Рученик {} назначен на сделку #{}", ruchenik_id, deal_id);
}

// ─── Захват и освобождение сделочных ─────────────────────────

/// Ошибка создания сделки в пуловой группе.
#[derive(Debug)]
pub enum StartDealError {
    NoFreeGroup,
    Register(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for StartDealError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartDealError::NoFreeGroup => write!(f, "Нет свободных сделочных"),
            StartDealError::Register(e) => write!(f, "{}", e),
        }
    }
}

/// Захватывает свободную сделочную: помечает занятой, переименовывает,
/// перевыпускает и сохраняет ссылку. `Ok(None)` — свободных групп нет.
pub async fn occupy_pool_group(
    bot: &Bot,
    deal_display: &str,
) -> Result<Option<(PoolGroup, String)>, Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!("Ищем свободную сделочную для сделки '{}'", deal_display);
    let Some(entry) = queries::claim_free_pool_group().await? else {
        tracing::warn!("Нет свободных сделочных для сделки '{}'", deal_display);
        return Ok(None);
    };
    tracing::info!("Найдена свободная сделочная: №{} (группа {})", entry.number, entry.group_id);

    let occupied_title = txt::occupied_group_title(deal_display);
    if let Err(e) = set_title_with_retry(bot, entry.group_id, &occupied_title).await {
        tracing::error!("Ошибка установки названия группы {}: {}", entry.group_id, e);
        release_claimed_pool_group(entry.group_id).await;
        return Err(e.into());
    }

    let invite_link = match export_link_with_retry(bot, entry.group_id).await {
        Ok(link) => {
            tracing::info!("Получена ссылка на группу: {}", link);
            match queries::set_pool_invite_link(entry.group_id, Some(&link)).await {
                Ok(_) => tracing::info!("Ссылка сохранена в БД"),
                Err(e) => tracing::error!("Ошибка сохранения ссылки в БД: {}", e),
            }
            link
        }
        Err(e) => {
            tracing::error!("Ошибка получения ссылки на группу {}: {}", entry.group_id, e);
            release_claimed_pool_group(entry.group_id).await;
            return Err(e.into());
        }
    };

    Ok(Some((entry, invite_link)))
}

/// Переименование группы с повтором: транзиентная сетевая ошибка не должна
/// срывать создание сделки.
async fn set_title_with_retry(bot: &Bot, group_id: i64, title: &str) -> Result<(), String> {
    let mut last = String::new();
    for attempt in 1..=3 {
        match bot.set_chat_title(ChatId(group_id), title).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                last = e.to_string();
                tracing::warn!(
                    "setChatTitle группы {} не удался (попытка {}/3): {}",
                    group_id, attempt, last
                );
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
        }
    }
    Err(last)
}

/// Выпуск пригласительной ссылки с повтором.
async fn export_link_with_retry(bot: &Bot, group_id: i64) -> Result<String, String> {
    let mut last = String::new();
    for attempt in 1..=3 {
        match bot.export_chat_invite_link(ChatId(group_id)).await {
            Ok(link) => return Ok(link),
            Err(e) => {
                last = e.to_string();
                tracing::warn!(
                    "exportChatInviteLink группы {} не удался (попытка {}/3): {}",
                    group_id, attempt, last
                );
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
        }
    }
    Err(last)
}

/// Возвращает захваченную, но не использованную сделочную в пул.
/// Без этого группа навсегда остаётся занятой без сделки.
async fn release_claimed_pool_group(group_id: i64) {
    if let Err(e) = queries::set_pool_status_by_group(group_id, "free").await {
        tracing::error!("Не удалось вернуть сделочную {} в пул: {}", group_id, e);
    }
}

/// Полный цикл создания сделки: захват сделочной + регистрация в БД.
/// При ошибке регистрации группа возвращается в пул.
pub async fn start_deal_in_pool(
    bot: &Bot,
    deal_display: &str,
    creator_id: i64,
) -> Result<(i32, PoolGroup, String), StartDealError> {
    let Some((entry, invite_link)) = occupy_pool_group(bot, deal_display)
        .await
        .map_err(StartDealError::Register)?
    else {
        return Err(StartDealError::NoFreeGroup);
    };

    let amount = deal_display.split_whitespace().next().unwrap_or("1").replace(',', ".");
    let currency = deal_display.split_whitespace().nth(1).unwrap_or("RUB");

    match register_deal(
        entry.group_id,
        &invite_link,
        &amount,
        &currency,
        creator_id,
        Some(deal_display),
    )
    .await
    {
        Ok(deal_id) => {
            tracing::info!(
                "Сделка #{} создана (сделочная №{}, группа {})",
                deal_id, entry.number, entry.group_id
            );
            Ok((deal_id, entry, invite_link))
        }
        Err(e) => {
            let _ = queries::set_pool_status_by_group(entry.group_id, "free").await;
            Err(StartDealError::Register(e))
        }
    }
}

/// Отзывает пригласительную ссылку: после этого по ней нельзя войти.
///
/// Используется при сбросе сделки — выданная участникам ссылка становится
/// нерабочей, чтобы в группу нельзя было зайти повторно или посторонним.
async fn revoke_link(bot: &Bot, group_id: i64, link: &str) -> bool {
    let link = link.trim();
    if link.is_empty() {
        return false;
    }
    match bot.revoke_chat_invite_link(ChatId(group_id), link).await {
        Ok(_) => {
            tracing::info!("Ссылка сделочной {} отозвана", group_id);
            true
        }
        Err(e) => {
            tracing::warn!("Не удалось отозвать ссылку сделочной {}: {}", group_id, e);
            false
        }
    }
}

/// Полностью освобождает сделочную: кикает участников, отзывает выданную
/// ссылку, удаляет сделку, возвращает название/описание/аватар, перевыпускает
/// ссылку для следующих сделок.
pub async fn release_pool_group(
    bot: &Bot,
    group_id: i64,
    keep_user_ids: HashSet<i64>,
) -> Result<Option<i32>, Box<dyn std::error::Error + Send + Sync>> {
    let entry = match queries::get_pool_entry_by_group(group_id).await? {
        Some(e) => e,
        None => return Ok(None),
    };

    let members = get_group_members(group_id);
    let mut kicked = 0;
    for uid in members {
        if keep_user_ids.contains(&uid) || uid <= 0 {
            continue;
        }
        match bot.ban_chat_member(ChatId(group_id), teloxide::types::UserId(uid as u64)).await {
            Ok(_) => {
                let _ = bot.unban_chat_member(ChatId(group_id), teloxide::types::UserId(uid as u64)).await;
                kicked += 1;
            }
            Err(e) => tracing::debug!("Не удалось кикнуть {} из {}: {}", uid, group_id, e),
        }
    }

    clear_group_members(group_id);

    // Ссылки, выданные участникам сделки, больше не работают: иначе после
    // сброса можно было бы войти в группу повторно или постороннему.
    let mut links: Vec<String> = Vec::new();
    if let Some(deal) = queries::get_deal_by_group(group_id).await? {
        links.push(deal.group_link);
    }
    if let Some(pool_link) = entry.invite_link.clone() {
        links.push(pool_link);
    }
    links.retain(|l| !l.trim().is_empty());
    links.sort();
    links.dedup();

    let mut revoked = 0;
    for link in links {
        if revoke_link(bot, group_id, &link).await {
            revoked += 1;
        }
    }

    let _ = queries::delete_deal_by_group(group_id).await;

    let number = entry.number;
    let _ = bot.set_chat_title(ChatId(group_id), &txt::pool_group_title(number)).await;
    let _ = bot.set_chat_description(ChatId(group_id)).description(txt::STANDARD_GROUP_DESCRIPTION).await;

    if !entry.avatar_set {
        let _ = bot.set_chat_photo(ChatId(group_id), teloxide::types::InputFile::file("assets/Avatarka_Group.png")).await;
        let _ = queries::set_pool_avatar_set(group_id, true).await;
    }

    let new_link = match bot.export_chat_invite_link(ChatId(group_id)).await {
        Ok(link) => link,
        Err(e) => {
            tracing::warn!("Не удалось перевыпустить ссылку сделочной {}: {}", group_id, e);
            entry.invite_link.unwrap_or_default()
        }
    };
    let _ = queries::set_pool_invite_link(group_id, Some(&new_link)).await;
    let _ = queries::set_pool_status_by_group(group_id, "free").await;

    tracing::info!(
        "Сделочная {} сброшена (кикнуто {}, отозвано ссылок {}, номер {})",
        group_id,
        kicked,
        revoked,
        number
    );
    Ok(Some(number))
}
