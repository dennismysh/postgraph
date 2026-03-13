use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use tracing::info;

use crate::analysis;
use crate::db;
use crate::graph;
use crate::state::AppState;

#[derive(Serialize)]
pub struct ReanalyzeResult {
    pub posts_reset: u64,
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

pub async fn trigger_reanalyze(
    State(state): State<AppState>,
) -> Result<Json<ReanalyzeResult>, Response> {
    info!("Reanalyze triggered — resetting all analysis");

    let posts_reset = db::reset_all_analysis(&state.pool).await.map_err(|e| {
        tracing::error!("Reset analysis failed: {e}");
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Reset analysis failed: {e}"),
        )
    })?;

    info!("Reset {posts_reset} posts, re-running analysis");

    let mut posts_analyzed: u32 = 0;
    loop {
        let batch = analysis::run_analysis(&state.pool, &state.mercury)
            .await
            .map_err(|e| {
                tracing::error!("Reanalysis failed: {e}");
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Reanalysis failed: {e}"),
                )
            })?;
        posts_analyzed += batch;
        if batch == 0 {
            break;
        }
        info!("Reanalyzed batch of {batch} posts ({posts_analyzed} total so far)");
    }

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

    Ok(Json(ReanalyzeResult {
        posts_reset,
        posts_analyzed,
        edges_computed,
    }))
}
