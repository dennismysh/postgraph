use axum::{Json, extract::State};
use serde::Serialize;
use std::sync::atomic::Ordering;
use tracing::info;

use crate::analysis;
use crate::graph;
use crate::state::AppState;

#[derive(Serialize)]
pub struct AnalyzeStartResult {
    pub started: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct AnalyzeStatus {
    pub running: bool,
    pub analyzed: u32,
    pub total: u32,
}

pub async fn start_analyze(
    State(state): State<AppState>,
) -> Result<Json<AnalyzeStartResult>, axum::http::StatusCode> {
    // Check if already running
    if state.analysis_running.swap(true, Ordering::SeqCst) {
        return Ok(Json(AnalyzeStartResult {
            started: false,
            message: "Analysis already in progress".to_string(),
        }));
    }

    // Count unanalyzed posts
    let unanalyzed_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM posts WHERE analyzed_at IS NULL")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| {
                state.analysis_running.store(false, Ordering::SeqCst);
                tracing::error!("Failed to count unanalyzed posts: {e}");
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            })?;

    state.analysis_progress.store(0, Ordering::SeqCst);
    state
        .analysis_total
        .store(unanalyzed_count as u32, Ordering::SeqCst);

    info!("Starting background analysis of {unanalyzed_count} posts");

    // Spawn background task
    let bg_state = state.clone();
    tokio::spawn(async move {
        let mut total_analyzed: u32 = 0;
        let mut consecutive_failures: u32 = 0;

        loop {
            match analysis::run_analysis(&bg_state.pool, &bg_state.mercury).await {
                Ok(0) => break,
                Ok(n) => {
                    total_analyzed += n;
                    consecutive_failures = 0;
                    bg_state
                        .analysis_progress
                        .store(total_analyzed, Ordering::SeqCst);
                    info!("Analysis progress: {total_analyzed}/{unanalyzed_count}");
                    // Brief pause between batches to avoid rate-limiting
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                Err(e) => {
                    consecutive_failures += 1;
                    tracing::error!("Analysis batch failed (attempt {consecutive_failures}): {e}");
                    if consecutive_failures >= 10 {
                        tracing::error!("Stopping analysis after 10 consecutive failures");
                        break;
                    }
                    // Exponential backoff: 2s, 4s, 8s, 16s, 32s, ...
                    let delay =
                        std::time::Duration::from_secs(2u64.pow(consecutive_failures.min(6)));
                    info!("Retrying in {}s...", delay.as_secs());
                    tokio::time::sleep(delay).await;
                }
            }
        }

        // Compute edges for all newly analyzed posts
        info!("Analysis complete ({total_analyzed} posts), computing edges...");
        loop {
            match graph::compute_edges_for_recent(&bg_state.pool).await {
                Ok(0) => break,
                Ok(n) => info!("Computed batch of {n} edges"),
                Err(e) => {
                    tracing::error!("Edge computation failed: {e}");
                    break;
                }
            }
        }

        // Auto-categorize if no categories exist yet
        let cat_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories")
            .fetch_one(&bg_state.pool)
            .await
            .unwrap_or(0);
        if cat_count == 0 && total_analyzed > 0 {
            info!("No categories exist, running auto-categorization...");
            bg_state.categorize_running.store(true, Ordering::SeqCst);
            if let Err(e) = crate::routes::categorize::run_full_categorization(&bg_state).await {
                tracing::error!("Auto-categorization failed: {e}");
            }
            bg_state.categorize_running.store(false, Ordering::SeqCst);
        }

        bg_state.analysis_running.store(false, Ordering::SeqCst);
        info!("Background analysis task finished: {total_analyzed} posts analyzed");
    });

    Ok(Json(AnalyzeStartResult {
        started: true,
        message: format!("{unanalyzed_count} posts queued for analysis"),
    }))
}

pub async fn analyze_status(State(state): State<AppState>) -> Json<AnalyzeStatus> {
    Json(AnalyzeStatus {
        running: state.analysis_running.load(Ordering::SeqCst),
        analyzed: state.analysis_progress.load(Ordering::SeqCst),
        total: state.analysis_total.load(Ordering::SeqCst),
    })
}
