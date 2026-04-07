use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::emotions;
use crate::state::AppState;

#[derive(Serialize)]
pub struct EmotionsSummaryResponse {
    pub window_start: String,
    pub window_end: String,
    pub total_posts: i64,
    pub emotions: Vec<emotions::EmotionStat>,
}

#[derive(Serialize)]
pub struct NarrativeResponse {
    pub id: String,
    pub generated_at: String,
    pub trigger_type: String,
    pub narrative: emotions::EmotionNarrative,
}

#[derive(Serialize)]
pub struct EmotionsError {
    pub error: String,
}

pub async fn get_summary(
    State(state): State<AppState>,
) -> Result<Json<EmotionsSummaryResponse>, (axum::http::StatusCode, Json<EmotionsError>)> {
    let summary = emotions::compute_summary(&state.pool).await.map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(EmotionsError {
                error: e.to_string(),
            }),
        )
    })?;

    Ok(Json(EmotionsSummaryResponse {
        window_start: summary.window_start,
        window_end: summary.window_end,
        total_posts: summary.total_posts,
        emotions: summary.emotions,
    }))
}

pub async fn get_narrative(
    State(state): State<AppState>,
) -> Result<Json<NarrativeResponse>, (axum::http::StatusCode, Json<EmotionsError>)> {
    let narrative = emotions::get_latest_narrative(&state.pool)
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(EmotionsError {
                    error: e.to_string(),
                }),
            )
        })?;

    match narrative {
        Some(n) => Ok(Json(NarrativeResponse {
            id: n.id,
            generated_at: n.generated_at.to_rfc3339(),
            trigger_type: n.trigger_type,
            narrative: n.narrative,
        })),
        None => Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(EmotionsError {
                error: "No emotion narrative generated yet".to_string(),
            }),
        )),
    }
}

#[derive(Serialize)]
pub struct BackfillResponse {
    pub classified: u32,
}

pub async fn backfill(
    State(state): State<AppState>,
) -> Result<Json<BackfillResponse>, (axum::http::StatusCode, Json<EmotionsError>)> {
    let classified = emotions::backfill_emotions(&state.pool, &state.mercury)
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(EmotionsError {
                    error: e.to_string(),
                }),
            )
        })?;

    Ok(Json(BackfillResponse { classified }))
}

pub async fn generate_narrative(
    State(state): State<AppState>,
) -> Result<Json<NarrativeResponse>, (axum::http::StatusCode, Json<EmotionsError>)> {
    let narrative = emotions::generate_narrative(&state.pool, &state.mercury, "manual")
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(EmotionsError {
                    error: e.to_string(),
                }),
            )
        })?;

    Ok(Json(NarrativeResponse {
        id: narrative.id,
        generated_at: narrative.generated_at.to_rfc3339(),
        trigger_type: narrative.trigger_type,
        narrative: narrative.narrative,
    }))
}
