use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::replies;
use crate::state::AppState;

#[derive(Serialize)]
pub struct RepliesError {
    pub error: String,
}

type RepliesResult<T> = Result<Json<T>, (axum::http::StatusCode, Json<RepliesError>)>;

fn err(
    status: axum::http::StatusCode,
    msg: impl ToString,
) -> (axum::http::StatusCode, Json<RepliesError>) {
    (
        status,
        Json(RepliesError {
            error: msg.to_string(),
        }),
    )
}

fn internal(e: impl ToString) -> (axum::http::StatusCode, Json<RepliesError>) {
    err(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e)
}

const MAX_REPLY_LENGTH: usize = 500;

#[derive(Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
}

#[derive(Serialize)]
pub struct CountResponse {
    pub count: i64,
}

#[derive(Deserialize)]
pub struct ReplyRequest {
    pub text: String,
}

#[derive(Serialize)]
pub struct ReplyResponse {
    pub our_reply_id: String,
}

#[derive(Serialize)]
pub struct DismissResponse {
    pub dismissed: bool,
}

pub async fn list_replies(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> RepliesResult<Vec<replies::ReplyWithContext>> {
    let status = query.status.as_deref().or(Some("unreplied"));
    let list = replies::list(&state.pool, status).await.map_err(internal)?;
    Ok(Json(list))
}

pub async fn count_unreplied(State(state): State<AppState>) -> RepliesResult<CountResponse> {
    let count = replies::count_unreplied(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(CountResponse { count }))
}

pub async fn send_reply(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReplyRequest>,
) -> RepliesResult<ReplyResponse> {
    if body.text.trim().is_empty() {
        return Err(err(
            axum::http::StatusCode::BAD_REQUEST,
            "Reply text cannot be empty",
        ));
    }
    if body.text.chars().count() > MAX_REPLY_LENGTH {
        return Err(err(
            axum::http::StatusCode::BAD_REQUEST,
            format!("Reply exceeds {MAX_REPLY_LENGTH} character limit"),
        ));
    }

    // Verify the reply exists
    let target = replies::get(&state.pool, &id).await.map_err(internal)?;
    if target.is_none() {
        return Err(err(axum::http::StatusCode::NOT_FOUND, "Reply not found"));
    }

    // Send the reply via Threads API (reply_to_id is the reply we're responding to)
    let our_reply_id = state
        .threads
        .create_reply(&id, &body.text)
        .await
        .map_err(|e| {
            err(
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Threads API error: {e}"),
            )
        })?;

    // Mark as replied
    replies::mark_replied(&state.pool, &id, &our_reply_id)
        .await
        .map_err(internal)?;

    Ok(Json(ReplyResponse { our_reply_id }))
}

pub async fn dismiss_reply(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> RepliesResult<DismissResponse> {
    let dismissed = replies::mark_dismissed(&state.pool, &id)
        .await
        .map_err(internal)?;
    if !dismissed {
        return Err(err(axum::http::StatusCode::NOT_FOUND, "Reply not found"));
    }
    Ok(Json(DismissResponse { dismissed }))
}

#[derive(Serialize)]
pub struct DetectResponse {
    pub detected: u64,
}

pub async fn detect_replies(State(state): State<AppState>) -> RepliesResult<DetectResponse> {
    let detected =
        crate::sync::detect_external_replies(&state.pool, &state.threads, &state.owner_username)
            .await
            .map_err(internal)?;
    Ok(Json(DetectResponse { detected }))
}
