use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use tracing::info;

use crate::state::AppState;
use crate::sync::{refresh_all_metrics, run_sync};

#[derive(Serialize)]
pub struct SyncResult {
    pub posts_synced: u32,
    pub metrics_refreshed: u32,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

fn json_error(status: StatusCode, msg: String) -> Response {
    (status, Json(ErrorBody { error: msg })).into_response()
}

/// Sync posts and refresh metrics only. Analysis and edge computation are
/// handled by the background task and /api/analyze, keeping this endpoint
/// fast enough to avoid gateway timeouts.
pub async fn trigger_sync(State(state): State<AppState>) -> Result<Json<SyncResult>, Response> {
    info!("Manual sync triggered");

    let posts_synced = run_sync(&state.pool, &state.threads).await.map_err(|e| {
        tracing::error!("Sync failed: {e}");
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Sync failed: {e}"),
        )
    })?;

    // Refresh metrics (views, likes, etc.) for all existing posts
    let metrics_refreshed = refresh_all_metrics(&state.pool, &state.threads)
        .await
        .map_err(|e| {
            tracing::error!("Metrics refresh failed: {e}");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Metrics refresh failed: {e}"),
            )
        })?;

    Ok(Json(SyncResult {
        posts_synced,
        metrics_refreshed,
    }))
}
