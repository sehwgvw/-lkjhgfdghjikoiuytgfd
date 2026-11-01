use crate::database::connection::get_pool;
use crate::database::models::{CustomCommand, CustomSms, Deal, PoolGroup, User};

use sqlx::FromRow;
use sqlx::Row;

// ─── Пользователи ────────────────────────────────────────────

pub async fn upsert_user(
    telegram_id: i64,
    username: Option<&str>,
    first_name: Option<&str>,
) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        r#"
        INSERT INTO users (telegram_id, username, first_name)
        VALUES (?, ?, ?)
        ON CONFLICT (telegram_id) DO UPDATE
        SET username = COALESCE(?, users.username),
            first_name = COALESCE(?, users.first_name)
        "#,
    )
    .bind(telegram_id)
    .bind(username)
    .bind(first_name)
    .bind(username)
    .bind(first_name)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn increment_deals_count(telegram_id: i64) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "UPDATE users SET deals_count = deals_count + 1 WHERE telegram_id = ?",
    )
    .bind(telegram_id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn get_user_by_username(username: &str) -> Result<Option<User>, sqlx::Error> {
    let clean = username.trim_start_matches('@').to_lowercase();
    let pool = get_pool().await?;
    let row = sqlx::query(
        "SELECT telegram_id, username, first_name, deals_count, created_at FROM users WHERE LOWER(username) = ? LIMIT 1",
    )
    .bind(&clean)
    .fetch_optional(&pool)
    .await?;
    Ok(row.map(|r| User::from_row(&r).unwrap()))
}

pub async fn get_user_by_id(telegram_id: i64) -> Result<Option<User>, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query(
        "SELECT telegram_id, username, first_name, deals_count, created_at FROM users WHERE telegram_id = ? LIMIT 1",
    )
    .bind(telegram_id)
    .fetch_optional(&pool)
    .await?;
    Ok(row.map(|r| User::from_row(&r).unwrap()))
}

// ─── Сделки ──────────────────────────────────────────────────

pub async fn create_deal(
    group_id: i64,
    group_link: &str,
    title: &str,
    amount: &str,
    currency: &str,
    buyer_id: i64,
) -> Result<i32, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO deals (group_id, group_link, title, amount, currency, buyer_id, status)
        VALUES (?, ?, ?, ?, ?, ?, 'active')
        "#,
    )
    .bind(group_id)
    .bind(group_link)
    .bind(title)
    .bind(amount)
    .bind(currency)
    .bind(buyer_id)
    .execute(&pool)
    .await?;
    Ok(row.last_insert_rowid() as i32)
}

pub async fn get_deal_by_group(group_id: i64) -> Result<Option<Deal>, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query("SELECT * FROM deals WHERE group_id = ?")
        .bind(group_id)
        .fetch_optional(&pool)
        .await?;
    Ok(row.map(|r| Deal::from_row(&r).unwrap()))
}

pub async fn get_deal_by_id(deal_id: i32) -> Result<Option<Deal>, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query("SELECT * FROM deals WHERE id = ?")
        .bind(deal_id)
        .fetch_optional(&pool)
        .await?;
    Ok(row.map(|r| Deal::from_row(&r).unwrap()))
}

pub async fn update_deal_status(group_id: i64, status: &str) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "UPDATE deals SET status = ?, updated_at = CURRENT_TIMESTAMP WHERE group_id = ?",
    )
    .bind(status)
    .bind(group_id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn update_deal_status_by_id(deal_id: i32, status: &str) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "UPDATE deals SET status = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(status)
    .bind(deal_id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn set_deal_seller(group_id: i64, seller_id: i64) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "UPDATE deals SET seller_id = ?, updated_at = CURRENT_TIMESTAMP WHERE group_id = ?",
    )
    .bind(seller_id)
    .bind(group_id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn set_deal_guarantor(group_id: i64, guarantor_id: i64) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "UPDATE deals SET guarantor_id = ?, updated_at = CURRENT_TIMESTAMP WHERE group_id = ?",
    )
    .bind(guarantor_id)
    .bind(group_id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn set_deal_ruchenik(deal_id: i32, ruchenik_id: i64) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "UPDATE deals SET ruchenik_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(ruchenik_id)
    .bind(deal_id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn delete_deal_by_group(group_id: i64) -> Result<bool, sqlx::Error> {
    let pool = get_pool().await?;
    let res = sqlx::query("DELETE FROM deals WHERE group_id = ?")
        .bind(group_id)
        .execute(&pool)
        .await?;
    Ok(res.rows_affected() == 1)
}

// ─── Пул сделочных ───────────────────────────────────────────

pub async fn create_pool_entry(
    group_id: i64,
    number: i32,
    owner_session: Option<i64>,
    invite_link: Option<&str>,
) -> Result<i32, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO deal_pool (group_id, number, owner_session, invite_link, status)
        VALUES (?, ?, ?, ?, 'free')
        ON CONFLICT (group_id) DO UPDATE
        SET number = excluded.number,
            owner_session = excluded.owner_session,
            status = 'free',
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(group_id)
    .bind(number)
    .bind(owner_session)
    .bind(invite_link)
    .execute(&pool)
    .await?;
    Ok(row.last_insert_rowid() as i32)
}

pub async fn get_pool_entry_by_group(group_id: i64) -> Result<Option<PoolGroup>, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query("SELECT * FROM deal_pool WHERE group_id = ?")
        .bind(group_id)
        .fetch_optional(&pool)
        .await?;
    Ok(row.map(|r| PoolGroup::from_row(&r).unwrap()))
}

pub async fn get_max_pool_number() -> Result<i32, sqlx::Error> {
    let pool = get_pool().await?;
    let val: Option<i64> = sqlx::query_scalar("SELECT MAX(number) FROM deal_pool")
        .fetch_one(&pool)
        .await?;
    Ok(val.unwrap_or(0) as i32)
}

pub async fn count_pool_entries(statuses: &[&str]) -> Result<i64, sqlx::Error> {
    let pool = get_pool().await?;
    if statuses.is_empty() {
        let row: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM deal_pool")
            .fetch_one(&pool)
            .await?;
        return Ok(row);
    }
    let placeholders: Vec<String> = statuses.iter().map(|_| "?".to_string()).collect();
    let sql = format!("SELECT COUNT(*) FROM deal_pool WHERE status IN ({})", placeholders.join(", "));
    let mut query = sqlx::query::<sqlx::Sqlite>(&sql);
    for s in statuses {
        query = query.bind(s);
    }
    let row: i64 = query.fetch_one(&pool).await?.get(0);
    Ok(row)
}

pub async fn claim_free_pool_group() -> Result<Option<PoolGroup>, sqlx::Error> {
    let pool = get_pool().await?;
    // Атомарно занимаем свободную группу и возвращаем именно её:
    // RETURNING * гарантирует, что вернётся захваченная строка, а не любая занятая.
    let row = sqlx::query_as::<_, PoolGroup>(
        r#"
        UPDATE deal_pool
        SET status = 'occupied', updated_at = CURRENT_TIMESTAMP
        WHERE id = (
            SELECT id FROM deal_pool
            WHERE status = 'free'
            ORDER BY number ASC
            LIMIT 1
        )
        RETURNING *
        "#,
    )
    .fetch_optional(&pool)
    .await?;
    Ok(row)
}

pub async fn set_pool_status_by_group(group_id: i64, status: &str) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "UPDATE deal_pool SET status = ?, updated_at = CURRENT_TIMESTAMP WHERE group_id = ?",
    )
    .bind(status)
    .bind(group_id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn set_pool_invite_link(group_id: i64, invite_link: Option<&str>) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "UPDATE deal_pool SET invite_link = ?, updated_at = CURRENT_TIMESTAMP WHERE group_id = ?",
    )
    .bind(invite_link)
    .bind(group_id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn set_pool_avatar_set(group_id: i64, avatar_set: bool) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "UPDATE deal_pool SET avatar_set = ?, updated_at = CURRENT_TIMESTAMP WHERE group_id = ?",
    )
    .bind(avatar_set as i32)
    .bind(group_id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn get_all_pool_entries() -> Result<Vec<PoolGroup>, sqlx::Error> {
    let pool = get_pool().await?;
    let rows = sqlx::query("SELECT * FROM deal_pool ORDER BY number ASC")
        .fetch_all(&pool)
        .await?;
    Ok(rows.into_iter().map(|r| PoolGroup::from_row(&r).unwrap()).collect())
}

pub async fn get_active_deals() -> Result<Vec<Deal>, sqlx::Error> {
    let pool = get_pool().await?;
    let rows = sqlx::query(
        "SELECT * FROM deals WHERE status IN ('created', 'waiting', 'active') ORDER BY id ASC",
    )
    .fetch_all(&pool)
    .await?;
    Ok(rows.into_iter().map(|r| Deal::from_row(&r).unwrap()).collect())
}

/// Занятые группы, у которых нет активной сделки (залипли из-за сбоя).
pub async fn get_stale_occupied_pool_groups() -> Result<Vec<PoolGroup>, sqlx::Error> {
    let pool = get_pool().await?;
    let rows = sqlx::query(
        r#"
        SELECT p.* FROM deal_pool p
        WHERE p.status = 'occupied'
          AND NOT EXISTS (
              SELECT 1 FROM deals d
              WHERE d.group_id = p.group_id
                AND d.status IN ('created', 'waiting', 'active')
          )
        ORDER BY p.number ASC
        "#,
    )
    .fetch_all(&pool)
    .await?;
    Ok(rows.into_iter().map(|r| PoolGroup::from_row(&r).unwrap()).collect())
}

// ─── Команды-триггеры ────────────────────────────────────────

pub async fn add_custom_command(
    trigger: &str,
    source_chat_id: i64,
    source_message_id: i64,
    created_by: Option<i64>,
) -> Result<Option<i32>, sqlx::Error> {
    let trigger = trigger.trim();
    let pool = get_pool().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO custom_commands (trigger_text, trigger_lower, source_chat_id, source_message_id, created_by)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT (trigger_lower) DO NOTHING
        "#,
    )
    .bind(trigger)
    .bind(trigger.to_lowercase())
    .bind(source_chat_id)
    .bind(source_message_id)
    .bind(created_by)
    .execute(&pool)
    .await?;
    Ok(Some(row.last_insert_rowid() as i32))
}

pub async fn delete_custom_command_by_trigger(trigger: &str) -> Result<Option<CustomCommand>, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query(
        "SELECT * FROM custom_commands WHERE trigger_lower = ?",
    )
    .bind(trigger.trim().to_lowercase())
    .fetch_optional(&pool)
    .await?;
    
    let deleted = row.map(|r| CustomCommand::from_row(&r).unwrap());

    sqlx::query(
        "DELETE FROM custom_commands WHERE trigger_lower = ?",
    )
    .bind(trigger.trim().to_lowercase())
    .execute(&pool)
    .await?;

    Ok(deleted)
}

pub async fn get_all_custom_commands() -> Result<Vec<CustomCommand>, sqlx::Error> {
    let pool = get_pool().await?;
    let rows = sqlx::query("SELECT * FROM custom_commands ORDER BY created_at ASC")
        .fetch_all(&pool)
        .await?;
    Ok(rows.into_iter().map(|r| CustomCommand::from_row(&r).unwrap()).collect())
}

pub async fn get_custom_command_by_trigger(trigger: &str) -> Result<Option<CustomCommand>, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query(
        "SELECT * FROM custom_commands WHERE trigger_lower = ?",
    )
    .bind(trigger.trim().to_lowercase())
    .fetch_optional(&pool)
    .await?;
    Ok(row.map(|r| CustomCommand::from_row(&r).unwrap()))
}

pub async fn count_custom_commands() -> Result<i64, sqlx::Error> {
    let pool = get_pool().await?;
    let val: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM custom_commands")
        .fetch_one(&pool)
        .await?;
    Ok(val)
}

// ─── Sms-сообщения ───────────────────────────────────────────

pub async fn add_custom_sms(
    people_count: i32,
    source_chat_id: i64,
    source_message_id: i64,
    created_by: Option<i64>,
) -> Result<Option<i32>, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO custom_sms (people_count, source_chat_id, source_message_id, created_by)
        VALUES (?, ?, ?, ?)
        ON CONFLICT (people_count) DO NOTHING
        "#,
    )
    .bind(people_count)
    .bind(source_chat_id)
    .bind(source_message_id)
    .bind(created_by)
    .execute(&pool)
    .await?;
    Ok(Some(row.last_insert_rowid() as i32))
}

pub async fn delete_custom_sms_by_count(people_count: i32) -> Result<Option<CustomSms>, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query(
        "SELECT * FROM custom_sms WHERE people_count = ?",
    )
    .bind(people_count)
    .fetch_optional(&pool)
    .await?;
    
    let deleted = row.map(|r| CustomSms::from_row(&r).unwrap());

    sqlx::query(
        "DELETE FROM custom_sms WHERE people_count = ?",
    )
    .bind(people_count)
    .execute(&pool)
    .await?;

    Ok(deleted)
}

pub async fn get_all_custom_sms() -> Result<Vec<CustomSms>, sqlx::Error> {
    let pool = get_pool().await?;
    let rows = sqlx::query("SELECT * FROM custom_sms ORDER BY people_count ASC")
        .fetch_all(&pool)
        .await?;
    Ok(rows.into_iter().map(|r| CustomSms::from_row(&r).unwrap()).collect())
}

pub async fn get_custom_sms_by_count(people_count: i32) -> Result<Option<CustomSms>, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query(
        "SELECT * FROM custom_sms WHERE people_count = ?",
    )
    .bind(people_count)
    .fetch_optional(&pool)
    .await?;
    Ok(row.map(|r| CustomSms::from_row(&r).unwrap()))
}

pub async fn get_sms_to_fire(deal_id: i32, member_count: i32) -> Result<Vec<CustomSms>, sqlx::Error> {
    let pool = get_pool().await?;
    let rows = sqlx::query(
        r#"
        SELECT s.* FROM custom_sms s
        WHERE s.people_count <= ?
          AND NOT EXISTS (
              SELECT 1 FROM sms_fires f
              WHERE f.sms_id = s.id AND f.deal_id = ?
          )
        ORDER BY s.people_count ASC
        "#,
    )
    .bind(member_count)
    .bind(deal_id)
    .fetch_all(&pool)
    .await?;
    Ok(rows.into_iter().map(|r| CustomSms::from_row(&r).unwrap()).collect())
}

pub async fn mark_sms_fired(deal_id: i32, sms_id: i32) -> Result<bool, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO sms_fires (deal_id, sms_id)
        VALUES (?, ?)
        ON CONFLICT (deal_id, sms_id) DO NOTHING
        "#,
    )
    .bind(deal_id)
    .bind(sms_id)
    .execute(&pool)
    .await?;
    Ok(row.rows_affected() == 1)
}

pub async fn count_custom_sms() -> Result<i64, sqlx::Error> {
    let pool = get_pool().await?;
    let val: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM custom_sms")
        .fetch_one(&pool)
        .await?;
    Ok(val)
}

// ─── Логи сообщений ─────────────────────────────────────────

pub async fn log_message(
    deal_id: i32,
    user_id: i64,
    username: Option<&str>,
    message: &str,
) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "INSERT INTO messages_log (deal_id, user_id, username, message) VALUES (?, ?, ?, ?)",
    )
    .bind(deal_id)
    .bind(user_id)
    .bind(username)
    .bind(message)
    .execute(&pool)
    .await?;
    Ok(())
}

// ─── Состояние бота ─────────────────────────────────────────

pub async fn get_bot_state(key: &str) -> Result<Option<String>, sqlx::Error> {
    let pool = get_pool().await?;
    let row = sqlx::query("SELECT value FROM bot_state WHERE key = ?")
        .bind(key)
        .fetch_optional(&pool)
        .await?;
    Ok(row.and_then(|r| r.try_get::<Option<String>, _>("value").ok().flatten()))
}

pub async fn set_bot_state(key: &str, value: &str) -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;
    sqlx::query(
        "INSERT INTO bot_state (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(&pool)
    .await?;
    Ok(())
}
