use axum::Json;
use axum::extract::State;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tracing::info;

use crate::state::AppState;
use crate::sync;

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

        // Phase 1: discover posts
        {
            *bg.sync_message.write().await = "Discovering posts from Threads...".to_string();
        }
        match sync::sync_posts(&bg.pool, &bg.threads, progress).await {
            Ok(n) => {
                status_parts.push(format!("{n} discovered"));
                info!("Post discovery complete: {n} posts");
            }
            Err(e) => {
                tracing::error!("Post discovery failed: {e}");
                *bg.sync_message.write().await = format!("Sync failed: {e}");
                bg.sync_running.store(false, Ordering::SeqCst);
                return;
            }
        }

        // Phase 2: refresh per-post metrics
        {
            *bg.sync_message.write().await = "Refreshing per-post metrics...".to_string();
        }
        match sync::sync_post_metrics(&bg.pool, &bg.threads, progress).await {
            Ok(n) => {
                status_parts.push(format!("{n} metrics refreshed"));
                info!("Metrics refresh complete: {n} posts");
            }
            Err(e) => {
                tracing::error!("Metrics refresh failed: {e}");
                status_parts.push(format!("metrics failed: {e}"));
            }
        }

        // Phase 3: sync daily views
        {
            *bg.sync_message.write().await = "Syncing daily views...".to_string();
        }
        match sync::sync_daily_views(&bg.pool, &bg.threads).await {
            Ok(n) => {
                status_parts.push(format!("{n} days synced"));
                info!("Daily views sync complete: {n} days");
            }
            Err(e) => {
                tracing::error!("Daily views sync failed: {e}");
                status_parts.push(format!("daily views failed: {e}"));
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

    // Clear daily views
    sqlx::query("TRUNCATE daily_views")
        .execute(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    info!("Database reset complete");

    Ok(Json(ResetResult {
        success: true,
        message: "Database reset. Trigger a sync to re-fetch all data.".to_string(),
    }))
}
