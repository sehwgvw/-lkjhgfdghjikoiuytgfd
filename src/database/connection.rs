use std::sync::OnceLock;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;

static POOL: OnceLock<SqlitePool> = OnceLock::new();

pub async fn get_pool() -> Result<SqlitePool, sqlx::Error> {
    if let Some(pool) = POOL.get() {
        return Ok(pool.clone());
    }

    let db_path = crate::config::config().database_path.clone();
    tracing::info!("Используется БД: {}", db_path.display());

    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .disable_statement_logging();

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    if POOL.set(pool.clone()).is_err() {
        return Ok(POOL.get().unwrap().clone());
    }
    Ok(pool)
}

pub async fn init_db() -> Result<(), sqlx::Error> {
    let pool = get_pool().await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            telegram_id  INTEGER PRIMARY KEY,
            username     TEXT,
            first_name   TEXT,
            deals_count  INTEGER DEFAULT 0,
            created_at   DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS deals (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            group_id      INTEGER UNIQUE,
            group_link    TEXT,
            title         TEXT NOT NULL,
            amount        TEXT NOT NULL,
            currency      TEXT NOT NULL,
            buyer_id      INTEGER,
            seller_id     INTEGER,
            guarantor_id  INTEGER,
            ruchenik_id   INTEGER,
            status        TEXT DEFAULT 'created'
                          CHECK (status IN ('created', 'waiting', 'active', 'completed', 'disputed', 'cancelled')),
            created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at    DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS messages_log (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            deal_id     INTEGER REFERENCES deals(id) ON DELETE CASCADE,
            user_id     INTEGER,
            username    TEXT,
            message     TEXT,
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS deal_pool (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            group_id      INTEGER UNIQUE NOT NULL,
            number        INTEGER NOT NULL,
            status        TEXT NOT NULL DEFAULT 'free'
                          CHECK (status IN ('free', 'occupied', 'deleted')),
            owner_session INTEGER,
            invite_link   TEXT,
            avatar_set    INTEGER NOT NULL DEFAULT 0,
            created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at    DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS custom_commands (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            trigger_text      TEXT NOT NULL,
            trigger_lower     TEXT NOT NULL UNIQUE,
            source_chat_id    INTEGER NOT NULL,
            source_message_id INTEGER NOT NULL,
            created_by        INTEGER,
            created_at        DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS custom_sms (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            people_count      INTEGER NOT NULL UNIQUE CHECK (people_count > 0),
            source_chat_id    INTEGER NOT NULL,
            source_message_id INTEGER NOT NULL,
            created_by        INTEGER,
            created_at        DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sms_fires (
            deal_id  INTEGER NOT NULL,
            sms_id   INTEGER NOT NULL REFERENCES custom_sms(id) ON DELETE CASCADE,
            fired_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (deal_id, sms_id)
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS bot_state (
            key   TEXT PRIMARY KEY,
            value TEXT
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_deals_group_id ON deals(group_id);")
        .execute(&pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_deal_id ON messages_log(deal_id);")
        .execute(&pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_deal_pool_status ON deal_pool(status);")
        .execute(&pool)
        .await?;

    Ok(())
}

pub async fn close_db() {
    if let Some(pool) = POOL.get() {
        pool.close().await;
    }
}
