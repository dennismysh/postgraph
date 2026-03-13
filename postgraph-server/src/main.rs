mod analysis;
mod auth;
mod db;
mod error;
mod graph;
mod mercury;
mod routes;
mod state;
mod sync;
mod threads;
mod types;

use axum::{
    Router, middleware,
    routing::{get, post},
};
use shuttle_axum::ShuttleAxum;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::mercury::MercuryClient;
use crate::state::AppState;
use crate::threads::ThreadsClient;

#[shuttle_runtime::main]
async fn main(#[shuttle_shared_db::Postgres] pool: PgPool) -> ShuttleAxum {
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("migrations failed");

    let threads_token =
        std::env::var("THREADS_ACCESS_TOKEN").expect("THREADS_ACCESS_TOKEN must be set");
    let mercury_key = std::env::var("MERCURY_API_KEY").expect("MERCURY_API_KEY must be set");
    let mercury_url = std::env::var("MERCURY_API_URL")
        .unwrap_or_else(|_| "https://api.inceptionlabs.ai/v1".to_string());
    let api_key = std::env::var("POSTGRAPH_API_KEY").expect("POSTGRAPH_API_KEY must be set");

    let state = AppState {
        pool: pool.clone(),
        threads: Arc::new(ThreadsClient::new(threads_token)),
        mercury: Arc::new(MercuryClient::new(mercury_key, mercury_url)),
        api_key,
    };

    // Spawn background sync task (first run after 30s delay, then every 15 min)
    let bg_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(15 * 60));
        loop {
            interval.tick().await;
            info!("Background sync starting");
            if let Err(e) = sync::run_sync(&bg_state.pool, &bg_state.threads).await {
                tracing::error!("Background sync failed: {e}");
                continue;
            }
            if let Err(e) = analysis::run_analysis(&bg_state.pool, &bg_state.mercury).await {
                tracing::error!("Background analysis failed: {e}");
            }
            if let Err(e) = graph::compute_edges_for_recent(&bg_state.pool).await {
                tracing::error!("Background edge computation failed: {e}");
            }
        }
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_routes = Router::new()
        .route("/api/posts", get(routes::posts::list_posts))
        .route("/api/graph", get(routes::graph::get_graph))
        .route("/api/analytics", get(routes::analytics::get_analytics))
        .route("/api/sync", post(routes::sync::trigger_sync))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_api_key,
        ));

    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(api_routes)
        .layer(cors)
        .with_state(state);

    Ok(router.into())
}
