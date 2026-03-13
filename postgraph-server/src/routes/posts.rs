use axum::{extract::State, Json};
use crate::db;
use crate::state::AppState;
use crate::types::Post;

pub async fn list_posts(State(state): State<AppState>) -> Result<Json<Vec<Post>>, axum::http::StatusCode> {
    db::get_all_posts(&state.pool)
        .await
        .map(Json)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}
