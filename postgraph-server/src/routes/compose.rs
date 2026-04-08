use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::compose;
use crate::state::AppState;

#[derive(Serialize)]
pub struct ComposeError {
    pub error: String,
}

type ComposeResult<T> = Result<Json<T>, (axum::http::StatusCode, Json<ComposeError>)>;

fn err(status: axum::http::StatusCode, msg: impl ToString) -> (axum::http::StatusCode, Json<ComposeError>) {
    (status, Json(ComposeError { error: msg.to_string() }))
}

fn internal(e: impl ToString) -> (axum::http::StatusCode, Json<ComposeError>) {
    err(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e)
}

const MAX_TEXT_LENGTH: usize = 500;

// -- Request/Response types --

#[derive(Deserialize)]
pub struct CreateRequest {
    pub text: String,
    pub status: Option<String>,
    pub scheduled_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub struct UpdateRequest {
    pub text: Option<String>,
    pub status: Option<String>,
    pub scheduled_at: Option<Option<DateTime<Utc>>>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}

#[derive(Serialize)]
pub struct PublishNowResponse {
    pub threads_post_id: String,
}

// -- Handlers --

pub async fn list_posts(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> ComposeResult<Vec<compose::ScheduledPost>> {
    let posts = compose::list(&state.pool, query.status.as_deref(), query.from, query.to)
        .await
        .map_err(internal)?;
    Ok(Json(posts))
}

pub async fn create_post(
    State(state): State<AppState>,
    Json(body): Json<CreateRequest>,
) -> ComposeResult<compose::ScheduledPost> {
    if body.text.is_empty() {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "Text cannot be empty"));
    }
    if body.text.chars().count() > MAX_TEXT_LENGTH {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, format!("Text exceeds {MAX_TEXT_LENGTH} character limit")));
    }

    let status = body.status.as_deref().unwrap_or("draft");
    if status != "draft" && status != "scheduled" {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "Status must be 'draft' or 'scheduled'"));
    }
    if status == "scheduled" && body.scheduled_at.is_none() {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "scheduled_at is required when status is 'scheduled'"));
    }

    let post = compose::create(&state.pool, &body.text, status, body.scheduled_at)
        .await
        .map_err(internal)?;
    Ok(Json(post))
}

pub async fn get_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ComposeResult<compose::ScheduledPost> {
    let post = compose::get(&state.pool, id)
        .await
        .map_err(internal)?;
    match post {
        Some(p) => Ok(Json(p)),
        None => Err(err(axum::http::StatusCode::NOT_FOUND, "Post not found")),
    }
}

pub async fn update_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRequest>,
) -> ComposeResult<compose::ScheduledPost> {
    if let Some(ref text) = body.text {
        if text.is_empty() {
            return Err(err(axum::http::StatusCode::BAD_REQUEST, "Text cannot be empty"));
        }
        if text.chars().count() > MAX_TEXT_LENGTH {
            return Err(err(axum::http::StatusCode::BAD_REQUEST, format!("Text exceeds {MAX_TEXT_LENGTH} character limit")));
        }
    }
    if let Some(ref status) = body.status {
        if !["draft", "scheduled", "cancelled"].contains(&status.as_str()) {
            return Err(err(axum::http::StatusCode::BAD_REQUEST, "Invalid status"));
        }
    }

    let post = compose::update(&state.pool, id, body.text.as_deref(), body.status.as_deref(), body.scheduled_at)
        .await
        .map_err(internal)?;
    match post {
        Some(p) => Ok(Json(p)),
        None => Err(err(axum::http::StatusCode::NOT_FOUND, "Post not found")),
    }
}

pub async fn delete_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ComposeResult<DeleteResponse> {
    let deleted = compose::delete(&state.pool, id)
        .await
        .map_err(internal)?;
    Ok(Json(DeleteResponse { deleted }))
}

pub async fn publish_now(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ComposeResult<PublishNowResponse> {
    let post = compose::get(&state.pool, id)
        .await
        .map_err(internal)?;
    let post = match post {
        Some(p) => p,
        None => return Err(err(axum::http::StatusCode::NOT_FOUND, "Post not found")),
    };
    if post.status == "published" {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "Post is already published"));
    }
    if post.status == "publishing" {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "Post is currently being published"));
    }
    if post.status == "cancelled" {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "Cannot publish a cancelled post"));
    }

    compose::update(&state.pool, id, None, Some("publishing"), None)
        .await
        .map_err(internal)?;

    let container_id = match state.threads.create_container(&post.text).await {
        Ok(cid) => cid,
        Err(e) => {
            let _ = compose::mark_failed(&state.pool, id, &e.to_string()).await;
            return Err(err(axum::http::StatusCode::BAD_GATEWAY, format!("Threads API error: {e}")));
        }
    };

    let threads_post_id = match state.threads.publish_container(&container_id).await {
        Ok(pid) => pid,
        Err(e) => {
            let _ = compose::mark_failed(&state.pool, id, &e.to_string()).await;
            return Err(err(axum::http::StatusCode::BAD_GATEWAY, format!("Threads API error: {e}")));
        }
    };

    compose::mark_published(&state.pool, id, &threads_post_id)
        .await
        .map_err(internal)?;

    Ok(Json(PublishNowResponse { threads_post_id }))
}
