use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use tracing::info;

use crate::analysis;
use crate::graph;
use crate::state::AppState;
use crate::sync::{refresh_all_metrics, run_sync};

#[derive(Serialize)]
pub struct SyncResult {
    pub posts_synced: u32,
    pub metrics_refreshed: u32,
    pub posts_analyzed: u32,
    pub edges_computed: u32,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

fn json_error(status: StatusCode, msg: String) -> Response {
    (status, Json(ErrorBody { error: msg })).into_response()
}

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

    // Loop analysis until all posts are analyzed (backfill)
    let mut posts_analyzed: u32 = 0;
    loop {
        let batch = analysis::run_analysis(&state.pool, &state.mercury)
            .await
            .map_err(|e| {
                tracing::error!("Analysis failed: {e}");
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Analysis failed: {e}"),
                )
            })?;
        posts_analyzed += batch;
        if batch == 0 {
            break;
        }
        info!("Analyzed batch of {batch} posts ({posts_analyzed} total so far)");
    }

    // Loop edge computation until all analyzed posts have edges
    let mut edges_computed: u32 = 0;
    loop {
        let batch = graph::compute_edges_for_recent(&state.pool)
            .await
            .map_err(|e| {
                tracing::error!("Edge computation failed: {e}");
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Edge computation failed: {e}"),
                )
            })?;
        edges_computed += batch;
        if batch == 0 {
            break;
        }
        info!("Computed batch of {batch} edges ({edges_computed} total so far)");
    }

    Ok(Json(SyncResult {
        posts_synced,
        metrics_refreshed,
        posts_analyzed,
        edges_computed,
    }))
}
