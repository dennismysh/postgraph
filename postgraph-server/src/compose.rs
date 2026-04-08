use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ScheduledPost {
    pub id: Uuid,
    pub text: String,
    pub status: String,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub published_at: Option<DateTime<Utc>>,
    pub threads_post_id: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn create(pool: &PgPool, text: &str, status: &str, scheduled_at: Option<DateTime<Utc>>) -> Result<ScheduledPost, AppError> {
    let row = sqlx::query_as::<_, ScheduledPost>(
        "INSERT INTO scheduled_posts (text, status, scheduled_at) VALUES ($1, $2, $3) RETURNING *"
    )
    .bind(text)
    .bind(status)
    .bind(scheduled_at)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn get(pool: &PgPool, id: Uuid) -> Result<Option<ScheduledPost>, AppError> {
    let row = sqlx::query_as::<_, ScheduledPost>(
        "SELECT * FROM scheduled_posts WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list(pool: &PgPool, status: Option<&str>, from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> Result<Vec<ScheduledPost>, AppError> {
    let rows = sqlx::query_as::<_, ScheduledPost>(
        "SELECT * FROM scheduled_posts
         WHERE ($1::text IS NULL OR status = $1)
           AND ($2::timestamptz IS NULL OR COALESCE(scheduled_at, created_at) >= $2)
           AND ($3::timestamptz IS NULL OR COALESCE(scheduled_at, created_at) < $3)
         ORDER BY COALESCE(scheduled_at, created_at) ASC"
    )
    .bind(status)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn update(pool: &PgPool, id: Uuid, text: Option<&str>, status: Option<&str>, scheduled_at: Option<Option<DateTime<Utc>>>) -> Result<Option<ScheduledPost>, AppError> {
    let row = sqlx::query_as::<_, ScheduledPost>(
        "UPDATE scheduled_posts SET
            text = COALESCE($2, text),
            status = COALESCE($3, status),
            scheduled_at = CASE WHEN $4 THEN $5 ELSE scheduled_at END,
            updated_at = now()
         WHERE id = $1
         RETURNING *"
    )
    .bind(id)
    .bind(text)
    .bind(status)
    .bind(scheduled_at.is_some())
    .bind(scheduled_at.flatten())
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM scheduled_posts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn claim_due_posts(pool: &PgPool) -> Result<Vec<ScheduledPost>, AppError> {
    let rows = sqlx::query_as::<_, ScheduledPost>(
        "UPDATE scheduled_posts
         SET status = 'publishing', updated_at = now()
         WHERE status = 'scheduled' AND scheduled_at <= now()
         RETURNING *"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_published(pool: &PgPool, id: Uuid, threads_post_id: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE scheduled_posts SET status = 'published', threads_post_id = $2, published_at = now(), updated_at = now() WHERE id = $1"
    )
    .bind(id)
    .bind(threads_post_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_failed(pool: &PgPool, id: Uuid, error: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE scheduled_posts SET status = 'failed', error_message = $2, updated_at = now() WHERE id = $1"
    )
    .bind(id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn recover_stuck(pool: &PgPool) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE scheduled_posts SET status = 'scheduled', updated_at = now()
         WHERE status = 'publishing' AND updated_at < now() - interval '5 minutes'"
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
