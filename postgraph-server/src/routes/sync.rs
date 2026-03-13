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

    let bg = state.clone();
    tokio::spawn(async move {
        let mut status_parts: Vec<String> = Vec::new();

        // Phase 1: sync posts
        {
            *bg.sync_message.write().await = "Syncing posts from Threads...".to_string();
        }
        match run_sync(&bg.pool, &bg.threads).await {
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
        match refresh_all_metrics(&bg.pool, &bg.threads).await {
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
    })
}
