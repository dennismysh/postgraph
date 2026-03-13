use crate::db;
use crate::state::AppState;
use crate::types::Post;
use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;

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
    let post = db::get_post_by_id(&state.pool, &post_id)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    let topics = db::get_topics_for_post(&state.pool, &post_id)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

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
