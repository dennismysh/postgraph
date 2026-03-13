use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub database: ServiceStatus,
    pub threads_api: ServiceStatus,
    pub mercury_api: ServiceStatus,
}

#[derive(Serialize)]
pub struct ServiceStatus {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ServiceStatus {
    fn ok() -> Self {
        Self {
            status: "ok",
            error: None,
        }
    }

    fn err(msg: String) -> Self {
        Self {
            status: "error",
            error: Some(msg),
        }
    }
}

pub async fn detailed_health(State(state): State<AppState>) -> Json<HealthResponse> {
    let (db, threads, mercury) = tokio::join!(
        check_database(&state),
        check_threads(&state),
        check_mercury(&state),
    );

    let overall = if db.status == "ok" && threads.status == "ok" && mercury.status == "ok" {
        "ok"
    } else {
        "degraded"
    };

    Json(HealthResponse {
        status: overall,
        database: db,
        threads_api: threads,
        mercury_api: mercury,
    })
}

async fn check_database(state: &AppState) -> ServiceStatus {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
    {
        Ok(_) => ServiceStatus::ok(),
        Err(e) => ServiceStatus::err(e.to_string()),
    }
}

async fn check_threads(state: &AppState) -> ServiceStatus {
    match state.threads.health_check().await {
        Ok(()) => ServiceStatus::ok(),
        Err(e) => ServiceStatus::err(e.to_string()),
    }
}

async fn check_mercury(state: &AppState) -> ServiceStatus {
    match state.mercury.health_check().await {
        Ok(()) => ServiceStatus::ok(),
        Err(e) => ServiceStatus::err(e.to_string()),
    }
}
