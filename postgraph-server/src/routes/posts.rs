use crate::db;
use crate::state::AppState;
use crate::types::Post;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct DebugQuery {
    pub since: Option<String>,
}

#[derive(Serialize)]
pub struct DebugPost {
    pub id: String,
    pub text_preview: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub views: i32,
    pub likes: i32,
    pub replies_count: i32,
    pub reposts: i32,
    pub quotes: i32,
    pub synced_at: DateTime<Utc>,
    pub sentiment: Option<f32>,
    pub intent: Option<String>,
    pub subject: Option<String>,
    pub last_captured_at: Option<DateTime<Utc>>,
}

pub async fn list_posts(
    State(state): State<AppState>,
) -> Result<Json<Vec<Post>>, axum::http::StatusCode> {
    db::get_all_posts(&state.pool)
        .await
        .map(Json)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Serialize)]
pub struct PostDetail {
    #[serde(flatten)]
    pub post: Post,
    pub topics: Vec<String>,
    pub engagement_rate: f64,
}

pub async fn get_post(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<PostDetail>, axum::http::StatusCode> {
    let (post_result, topics_result) = tokio::join!(
        db::get_post_by_id(&state.pool, &post_id),
        db::get_topics_for_post(&state.pool, &post_id),
    );

    let post = post_result
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    let topics = topics_result.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let total_interactions = (post.likes + post.replies_count + post.reposts + post.quotes) as f64;
    let engagement_rate = if total_interactions > 0.0 {
        total_interactions
    } else {
        0.0
    };

    Ok(Json(PostDetail {
        post,
        topics,
        engagement_rate,
    }))
}

pub async fn get_debug_posts(
    State(state): State<AppState>,
    Query(query): Query<DebugQuery>,
) -> Result<Json<Vec<DebugPost>>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| Utc::now() - chrono::Duration::days(1));

    let rows: Vec<DebugPost> = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            DateTime<Utc>,
            i32,
            i32,
            i32,
            i32,
            i32,
            DateTime<Utc>,
            Option<f32>,
            Option<String>,
            Option<String>,
            Option<DateTime<Utc>>,
        ),
    >(
        r#"SELECT p.id, LEFT(p.text, 120) AS text_preview,
                  p.timestamp, p.views, p.likes, p.replies_count, p.reposts, p.quotes,
                  p.synced_at,
                  p.sentiment,
                  i.name AS intent,
                  s.name AS subject,
                  es.captured_at AS last_captured_at
           FROM posts p
           LEFT JOIN intents i ON p.intent_id = i.id
           LEFT JOIN subjects s ON p.subject_id = s.id
           LEFT JOIN LATERAL (
               SELECT captured_at FROM engagement_snapshots
               WHERE post_id = p.id ORDER BY captured_at DESC LIMIT 1
           ) es ON true
           WHERE p.timestamp >= $1
           ORDER BY p.timestamp DESC"#,
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(
        |(
            id,
            text_preview,
            timestamp,
            views,
            likes,
            replies_count,
            reposts,
            quotes,
            synced_at,
            sentiment,
            intent,
            subject,
            last_captured_at,
        )| {
            DebugPost {
                id,
                text_preview,
                timestamp,
                views,
                likes,
                replies_count,
                reposts,
                quotes,
                synced_at,
                sentiment,
                intent,
                subject,
                last_captured_at,
            }
        },
    )
    .collect();

    Ok(Json(rows))
}
