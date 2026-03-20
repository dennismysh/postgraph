use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tracing::{info, warn};

use crate::db;
use crate::error::AppError;
use crate::threads::{ThreadsClient, ThreadsPost};
use crate::types::Post;

/// Refresh insights metrics for all existing posts in the database.
/// Returns the number of posts successfully updated.
/// When `progress` and `total_counter` are provided, updates them atomically for
/// real-time tracking by the status endpoint.
pub async fn refresh_all_metrics(
    pool: &PgPool,
    client: &ThreadsClient,
    progress: Option<(&Arc<AtomicU32>, &Arc<AtomicU32>)>,
) -> Result<u32, AppError> {
    let post_ids = db::get_all_post_ids(pool).await?;
    let total = post_ids.len();
    info!("Refreshing metrics for {total} existing posts");

    if let Some((prog, tot)) = &progress {
        tot.store(total as u32, Ordering::SeqCst);
        prog.store(0, Ordering::SeqCst);
    }

    let mut updated: u32 = 0;

    for (i, post_id) in post_ids.iter().enumerate() {
        let mut retries = 0u32;
        loop {
            match client.get_post_insights(post_id).await {
                Ok(insights) => {
                    sqlx::query(
                        "UPDATE posts SET views = GREATEST(views, $1), likes = GREATEST(likes, $2), replies_count = GREATEST(replies_count, $3), reposts = GREATEST(reposts, $4), quotes = GREATEST(quotes, $5), shares = GREATEST(shares, $6), synced_at = NOW() WHERE id = $7",
                    )
                    .bind(insights.views)
                    .bind(insights.likes)
                    .bind(insights.replies)
                    .bind(insights.reposts)
                    .bind(insights.quotes)
                    .bind(insights.shares)
                    .bind(post_id)
                    .execute(pool)
                    .await?;

                    db::insert_engagement_snapshot(
                        pool,
                        post_id,
                        insights.views,
                        insights.likes,
                        insights.replies,
                        insights.reposts,
                        insights.quotes,
                    )
                    .await?;

                    updated += 1;
                    break;
                }
                Err(AppError::RateLimited(secs)) => {
                    retries += 1;
                    if retries > 3 {
                        warn!("Rate limited too many times for {post_id}, skipping");
                        break;
                    }
                    warn!(
                        "Rate limited refreshing metrics for {post_id}, waiting {secs}s (attempt {retries}/3)"
                    );
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                }
                Err(e) => {
                    // Don't overwrite existing metrics — just skip this post
                    warn!("Failed to refresh metrics for {post_id}: {e}");
                    break;
                }
            }
        }

        if let Some((prog, _)) = &progress {
            prog.store((i + 1) as u32, Ordering::SeqCst);
        }

        // Throttle between API calls
        tokio::time::sleep(Duration::from_millis(200)).await;

        if (i + 1) % 25 == 0 {
            info!("Metrics refresh progress: {}/{total}", i + 1);
        }
    }

    info!("Metrics refresh complete: {updated}/{total} posts updated");
    Ok(updated)
}

fn parse_threads_timestamp(ts: &str) -> Option<DateTime<Utc>> {
    // Try RFC3339 first (e.g. "2026-02-19T12:34:56+00:00" or "...Z")
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        return Some(dt.with_timezone(&Utc));
    }
    // Threads API often returns "+0000" without colon, which isn't valid RFC3339
    if let Ok(dt) = DateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%z") {
        return Some(dt.with_timezone(&Utc));
    }
    // Naive datetime without timezone — assume UTC
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc());
    }
    warn!("Failed to parse Threads timestamp: {ts:?}");
    None
}

fn threads_post_to_post(tp: &ThreadsPost) -> Post {
    let timestamp = match tp.timestamp.as_deref() {
        Some(ts) => parse_threads_timestamp(ts).unwrap_or_else(|| {
            warn!(
                "Post {} has unparseable timestamp {ts:?}, using now()",
                tp.id
            );
            Utc::now()
        }),
        None => {
            warn!(
                "Post {} has no timestamp from Threads API, using now()",
                tp.id
            );
            Utc::now()
        }
    };

    Post {
        id: tp.id.clone(),
        text: tp.text.clone(),
        media_type: tp.media_type.clone(),
        media_url: tp.media_url.clone(),
        timestamp,
        permalink: tp.permalink.clone(),
        views: 0,
        likes: 0,
        replies_count: 0,
        reposts: 0,
        quotes: 0,
        shares: 0,
        intent_id: None,
        subject_id: None,
        sentiment: None,
        synced_at: Utc::now(),
        analyzed_at: None,
    }
}

/// When `progress` and `total_counter` are provided, updates them atomically for
/// real-time tracking. Since the Threads API is paginated, total is updated as
/// batches arrive.
pub async fn run_sync(
    pool: &PgPool,
    client: &ThreadsClient,
    progress: Option<(&Arc<AtomicU32>, &Arc<AtomicU32>)>,
) -> Result<u32, AppError> {
    let sync_state = db::get_sync_state(pool).await?;
    let mut cursor = sync_state.last_sync_cursor;
    let mut total_synced: u32 = 0;

    // Use existing DB post count as the known total; new posts increment it.
    let existing_count = db::get_all_post_ids(pool).await?.len() as u32;
    if let Some((prog, tot)) = &progress {
        prog.store(0, Ordering::SeqCst);
        tot.store(existing_count, Ordering::SeqCst);
    }

    loop {
        let response = client.get_user_threads(cursor.as_deref()).await?;
        let post_count = response.data.len();

        for tp in &response.data {
            // Skip reposts — these are other users' content we reposted
            if tp.media_type.as_deref() == Some("REPOST_FACADE") {
                info!("Skipping repost {}", tp.id);
                continue;
            }

            let post = threads_post_to_post(tp);
            let is_new = db::upsert_post(pool, &post).await?;

            // Fetch insights with throttling and retry on rate limit
            let mut retries = 0u32;
            loop {
                match client.get_post_insights(&tp.id).await {
                    Ok(insights) => {
                        sqlx::query(
                            "UPDATE posts SET views = GREATEST(views, $1), likes = GREATEST(likes, $2), replies_count = GREATEST(replies_count, $3), reposts = GREATEST(reposts, $4), quotes = GREATEST(quotes, $5), shares = GREATEST(shares, $6) WHERE id = $7",
                        )
                        .bind(insights.views)
                        .bind(insights.likes)
                        .bind(insights.replies)
                        .bind(insights.reposts)
                        .bind(insights.quotes)
                        .bind(insights.shares)
                        .bind(&tp.id)
                        .execute(pool)
                        .await?;

                        // Only create engagement snapshots for newly discovered posts.
                        // Existing posts get their snapshots via refresh_all_metrics().
                        if is_new {
                            db::insert_engagement_snapshot(
                                pool,
                                &tp.id,
                                insights.views,
                                insights.likes,
                                insights.replies,
                                insights.reposts,
                                insights.quotes,
                            )
                            .await?;
                        }
                        break;
                    }
                    Err(AppError::RateLimited(secs)) => {
                        retries += 1;
                        if retries > 3 {
                            warn!("Rate limited too many times for {}, skipping", tp.id);
                            break;
                        }
                        warn!(
                            "Rate limited fetching insights for {}, waiting {}s (attempt {}/3)",
                            tp.id, secs, retries
                        );
                        tokio::time::sleep(Duration::from_secs(secs)).await;
                    }
                    Err(e) => {
                        // Don't overwrite existing metrics — leave them as-is
                        warn!("Failed to fetch insights for {}: {}", tp.id, e);
                        break;
                    }
                }
            }

            // Throttle between insight calls
            tokio::time::sleep(Duration::from_millis(200)).await;

            total_synced += 1;
            if let Some((prog, tot)) = &progress {
                prog.store(total_synced, Ordering::SeqCst);
                if is_new {
                    tot.fetch_add(1, Ordering::SeqCst);
                }
            }
        }

        info!("Synced {} posts (batch of {})", total_synced, post_count);

        // Update cursor — extract next cursor and whether there's a next page
        // before consuming `response.paging`
        let next_cursor = response
            .paging
            .as_ref()
            .and_then(|p| p.cursors.as_ref())
            .and_then(|c| c.after.clone());

        let has_next = response
            .paging
            .as_ref()
            .and_then(|p| p.next.as_ref())
            .is_some();

        db::update_sync_state(pool, next_cursor.as_deref()).await?;

        if !has_next {
            break;
        }

        cursor = next_cursor;
    }

    Ok(total_synced)
}
