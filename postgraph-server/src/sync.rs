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

// ── Task 1: Post Discovery ─────────────────────────────────────────

/// Discover posts from the Threads API and upsert them into the database.
/// Does NOT fetch metrics — only stores post metadata.
pub async fn sync_posts(
    pool: &PgPool,
    client: &ThreadsClient,
    progress: Option<(&Arc<AtomicU32>, &Arc<AtomicU32>)>,
) -> Result<u32, AppError> {
    let sync_state = db::get_sync_state(pool).await?;
    let mut cursor = sync_state.last_sync_cursor;
    let mut total_synced: u32 = 0;

    let existing_count = db::get_all_post_ids(pool).await?.len() as u32;
    if let Some((prog, tot)) = &progress {
        prog.store(0, Ordering::SeqCst);
        tot.store(existing_count, Ordering::SeqCst);
    }

    loop {
        let response = client.get_user_threads(cursor.as_deref()).await?;
        let post_count = response.data.len();

        for tp in &response.data {
            if tp.media_type.as_deref() == Some("REPOST_FACADE") {
                info!("Skipping repost {}", tp.id);
                continue;
            }

            let post = threads_post_to_post(tp);
            let is_new = db::upsert_post(pool, &post).await?;

            total_synced += 1;
            if let Some((prog, tot)) = &progress {
                prog.store(total_synced, Ordering::SeqCst);
                if is_new {
                    tot.fetch_add(1, Ordering::SeqCst);
                }
            }
        }

        info!("Discovered {} posts (batch of {})", total_synced, post_count);

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

// ── Task 2: Per-Post Metrics ────────────────────────────────────────

/// Refresh insights metrics for all posts. Writes API values directly (no GREATEST).
pub async fn sync_post_metrics(
    pool: &PgPool,
    client: &ThreadsClient,
    progress: Option<(&Arc<AtomicU32>, &Arc<AtomicU32>)>,
) -> Result<u32, AppError> {
    let post_ids = db::get_all_post_ids(pool).await?;
    let total = post_ids.len();
    info!("Refreshing metrics for {total} posts");

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
                    // Trust the API — write values directly, no GREATEST
                    sqlx::query(
                        "UPDATE posts SET views = $1, likes = $2, replies_count = $3, reposts = $4, quotes = $5, shares = $6, synced_at = NOW() WHERE id = $7",
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
                    warn!("Rate limited for {post_id}, waiting {secs}s (attempt {retries}/3)");
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                }
                Err(e) => {
                    warn!("Failed to refresh metrics for {post_id}: {e}");
                    break;
                }
            }
        }

        if let Some((prog, _)) = &progress {
            prog.store((i + 1) as u32, Ordering::SeqCst);
        }

        tokio::time::sleep(Duration::from_millis(200)).await;

        if (i + 1) % 25 == 0 {
            info!("Metrics refresh progress: {}/{total}", i + 1);
        }
    }

    info!("Metrics refresh complete: {updated}/{total} posts updated");
    Ok(updated)
}

// ── Task 3: Daily Views Collection ──────────────────────────────────

/// Fetch daily views from the user-level insights API and store in daily_views.
/// On first run (empty table), backfills up to 730 days.
/// On subsequent runs, fetches only the last 7 days.
pub async fn sync_daily_views(
    pool: &PgPool,
    client: &ThreadsClient,
) -> Result<u32, AppError> {
    let max_date = db::get_max_daily_views_date(pool).await?;

    let max_days = if max_date.is_some() {
        7 // Incremental: fetch last 7 days to catch late-arriving data
    } else {
        730 // Backfill: fetch up to 730 days
    };

    info!(
        "Syncing daily views (max_days={max_days}, last_date={:?})",
        max_date
    );

    let daily_data = client.get_user_insights(Some(max_days)).await?;
    let mut upserted: u32 = 0;

    for (date, views) in &daily_data {
        db::upsert_daily_views(pool, *date, *views).await?;
        upserted += 1;
    }

    info!("Daily views sync complete: {upserted} days upserted");
    Ok(upserted)
}

// ── Helpers ─────────────────────────────────────────────────────────

fn parse_threads_timestamp(ts: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = DateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%z") {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc());
    }
    warn!("Failed to parse Threads timestamp: {ts:?}");
    None
}

fn threads_post_to_post(tp: &ThreadsPost) -> Post {
    let timestamp = match tp.timestamp.as_deref() {
        Some(ts) => parse_threads_timestamp(ts).unwrap_or_else(|| {
            warn!("Post {} has unparseable timestamp {ts:?}, using now()", tp.id);
            Utc::now()
        }),
        None => {
            warn!("Post {} has no timestamp from Threads API, using now()", tp.id);
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
