use chrono::{DateTime, Utc};
use sqlx::FromRow;

#[derive(Debug, FromRow, Clone)]
pub struct User {
    pub telegram_id: i64,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub deals_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Clone)]
pub struct Deal {
    pub id: i32,
    pub group_id: i64,
    pub group_link: String,
    pub title: String,
    pub amount: String,
    pub currency: String,
    pub buyer_id: i64,
    pub seller_id: Option<i64>,
    pub guarantor_id: Option<i64>,
    pub ruchenik_id: Option<i64>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Clone)]
pub struct MessageLog {
    pub id: i32,
    pub deal_id: i32,
    pub user_id: i64,
    pub username: Option<String>,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Clone)]
pub struct PoolGroup {
    pub id: i32,
    pub group_id: i64,
    pub number: i32,
    pub status: String,
    pub owner_session: Option<i64>,
    pub invite_link: Option<String>,
    pub avatar_set: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Clone)]
pub struct CustomCommand {
    pub id: i32,
    pub trigger_text: String,
    pub trigger_lower: String,
    pub source_chat_id: i64,
    pub source_message_id: i64,
    pub created_by: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Clone)]
pub struct CustomSms {
    pub id: i32,
    pub people_count: i32,
    pub source_chat_id: i64,
    pub source_message_id: i64,
    pub created_by: Option<i64>,
    pub created_at: DateTime<Utc>,
}
