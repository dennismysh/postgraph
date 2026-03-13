use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::state::AppState;

pub async fn require_api_key(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(header) if header.strip_prefix("Bearer ").unwrap_or("") == state.api_key => {
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
