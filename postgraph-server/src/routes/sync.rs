use axum::{extract::State, Json};
use serde::Serialize;
use tracing::info;

use crate::analysis;
use crate::graph;
use crate::state::AppState;
use crate::sync::run_sync;

#[derive(Serialize)]
pub struct SyncResult {
    pub posts_synced: u32,
    pub posts_analyzed: u32,
    pub edges_computed: u32,
}

pub async fn trigger_sync(
    State(state): State<AppState>,
) -> Result<Json<SyncResult>, axum::http::StatusCode> {
    info!("Manual sync triggered");

    let posts_synced = run_sync(&state.pool, &state.threads)
        .await
        .map_err(|e| {
            tracing::error!("Sync failed: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let posts_analyzed = analysis::run_analysis(&state.pool, &state.mercury)
        .await
        .map_err(|e| {
            tracing::error!("Analysis failed: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let edges_computed = graph::compute_edges_for_recent(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("Edge computation failed: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(SyncResult {
        posts_synced,
        posts_analyzed,
        edges_computed,
    }))
}
