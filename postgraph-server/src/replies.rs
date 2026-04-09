use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Reply {
    pub id: String,
    pub parent_post_id: String,
    pub username: Option<String>,
    pub text: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    pub status: String,
    pub replied_at: Option<DateTime<Utc>>,
    pub our_reply_id: Option<String>,
    pub synced_at: DateTime<Utc>,
}

/// Reply with parent post context for the inbox view.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ReplyWithContext {
    pub id: String,
    pub parent_post_id: String,
    pub username: Option<String>,
    pub text: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    pub status: String,
    pub replied_at: Option<DateTime<Utc>>,
    pub our_reply_id: Option<String>,
    pub synced_at: DateTime<Utc>,
    pub parent_post_text: Option<String>,
}

/// Upsert a reply from the Threads API. New replies get status 'unreplied'.
/// Existing replies only update synced_at — never overwrite status.
pub async fn upsert_reply(
    pool: &PgPool,
    id: &str,
    parent_post_id: &str,
    username: Option<&str>,
    text: Option<&str>,
    timestamp: Option<DateTime<Utc>>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO replies (id, parent_post_id, username, text, timestamp)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (id) DO UPDATE SET synced_at = now()"
    )
    .bind(id)
    .bind(parent_post_id)
    .bind(username)
    .bind(text)
    .bind(timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

/// List replies with parent post context.
pub async fn list(pool: &PgPool, status: Option<&str>) -> Result<Vec<ReplyWithContext>, AppError> {
    let rows = sqlx::query_as::<_, ReplyWithContext>(
        "SELECT r.*, LEFT(p.text, 80) AS parent_post_text
         FROM replies r
         LEFT JOIN posts p ON r.parent_post_id = p.id
         WHERE ($1::text IS NULL OR r.status = $1)
         ORDER BY CASE WHEN r.status = 'unreplied' THEN 0 ELSE 1 END, r.timestamp ASC"
    )
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Count unreplied replies.
pub async fn count_unreplied(pool: &PgPool) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM replies WHERE status = 'unreplied'")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Get a single reply by ID.
pub async fn get(pool: &PgPool, id: &str) -> Result<Option<Reply>, AppError> {
    let row = sqlx::query_as::<_, Reply>("SELECT * FROM replies WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Mark a reply as replied, storing our reply ID.
pub async fn mark_replied(pool: &PgPool, id: &str, our_reply_id: &str) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE replies SET status = 'replied', replied_at = now(), our_reply_id = $2 WHERE id = $1"
    )
    .bind(id)
    .bind(our_reply_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Mark a reply as dismissed.
pub async fn mark_dismissed(pool: &PgPool, id: &str) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE replies SET status = 'dismissed' WHERE id = $1"
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Get post IDs from the last N days (for reply sync scope).
pub async fn recent_post_ids(pool: &PgPool, days: i32) -> Result<Vec<String>, AppError> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM posts WHERE timestamp >= now() - make_interval(days => $1) ORDER BY timestamp DESC"
    )
    .bind(days)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}
