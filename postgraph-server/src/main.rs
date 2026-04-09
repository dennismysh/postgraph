mod analysis;
mod auth;
mod compose;
mod db;
mod emotions;
mod error;
mod graph;
mod insights;
mod mercury;
mod replies;
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

    let threads = Arc::new(ThreadsClient::new(effective_token));

    // Resolve owner username from Threads API
    let owner_username = match threads.get_me().await {
        Ok(me) => {
            let username = me.username.unwrap_or_else(|| {
                tracing::warn!("Threads /me returned no username, using empty string");
                String::new()
            });
            info!("Threads owner: @{username}");
            username
        }
        Err(e) => {
            tracing::error!("Failed to fetch Threads username: {e}");
            String::new()
        }
    };

    let state = AppState {
        pool: pool.clone(),
        threads,
        mercury: Arc::new(MercuryClient::new(mercury_key, mercury_url)),
        api_key,
        owner_username,
        analysis_running: Arc::new(AtomicBool::new(false)),
        analysis_progress: Arc::new(AtomicU32::new(0)),
        analysis_total: Arc::new(AtomicU32::new(0)),
        sync_running: Arc::new(AtomicBool::new(false)),
        sync_message: Arc::new(tokio::sync::RwLock::new(String::new())),
        sync_progress: Arc::new(AtomicU32::new(0)),
        sync_total: Arc::new(AtomicU32::new(0)),
    };

    // Spawn background sync task (first run after 30s, then every 15 min)
    let bg_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;

        // On startup: backfill daily_views if empty
        info!("Checking daily_views backfill...");
        if let Err(e) = sync::sync_daily_views(&bg_state.pool, &bg_state.threads).await {
            tracing::error!("Startup daily views backfill failed: {e}");
        }

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

            // Task 1: Discover posts
            info!("Background sync starting");
            if let Err(e) = sync::sync_posts(&bg_state.pool, &bg_state.threads, None).await {
                tracing::error!("Background post discovery failed: {e}");
                continue;
            }
            // Task 2: Refresh per-post metrics
            if let Err(e) = sync::sync_post_metrics(&bg_state.pool, &bg_state.threads, None).await {
                tracing::error!("Background metrics refresh failed: {e}");
            }
            // Task 3: Sync replies for recent posts
            if let Err(e) = sync::sync_replies(&bg_state.pool, &bg_state.threads).await {
                tracing::error!("Background reply sync failed: {e}");
            }
            // Detect externally-replied replies
            if let Err(e) = sync::detect_external_replies(
                &bg_state.pool,
                &bg_state.threads,
                &bg_state.owner_username,
            )
            .await
            {
                tracing::error!("Background reply detection failed: {e}");
            }
            // Task 3: Refresh daily views (idempotent upsert, fetches last 7 days)
            if let Err(e) = sync::sync_daily_views(&bg_state.pool, &bg_state.threads).await {
                tracing::error!("Background daily views sync failed: {e}");
            }
            // Analysis + edge computation
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

    // Spawn nightly sync task at 2am — handles daily_views collection
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

            // Discover + refresh metrics
            if let Err(e) =
                sync::sync_posts(&nightly_state.pool, &nightly_state.threads, None).await
            {
                tracing::error!("Nightly post discovery failed: {e}");
            }
            if let Err(e) =
                sync::sync_post_metrics(&nightly_state.pool, &nightly_state.threads, None).await
            {
                tracing::error!("Nightly metrics refresh failed: {e}");
            }

            // Daily views collection (the primary reason for nightly sync)
            if let Err(e) =
                sync::sync_daily_views(&nightly_state.pool, &nightly_state.threads).await
            {
                tracing::error!("Nightly daily views sync failed: {e}");
            }

            // Analysis + edge computation
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
            // Generate insights report
            match insights::generate_report(&nightly_state.pool, &nightly_state.mercury, "nightly")
                .await
            {
                Ok(r) => info!("Nightly insights report generated: {}", r.id),
                Err(e) => tracing::error!("Nightly insights generation failed: {e}"),
            }
            // Generate emotion narrative
            match emotions::generate_narrative(
                &nightly_state.pool,
                &nightly_state.mercury,
                "nightly",
            )
            .await
            {
                Ok(n) => info!("Nightly emotion narrative generated: {}", n.id),
                Err(e) => tracing::error!("Nightly emotion narrative generation failed: {e}"),
            }
            info!("Nightly sync complete");
        }
    });

    // Spawn publish scheduler (checks every 60s for posts due to publish)
    let sched_state = state.clone();
    tokio::spawn(async move {
        // Startup recovery: reset stuck 'publishing' posts
        match compose::recover_stuck(&sched_state.pool).await {
            Ok(0) => {}
            Ok(n) => info!("Recovered {n} stuck publishing posts"),
            Err(e) => tracing::error!("Failed to recover stuck posts: {e}"),
        }

        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;

            let due = match compose::claim_due_posts(&sched_state.pool).await {
                Ok(posts) => posts,
                Err(e) => {
                    tracing::error!("Scheduler: failed to claim due posts: {e}");
                    continue;
                }
            };

            for post in due {
                info!("Publishing scheduled post {}", post.id);

                // Re-check status (race condition guard)
                let current = compose::get(&sched_state.pool, post.id).await;
                if let Ok(Some(p)) = &current {
                    if p.status != "publishing" {
                        info!(
                            "Post {} status changed to '{}', skipping",
                            post.id, p.status
                        );
                        continue;
                    }
                }

                let result: Result<String, crate::error::AppError> = async {
                    let container_id = sched_state.threads.create_container(&post.text).await?;
                    sched_state.threads.publish_container(&container_id).await
                }
                .await;

                match result {
                    Ok(threads_post_id) => {
                        info!("Published post {} as {threads_post_id}", post.id);
                        if let Err(e) =
                            compose::mark_published(&sched_state.pool, post.id, &threads_post_id)
                                .await
                        {
                            tracing::error!("Failed to mark post {} as published: {e}", post.id);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to publish post {}: {e}", post.id);
                        if let Err(e2) =
                            compose::mark_failed(&sched_state.pool, post.id, &e.to_string()).await
                        {
                            tracing::error!("Failed to mark post {} as failed: {e2}", post.id);
                        }
                    }
                }
            }
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
        .route("/api/posts/debug", get(routes::posts::get_debug_posts))
        .route("/api/posts/{id}", get(routes::posts::get_post))
        .route("/api/graph", get(routes::graph::get_graph))
        .route("/api/analytics", get(routes::analytics::get_analytics))
        .route("/api/analytics/views", get(routes::analytics::get_views))
        .route(
            "/api/analytics/views/range-sums",
            get(routes::analytics::get_views_range_sums),
        )
        .route(
            "/api/analytics/views/cumulative",
            get(routes::analytics::get_views_cumulative),
        )
        .route(
            "/api/analytics/views/per-post",
            get(routes::analytics::get_views_per_post),
        )
        .route(
            "/api/analytics/views/per-post/cumulative",
            get(routes::analytics::get_views_per_post_cumulative),
        )
        .route(
            "/api/analytics/views/per-post/range-sums",
            get(routes::analytics::get_views_per_post_range_sums),
        )
        .route(
            "/api/analytics/heatmap",
            get(routes::analytics::get_heatmap),
        )
        .route(
            "/api/analytics/heatmap/views",
            get(routes::analytics::get_views_heatmap),
        )
        .route(
            "/api/analytics/engagement",
            get(routes::analytics::get_engagement),
        )
        .route(
            "/api/analytics/engagement/daily-deltas",
            get(routes::analytics::get_engagement_daily_deltas),
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
        .route("/api/sync/reset", post(routes::sync::reset_database))
        .route("/api/reanalyze", post(routes::reanalyze::trigger_reanalyze))
        .route("/api/analyze", post(routes::analyze::start_analyze))
        .route("/api/analyze/status", get(routes::analyze::analyze_status))
        .route(
            "/api/subjects/{id}/posts",
            get(routes::subjects::get_subject_posts),
        )
        .route("/api/insights/latest", get(routes::insights::get_latest))
        .route("/api/insights/generate", post(routes::insights::generate))
        .route("/api/emotions/summary", get(routes::emotions::get_summary))
        .route(
            "/api/emotions/narrative",
            get(routes::emotions::get_narrative),
        )
        .route(
            "/api/emotions/narrative/generate",
            post(routes::emotions::generate_narrative),
        )
        .route("/api/emotions/backfill", post(routes::emotions::backfill))
        .route(
            "/api/compose",
            get(routes::compose::list_posts).post(routes::compose::create_post),
        )
        .route(
            "/api/compose/{id}",
            get(routes::compose::get_post)
                .put(routes::compose::update_post)
                .delete(routes::compose::delete_post),
        )
        .route(
            "/api/compose/{id}/publish",
            post(routes::compose::publish_now),
        )
        .route("/api/replies", get(routes::replies::list_replies))
        .route("/api/replies/count", get(routes::replies::count_unreplied))
        .route("/api/replies/{id}/reply", post(routes::replies::send_reply))
        .route(
            "/api/replies/{id}/dismiss",
            post(routes::replies::dismiss_reply),
        )
        .route("/api/replies/detect", post(routes::replies::detect_replies))
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
