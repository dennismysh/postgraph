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
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::time::Duration;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::mercury::MercuryClient;
use crate::state::AppState;
use crate::threads::ThreadsClient;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "postgraph_server=info,tower_http=info".parse().unwrap()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPool::connect(&database_url)
        .await
        .expect("failed to connect to database");

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

    // Load token from DB if available (persists across deploys), otherwise use env var.
    let effective_token = match db::load_token(&pool).await {
        Ok(Some(stored)) => {
            info!(
                "Loaded Threads token from database (expires {:?})",
                stored.expires_at
            );
            stored.access_token
        }
        _ => {
            info!("No stored token found, using THREADS_ACCESS_TOKEN env var");
            // Seed the DB with the env var token (assume 60-day expiry from now)
            let expires_at = chrono::Utc::now() + chrono::Duration::days(60);
            if let Err(e) = db::save_token(&pool, &threads_token, expires_at).await {
                tracing::warn!("Failed to seed token to database: {e}");
            }
            threads_token
        }
    };

    let state = AppState {
        pool: pool.clone(),
        threads: Arc::new(ThreadsClient::new(effective_token)),
        mercury: Arc::new(MercuryClient::new(mercury_key, mercury_url)),
        api_key,
        analysis_running: Arc::new(AtomicBool::new(false)),
        analysis_progress: Arc::new(AtomicU32::new(0)),
        analysis_total: Arc::new(AtomicU32::new(0)),
        sync_running: Arc::new(AtomicBool::new(false)),
        sync_message: Arc::new(tokio::sync::RwLock::new(String::new())),
        sync_progress: Arc::new(AtomicU32::new(0)),
        sync_total: Arc::new(AtomicU32::new(0)),
    };

    // Spawn background sync task (first run after 30s delay, then every 15 min)
    let bg_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(15 * 60));
        loop {
            interval.tick().await;

            // Refresh Threads token if it expires within 7 days
            if let Ok(Some(stored)) = db::load_token(&bg_state.pool).await {
                let should_refresh = stored
                    .expires_at
                    .map(|exp| exp - chrono::Utc::now() < chrono::Duration::days(7))
                    .unwrap_or(false);
                if should_refresh {
                    info!("Threads token expires soon, refreshing...");
                    match bg_state.threads.refresh_token().await {
                        Ok((new_token, expires_in)) => {
                            let expires_at =
                                chrono::Utc::now() + chrono::Duration::seconds(expires_in);
                            if let Err(e) =
                                db::save_token(&bg_state.pool, &new_token, expires_at).await
                            {
                                tracing::error!("Failed to save refreshed token: {e}");
                            } else {
                                info!("Threads token refreshed, expires at {expires_at}");
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to refresh Threads token: {e}");
                        }
                    }
                }
            }

            info!("Background sync starting");
            if let Err(e) = sync::run_sync(&bg_state.pool, &bg_state.threads, None).await {
                tracing::error!("Background sync failed: {e}");
                continue;
            }
            // Refresh metrics for all existing posts so views/likes stay current
            if let Err(e) = sync::refresh_all_metrics(&bg_state.pool, &bg_state.threads, None).await
            {
                tracing::error!("Background metrics refresh failed: {e}");
            }
            let mut consecutive_failures = 0;
            loop {
                match analysis::run_analysis(&bg_state.pool, &bg_state.mercury).await {
                    Ok(0) => break,
                    Ok(n) => {
                        info!("Background analysis batch: {n} posts");
                        consecutive_failures = 0;
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        tracing::error!(
                            "Background analysis failed (attempt {consecutive_failures}): {e}"
                        );
                        if consecutive_failures >= 3 {
                            tracing::error!("Stopping analysis after 3 consecutive failures");
                            break;
                        }
                        // Brief pause before retrying
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
            if let Err(e) = graph::compute_subject_edges(&bg_state.pool).await {
                tracing::error!("Background edge computation failed: {e}");
            }
        }
    });

    // Spawn nightly sync task at 2am in user's timezone
    let timezone_str = std::env::var("TIMEZONE").unwrap_or_else(|_| "UTC".to_string());
    let tz: chrono_tz::Tz = timezone_str.parse().unwrap_or_else(|_| {
        tracing::warn!("Invalid TIMEZONE '{timezone_str}', defaulting to UTC");
        chrono_tz::UTC
    });
    let nightly_state = state.clone();
    tokio::spawn(async move {
        loop {
            let sleep_dur = duration_until_2am(tz);
            info!(
                "Nightly sync scheduled in {:.1}h ({tz})",
                sleep_dur.as_secs_f64() / 3600.0
            );
            tokio::time::sleep(sleep_dur).await;

            info!("Nightly sync starting");
            if let Err(e) = sync::run_sync(&nightly_state.pool, &nightly_state.threads, None).await
            {
                tracing::error!("Nightly sync failed: {e}");
            }
            if let Err(e) =
                sync::refresh_all_metrics(&nightly_state.pool, &nightly_state.threads, None).await
            {
                tracing::error!("Nightly metrics refresh failed: {e}");
            }
            // Run analysis + edge computation for any new posts
            let mut consecutive_failures = 0;
            loop {
                match analysis::run_analysis(&nightly_state.pool, &nightly_state.mercury).await {
                    Ok(0) => break,
                    Ok(n) => {
                        info!("Nightly analysis batch: {n} posts");
                        consecutive_failures = 0;
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        tracing::error!(
                            "Nightly analysis failed (attempt {consecutive_failures}): {e}"
                        );
                        if consecutive_failures >= 3 {
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
            if let Err(e) = graph::compute_subject_edges(&nightly_state.pool).await {
                tracing::error!("Nightly edge computation failed: {e}");
            }
            info!("Nightly sync complete");
        }
    });

    let frontend_origin =
        std::env::var("FRONTEND_ORIGIN").unwrap_or_else(|_| "http://localhost:5173".to_string());
    let cors = CorsLayer::new()
        .allow_origin(
            frontend_origin
                .parse::<axum::http::HeaderValue>()
                .expect("FRONTEND_ORIGIN must be a valid origin"),
        )
        .allow_methods(Any)
        .allow_headers(Any);

    let api_routes = Router::new()
        .route("/api/posts", get(routes::posts::list_posts))
        .route("/api/posts/{id}", get(routes::posts::get_post))
        .route("/api/graph", get(routes::graph::get_graph))
        .route("/api/analytics", get(routes::analytics::get_analytics))
        .route("/api/analytics/views", get(routes::analytics::get_views))
        .route(
            "/api/analytics/heatmap",
            get(routes::analytics::get_heatmap),
        )
        .route(
            "/api/analytics/engagement",
            get(routes::analytics::get_engagement),
        )
        .route(
            "/api/analytics/histograms",
            get(routes::analytics::get_histograms),
        )
        .route(
            "/api/posts/{id}/engagement",
            get(routes::analytics::get_post_engagement),
        )
        .route("/api/sync", post(routes::sync::trigger_sync))
        .route("/api/sync/status", get(routes::sync::sync_status))
        .route("/api/reanalyze", post(routes::reanalyze::trigger_reanalyze))
        .route("/api/analyze", post(routes::analyze::start_analyze))
        .route("/api/analyze/status", get(routes::analyze::analyze_status))
        .route(
            "/api/subjects/{id}/posts",
            get(routes::subjects::get_subject_posts),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_api_key,
        ));

    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/health/detail", get(routes::health::detailed_health))
        .merge(api_routes)
        .layer(cors)
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await.expect("failed to bind");
    info!("Listening on {addr}");
    axum::serve(listener, router).await.expect("server error");
}

/// Calculate how long to sleep until the next 2am in the given timezone.
fn duration_until_2am(tz: chrono_tz::Tz) -> Duration {
    use chrono::Timelike;

    let now_local = chrono::Utc::now().with_timezone(&tz);
    let target_date = if now_local.hour() >= 2 {
        now_local.date_naive() + chrono::Duration::days(1)
    } else {
        now_local.date_naive()
    };
    let target_naive = target_date.and_hms_opt(2, 0, 0).unwrap();

    // Handle DST: earliest() covers normal + ambiguous; None means spring-forward gap
    let target_utc = match target_naive.and_local_timezone(tz).earliest() {
        Some(t) => t.with_timezone(&chrono::Utc),
        None => (target_naive + chrono::Duration::hours(1))
            .and_local_timezone(tz)
            .earliest()
            .expect("3am must exist")
            .with_timezone(&chrono::Utc),
    };

    let duration = target_utc - chrono::Utc::now();
    duration.to_std().unwrap_or(Duration::from_secs(60))
}
