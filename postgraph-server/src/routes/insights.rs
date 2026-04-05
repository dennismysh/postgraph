use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::insights;
use crate::state::AppState;

#[derive(Serialize)]
pub struct InsightsResponse {
    pub id: String,
    pub generated_at: String,
    pub trigger_type: String,
    pub report: insights::InsightsReport,
}

#[derive(Serialize)]
pub struct InsightsError {
    pub error: String,
}

pub async fn get_latest(
    State(state): State<AppState>,
) -> Result<Json<InsightsResponse>, (axum::http::StatusCode, Json<InsightsError>)> {
    let report = insights::get_latest_report(&state.pool)
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(InsightsError { error: e.to_string() }),
            )
        })?;

    match report {
        Some(r) => Ok(Json(InsightsResponse {
            id: r.id,
            generated_at: r.generated_at.to_rfc3339(),
            trigger_type: r.trigger_type,
            report: r.report,
        })),
        None => Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(InsightsError { error: "No insights report generated yet".to_string() }),
        )),
    }
}

pub async fn generate(
    State(state): State<AppState>,
) -> Result<Json<InsightsResponse>, (axum::http::StatusCode, Json<InsightsError>)> {
    let report = insights::generate_report(&state.pool, &state.mercury, "manual")
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(InsightsError { error: e.to_string() }),
            )
        })?;

    Ok(Json(InsightsResponse {
        id: report.id,
        generated_at: report.generated_at.to_rfc3339(),
        trigger_type: report.trigger_type,
        report: report.report,
    }))
}
