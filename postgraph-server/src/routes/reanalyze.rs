use axum::Json;
use axum::extract::State;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tracing::info;

use crate::analysis;
use crate::db;
use crate::graph;
use crate::state::AppState;

#[derive(Serialize)]
pub struct ReanalyzeResult {
    pub started: bool,
    pub message: String,
}

/// Trigger a full reanalysis in the background. Returns immediately to avoid
/// gateway timeouts.
pub async fn trigger_reanalyze(
    State(state): State<AppState>,
) -> Result<Json<ReanalyzeResult>, axum::http::StatusCode> {
    if state.analysis_running.swap(true, Ordering::SeqCst) {
        return Ok(Json(ReanalyzeResult {
            started: false,
            message: "Analysis already in progress".to_string(),
        }));
    }

    info!("Reanalyze triggered — resetting all analysis");

    let posts_reset = db::reset_all_analysis(&state.pool).await.map_err(|e| {
        state.analysis_running.store(false, Ordering::SeqCst);
        tracing::error!("Reset analysis failed: {e}");
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Count total for progress tracking
    state.analysis_progress.store(0, Ordering::SeqCst);
    state
        .analysis_total
        .store(posts_reset as u32, Ordering::SeqCst);

    info!("Reset {posts_reset} posts, starting background reanalysis");

    let bg = state.clone();
    tokio::spawn(async move {
        let mut total_analyzed: u32 = 0;
        let mut consecutive_failures: u32 = 0;

        loop {
            match analysis::run_analysis(&bg.pool, &bg.mercury).await {
                Ok(0) => break,
                Ok(n) => {
                    total_analyzed += n;
                    consecutive_failures = 0;
                    bg.analysis_progress.store(total_analyzed, Ordering::SeqCst);
                    info!("Reanalysis progress: {total_analyzed}/{posts_reset}");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                Err(e) => {
                    consecutive_failures += 1;
                    tracing::error!(
                        "Reanalysis batch failed (attempt {consecutive_failures}): {e}"
                    );
                    if consecutive_failures >= 10 {
                        tracing::error!("Stopping reanalysis after 10 consecutive failures");
                        break;
                    }
                    let delay =
                        std::time::Duration::from_secs(2u64.pow(consecutive_failures.min(6)));
                    tokio::time::sleep(delay).await;
                }
            }
        }

        info!("Reanalysis complete ({total_analyzed} posts), computing edges...");
        loop {
            match graph::compute_edges_for_recent(&bg.pool).await {
                Ok(0) => break,
                Ok(n) => info!("Computed batch of {n} edges"),
                Err(e) => {
                    tracing::error!("Edge computation failed: {e}");
                    break;
                }
            }
        }

        // Set analysis_running = false FIRST so categorize doesn't conflict
        bg.analysis_running.store(false, Ordering::SeqCst);

        // Auto-trigger categorization after reanalysis
        info!("Reanalysis complete, running categorization...");
        bg.categorize_running.store(true, Ordering::SeqCst);
        if let Err(e) = crate::routes::categorize::run_full_categorization(&bg).await {
            tracing::error!("Auto-categorization after reanalysis failed: {e}");
        }
        bg.categorize_running.store(false, Ordering::SeqCst);

        info!("Background reanalysis task finished: {total_analyzed} posts analyzed");
    });

    Ok(Json(ReanalyzeResult {
        started: true,
        message: format!("{posts_reset} posts queued for reanalysis"),
    }))
}
