use crate::types::*;
use sqlx::PgPool;

pub const CATEGORY_COLORS: &[&str] = &[
    "#e6194b", "#3cb44b", "#4363d8", "#f58231", "#911eb4", "#42d4f4", "#f032e6", "#bfef45",
    "#fabed4", "#469990", "#dcbeff", "#9A6324", "#800000", "#aaffc3", "#808000",
];

// -- Posts --

/// Upsert a post. Returns `true` if this was a new insert, `false` if it updated an existing row.
pub async fn upsert_post(pool: &PgPool, post: &Post) -> sqlx::Result<bool> {
    let row: (bool,) = sqlx::query_as(
        r#"INSERT INTO posts (id, text, media_type, media_url, timestamp, permalink, views, likes, replies_count, reposts, quotes, shares, synced_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, NOW())
           ON CONFLICT (id) DO UPDATE SET
             text = COALESCE(EXCLUDED.text, posts.text),
             media_type = COALESCE(EXCLUDED.media_type, posts.media_type),
             media_url = COALESCE(EXCLUDED.media_url, posts.media_url),
             permalink = COALESCE(EXCLUDED.permalink, posts.permalink),
             timestamp = EXCLUDED.timestamp,
             synced_at = NOW()
           RETURNING (xmax = 0) AS inserted"#,
    )
    .bind(&post.id)
    .bind(&post.text)
    .bind(&post.media_type)
    .bind(&post.media_url)
    .bind(post.timestamp)
    .bind(&post.permalink)
    .bind(post.views)
    .bind(post.likes)
    .bind(post.replies_count)
    .bind(post.reposts)
    .bind(post.quotes)
    .bind(post.shares)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn get_unanalyzed_posts(pool: &PgPool, limit: i64) -> sqlx::Result<Vec<Post>> {
    sqlx::query_as::<_, Post>(
        "SELECT * FROM posts WHERE analyzed_at IS NULL ORDER BY timestamp DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn get_all_posts(pool: &PgPool) -> sqlx::Result<Vec<Post>> {
    sqlx::query_as::<_, Post>("SELECT * FROM posts ORDER BY timestamp DESC")
        .fetch_all(pool)
        .await
}

/// Lightweight post data for graph nodes — avoids loading full text and media URLs.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GraphPost {
    pub id: String,
    pub text_preview: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub likes: i32,
    pub replies_count: i32,
    pub reposts: i32,
    pub quotes: i32,
    pub sentiment: Option<f32>,
    pub analyzed_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn get_posts_for_graph(pool: &PgPool) -> sqlx::Result<Vec<GraphPost>> {
    sqlx::query_as::<_, GraphPost>(
        "SELECT id, LEFT(text, 80) AS text_preview, timestamp, likes, replies_count, reposts, quotes, sentiment, analyzed_at FROM posts ORDER BY timestamp DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn get_post_by_id(pool: &PgPool, post_id: &str) -> sqlx::Result<Option<Post>> {
    sqlx::query_as::<_, Post>("SELECT * FROM posts WHERE id = $1")
        .bind(post_id)
        .fetch_optional(pool)
        .await
}

pub async fn get_topics_for_post(pool: &PgPool, post_id: &str) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT t.name FROM post_topics pt JOIN topics t ON pt.topic_id = t.id WHERE pt.post_id = $1 ORDER BY pt.weight DESC",
    )
    .bind(post_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(name,)| name).collect())
}

pub async fn mark_post_analyzed(pool: &PgPool, post_id: &str, sentiment: f32) -> sqlx::Result<()> {
    sqlx::query("UPDATE posts SET analyzed_at = NOW(), sentiment = $1 WHERE id = $2")
        .bind(sentiment)
        .bind(post_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn reset_all_analysis(pool: &PgPool) -> sqlx::Result<u64> {
    // Delete edges, post_topics, then reset analyzed_at
    sqlx::query("DELETE FROM post_edges").execute(pool).await?;
    sqlx::query("DELETE FROM post_topics").execute(pool).await?;
    sqlx::query("DELETE FROM topics").execute(pool).await?;
    let result = sqlx::query("UPDATE posts SET analyzed_at = NULL, sentiment = NULL")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn get_all_post_ids(pool: &PgPool) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT id FROM posts ORDER BY timestamp DESC")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

// -- Topics --

pub async fn upsert_topic(pool: &PgPool, name: &str, description: &str) -> sqlx::Result<Topic> {
    sqlx::query_as::<_, Topic>(
        r#"INSERT INTO topics (name, description)
           VALUES ($1, $2)
           ON CONFLICT (name) DO UPDATE SET description = EXCLUDED.description
           RETURNING *"#,
    )
    .bind(name)
    .bind(description)
    .fetch_one(pool)
    .await
}

pub async fn get_all_topics(pool: &PgPool) -> sqlx::Result<Vec<Topic>> {
    sqlx::query_as::<_, Topic>("SELECT * FROM topics ORDER BY name")
        .fetch_all(pool)
        .await
}

// -- Categories --

pub async fn get_all_categories(pool: &PgPool) -> sqlx::Result<Vec<Category>> {
    sqlx::query_as::<_, Category>("SELECT * FROM categories ORDER BY name")
        .fetch_all(pool)
        .await
}

pub async fn upsert_category(
    pool: &PgPool,
    name: &str,
    description: &str,
    color: &str,
) -> sqlx::Result<Category> {
    sqlx::query_as::<_, Category>(
        r#"INSERT INTO categories (name, description, color)
           VALUES ($1, $2, $3)
           ON CONFLICT (name) DO UPDATE SET description = EXCLUDED.description
           RETURNING *"#,
    )
    .bind(name)
    .bind(description)
    .bind(color)
    .fetch_one(pool)
    .await
}

pub async fn set_topic_category(
    pool: &PgPool,
    topic_name: &str,
    category_id: uuid::Uuid,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE topics SET category_id = $1 WHERE name = $2")
        .bind(category_id)
        .bind(topic_name)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_orphaned_categories(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM categories WHERE id NOT IN (SELECT DISTINCT category_id FROM topics WHERE category_id IS NOT NULL)",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn get_categories_with_topics(
    pool: &PgPool,
) -> sqlx::Result<Vec<(Category, Vec<String>)>> {
    let categories = get_all_categories(pool).await?;
    let mut result = Vec::new();
    for cat in categories {
        let topic_names: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM topics WHERE category_id = $1 ORDER BY name")
                .bind(cat.id)
                .fetch_all(pool)
                .await?;
        let names: Vec<String> = topic_names.into_iter().map(|(n,)| n).collect();
        result.push((cat, names));
    }
    Ok(result)
}

pub async fn upsert_post_topic(
    pool: &PgPool,
    post_id: &str,
    topic_id: uuid::Uuid,
    weight: f32,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"INSERT INTO post_topics (post_id, topic_id, weight)
           VALUES ($1, $2, $3)
           ON CONFLICT (post_id, topic_id) DO UPDATE SET weight = EXCLUDED.weight"#,
    )
    .bind(post_id)
    .bind(topic_id)
    .bind(weight)
    .execute(pool)
    .await?;
    Ok(())
}

// -- Edges --

pub async fn upsert_edge(pool: &PgPool, edge: &PostEdge) -> sqlx::Result<()> {
    sqlx::query(
        r#"INSERT INTO post_edges (source_post_id, target_post_id, edge_type, weight)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (source_post_id, target_post_id, edge_type)
           DO UPDATE SET weight = EXCLUDED.weight"#,
    )
    .bind(&edge.source_post_id)
    .bind(&edge.target_post_id)
    .bind(&edge.edge_type)
    .bind(edge.weight)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_all_edges(pool: &PgPool) -> sqlx::Result<Vec<PostEdge>> {
    sqlx::query_as::<_, PostEdge>("SELECT * FROM post_edges WHERE weight >= 0.1")
        .fetch_all(pool)
        .await
}

// -- Engagement Snapshots --

pub async fn insert_engagement_snapshot(
    pool: &PgPool,
    post_id: &str,
    views: i32,
    likes: i32,
    replies_count: i32,
    reposts: i32,
    quotes: i32,
) -> sqlx::Result<()> {
    // Skip if a snapshot for this post already exists within the last 10 minutes
    // to avoid near-duplicate entries from re-syncs.
    sqlx::query(
        r#"INSERT INTO engagement_snapshots (post_id, views, likes, replies_count, reposts, quotes)
           SELECT $1, $2, $3, $4, $5, $6
           WHERE NOT EXISTS (
               SELECT 1 FROM engagement_snapshots
               WHERE post_id = $1 AND captured_at > NOW() - INTERVAL '10 minutes'
           )"#,
    )
    .bind(post_id)
    .bind(views)
    .bind(likes)
    .bind(replies_count)
    .bind(reposts)
    .bind(quotes)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_engagement_history(
    pool: &PgPool,
    post_id: &str,
) -> sqlx::Result<Vec<EngagementSnapshot>> {
    sqlx::query_as::<_, EngagementSnapshot>(
        "SELECT * FROM engagement_snapshots WHERE post_id = $1 ORDER BY captured_at",
    )
    .bind(post_id)
    .fetch_all(pool)
    .await
}

// -- API Tokens --

/// Stored token with expiry info.
pub struct StoredToken {
    pub access_token: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Load the stored Threads API token (if any).
pub async fn load_token(pool: &PgPool) -> sqlx::Result<Option<StoredToken>> {
    let row: Option<(String, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as("SELECT access_token, expires_at FROM api_tokens WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(access_token, expires_at)| StoredToken {
        access_token,
        expires_at,
    }))
}

/// Save (upsert) the Threads API token with its expiry.
pub async fn save_token(
    pool: &PgPool,
    access_token: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"INSERT INTO api_tokens (id, access_token, expires_at, refreshed_at)
           VALUES (1, $1, $2, NOW())
           ON CONFLICT (id) DO UPDATE SET
             access_token = EXCLUDED.access_token,
             expires_at = EXCLUDED.expires_at,
             refreshed_at = NOW()"#,
    )
    .bind(access_token)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

// -- Sync State --

pub async fn get_sync_state(pool: &PgPool) -> sqlx::Result<SyncState> {
    sqlx::query_as::<_, SyncState>("SELECT * FROM sync_state WHERE id = 1")
        .fetch_one(pool)
        .await
}

pub async fn update_sync_state(pool: &PgPool, cursor: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE sync_state SET last_sync_cursor = $1, last_sync_at = NOW() WHERE id = 1")
        .bind(cursor)
        .execute(pool)
        .await?;
    Ok(())
}
