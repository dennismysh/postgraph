use axum::{Json, extract::State};
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

pub async fn trigger_sync(
    State(state): State<AppState>,
) -> Result<Json<SyncResult>, axum::http::StatusCode> {
    info!("Manual sync triggered");

    let posts_synced = run_sync(&state.pool, &state.threads).await.map_err(|e| {
        tracing::error!("Sync failed: {e}");
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Refresh metrics (views, likes, etc.) for all existing posts
    let metrics_refreshed = refresh_all_metrics(&state.pool, &state.threads)
        .await
        .map_err(|e| {
            tracing::error!("Metrics refresh failed: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Loop analysis until all posts are analyzed (backfill)
    let mut posts_analyzed: u32 = 0;
    loop {
        let batch = analysis::run_analysis(&state.pool, &state.mercury)
            .await
            .map_err(|e| {
                tracing::error!("Analysis failed: {e}");
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
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
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
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
