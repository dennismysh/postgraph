use axum::Json;
use axum::extract::State;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tracing::info;

use crate::state::AppState;
use crate::sync::{refresh_all_metrics, run_sync};

#[derive(Serialize)]
pub struct SyncStartResult {
    pub started: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct SyncStatus {
    pub running: bool,
    pub message: String,
    pub synced: u32,
    pub total: u32,
}

/// Kick off sync in a background task and return immediately, avoiding
/// gateway timeouts on Railway.
pub async fn trigger_sync(State(state): State<AppState>) -> Json<SyncStartResult> {
    if state.sync_running.swap(true, Ordering::SeqCst) {
        return Json(SyncStartResult {
            started: false,
            message: "Sync already in progress".to_string(),
        });
    }

    *state.sync_message.write().await = "Starting sync...".to_string();
    info!("Manual sync triggered (background)");

    // Reset progress counters
    state.sync_progress.store(0, Ordering::SeqCst);
    state.sync_total.store(0, Ordering::SeqCst);

    let bg = state.clone();
    tokio::spawn(async move {
        let mut status_parts: Vec<String> = Vec::new();
        let progress = Some((&bg.sync_progress, &bg.sync_total));

        // Phase 1: sync posts
        {
            *bg.sync_message.write().await = "Syncing posts from Threads...".to_string();
        }
        match run_sync(&bg.pool, &bg.threads, progress).await {
            Ok(n) => {
                status_parts.push(format!("{n} synced"));
                info!("Sync complete: {n} posts synced");
            }
            Err(e) => {
                tracing::error!("Sync failed: {e}");
                *bg.sync_message.write().await = format!("Sync failed: {e}");
                bg.sync_running.store(false, Ordering::SeqCst);
                return;
            }
        }

        // Phase 2: refresh metrics
        {
            *bg.sync_message.write().await = "Refreshing metrics...".to_string();
        }
        match refresh_all_metrics(&bg.pool, &bg.threads, progress).await {
            Ok(n) => {
                status_parts.push(format!("{n} refreshed"));
                info!("Metrics refresh complete: {n} posts refreshed");
            }
            Err(e) => {
                tracing::error!("Metrics refresh failed: {e}");
                status_parts.push(format!("metrics failed: {e}"));
            }
        }

        let done_msg = format!("Done! {}", status_parts.join(", "));
        *bg.sync_message.write().await = done_msg;
        bg.sync_running.store(false, Ordering::SeqCst);
    });

    Json(SyncStartResult {
        started: true,
        message: "Sync started".to_string(),
    })
}

pub async fn sync_status(State(state): State<AppState>) -> Json<SyncStatus> {
    Json(SyncStatus {
        running: state.sync_running.load(Ordering::SeqCst),
        message: state.sync_message.read().await.clone(),
        synced: state.sync_progress.load(Ordering::SeqCst),
        total: state.sync_total.load(Ordering::SeqCst),
    })
}

#[derive(Serialize)]
pub struct ResetResult {
    pub success: bool,
    pub message: String,
}

/// Wipe all data (posts, snapshots, analysis, graph) and reset sync cursor
/// so the next sync re-fetches everything from the Threads API.
pub async fn reset_database(
    State(state): State<AppState>,
) -> Result<Json<ResetResult>, axum::http::StatusCode> {
    if state.sync_running.load(Ordering::SeqCst) {
        return Ok(Json(ResetResult {
            success: false,
            message: "Cannot reset while sync is running".to_string(),
        }));
    }

    info!("Database reset requested — wiping all data");

    // Truncate in dependency order (children before parents)
    let tables = [
        "engagement_snapshots",
        "post_edges",
        "subject_edges",
        "post_topics",
        "topics",
        "categories",
        "posts",
        "intents",
        "subjects",
    ];
    for table in tables {
        sqlx::query(&format!("TRUNCATE {table} CASCADE"))
            .execute(&state.pool)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    // Reset sync cursor so next sync fetches everything
    sqlx::query("UPDATE sync_state SET last_sync_cursor = NULL, last_sync_at = NULL")
        .execute(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Reset user insights
    sqlx::query("UPDATE user_insights SET total_views = 0, captured_at = NOW()")
        .execute(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    info!("Database reset complete");

    Ok(Json(ResetResult {
        success: true,
        message: "Database reset. Trigger a sync to re-fetch all data.".to_string(),
    }))
}
