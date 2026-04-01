# Data Integrity Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace fabricated analytics calculations with authoritative API data sources — every number traceable to one source, no workarounds.

**Architecture:** New `daily_views` table stores user-level insights time-series. Sync pipeline split into three independent tasks (post discovery, per-post metrics, daily views). Analytics endpoints rewritten to query one table each. ~300 lines of workaround SQL deleted.

**Tech Stack:** Rust (axum, sqlx, tokio, chrono, chrono-tz), PostgreSQL, Svelte/SvelteKit, Chart.js

**Spec:** `docs/superpowers/specs/2026-03-31-data-integrity-redesign.md`

---

## File Map

### Backend — Create
- `postgraph-server/migrations/011_daily_views.sql` — New table + drop user_insights

### Backend — Modify
- `postgraph-server/src/threads.rs` — Add `end_time` to InsightValue, rewrite `get_user_insights` return type
- `postgraph-server/src/sync.rs` — Split into `sync_posts`, `sync_post_metrics`, `sync_daily_views`; remove GREATEST
- `postgraph-server/src/db.rs` — Add `upsert_daily_views`, `get_max_daily_views_date`; remove `get_user_insights_total`, `update_user_insights`
- `postgraph-server/src/routes/analytics.rs` — Rewrite all views endpoints to use `daily_views`; fix engagement attribution; add cumulative + heatmap/views endpoints; delete spreading CTEs, triple-max, debug endpoint
- `postgraph-server/src/routes/sync.rs` — Update trigger_sync and reset_database for new sync functions
- `postgraph-server/src/routes/mod.rs` — No change (same module set)
- `postgraph-server/src/main.rs` — Restructure scheduling: 15-min sync_posts+sync_post_metrics, daily sync_daily_views, startup backfill
- `postgraph-server/src/types.rs` — No change
- `postgraph-server/tests/views_accuracy.rs` — Rewrite to test daily_views-based queries

### Frontend — Create
- `web/src/routes/api/analytics/views/cumulative/+server.ts` — Proxy for cumulative views endpoint
- `web/src/routes/api/analytics/heatmap/views/+server.ts` — Proxy for views heatmap endpoint

### Frontend — Modify
- `web/src/lib/api.ts` — Add `getViewsCumulative`, `getViewsHeatmap` types/functions
- `web/src/lib/components/Dashboard.svelte` — Rewrite views chart, add cumulative chart, split heatmaps, simplify range buttons

### Frontend — Delete
- `web/src/routes/api/analytics/views/debug/+server.ts` — Remove debug proxy

---

## Task 1: Database Migration

**Files:**
- Create: `postgraph-server/migrations/011_daily_views.sql`

- [ ] **Step 1: Write migration**

```sql
-- Create authoritative daily views table (user-level insights API)
CREATE TABLE daily_views (
    date DATE PRIMARY KEY,
    views BIGINT NOT NULL,
    source TEXT NOT NULL DEFAULT 'user_insights',
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Drop the single-row user_insights table (replaced by daily_views)
DROP TABLE IF EXISTS user_insights;
```

Write this to `postgraph-server/migrations/011_daily_views.sql`.

- [ ] **Step 2: Verify migration compiles**

Run: `cd postgraph-server && cargo check`
Expected: PASS (migrations are loaded by `sqlx::migrate!()` at runtime, but check that the project still compiles)

- [ ] **Step 3: Commit**

```
git add postgraph-server/migrations/011_daily_views.sql
git commit -m "feat: add daily_views table and drop user_insights"
```

---

## Task 2: Threads API Client — Parse `end_time`

**Files:**
- Modify: `postgraph-server/src/threads.rs:48-51` (InsightValue struct)
- Modify: `postgraph-server/src/threads.rs:76-87` (UserInsights/UserDailyViews structs)
- Modify: `postgraph-server/src/threads.rs:230-300` (get_user_insights method)

- [ ] **Step 1: Add `end_time` to InsightValue**

In `postgraph-server/src/threads.rs`, modify the `InsightValue` struct (line 48):

```rust
#[derive(Debug, Deserialize)]
pub struct InsightValue {
    pub value: Option<serde_json::Value>,
    pub end_time: Option<String>,
}
```

- [ ] **Step 2: Replace UserInsights/UserDailyViews with simple return type**

Remove the `UserDailyViews` and `UserInsights` structs (lines 76-87). They are no longer needed — `get_user_insights` will return `Vec<(chrono::NaiveDate, i64)>` directly.

- [ ] **Step 3: Rewrite `get_user_insights` to return dated pairs**

Replace the `get_user_insights` method (lines 230-300) with:

```rust
/// Fetch user-level daily views from the Threads API.
/// Returns (date, views) pairs parsed from the API's end_time field.
/// Walks backwards in 90-day windows up to `max_days` (default 730).
pub async fn get_user_insights(
    &self,
    max_days: Option<u32>,
) -> Result<Vec<(chrono::NaiveDate, i64)>, AppError> {
    let max_days = max_days.unwrap_or(730) as i64;
    let mut result: Vec<(chrono::NaiveDate, i64)> = Vec::new();
    let now = Utc::now();
    let earliest = now - chrono::Duration::days(max_days);

    let mut window_end = now;
    while window_end > earliest {
        let window_start = (window_end - chrono::Duration::days(89)).max(earliest);
        let since = window_start.timestamp();
        let until = window_end.timestamp();

        let url = format!(
            "{}/me/threads_insights?metric=views&since={}&until={}&access_token={}",
            BASE_URL, since, until,
            self.token().await
        );

        let resp = self.client.get(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("User insights request failed: {body}");
            break;
        }

        let data: InsightsResponse = resp.json().await?;
        for item in &data.data {
            if item.name != "views" {
                continue;
            }
            if let Some(values) = &item.values {
                for v in values {
                    let count = v.value.as_ref().and_then(|val| val.as_i64()).unwrap_or(0);
                    if count == 0 {
                        continue;
                    }
                    // Parse end_time to get the date this value covers
                    if let Some(ref end_time) = v.end_time {
                        if let Some(date) = parse_end_time(end_time) {
                            result.push((date, count));
                        }
                    }
                }
            }
        }

        window_end = window_start - chrono::Duration::days(1);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    Ok(result)
}
```

- [ ] **Step 4: Add `parse_end_time` helper**

Add this function near the top of `threads.rs` (after the imports):

```rust
/// Parse Threads API end_time string (e.g. "2024-07-12T08:00:00+0000") into a NaiveDate.
/// The end_time marks the end of the day period, so we subtract one day to get the actual date.
fn parse_end_time(s: &str) -> Option<chrono::NaiveDate> {
    // Try standard RFC3339
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some((dt - chrono::Duration::days(1)).date_naive());
    }
    // Threads uses "+0000" without colon
    if let Ok(dt) = chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z") {
        return Some((dt - chrono::Duration::days(1)).date_naive());
    }
    tracing::warn!("Failed to parse end_time: {s:?}");
    None
}
```

Note: The Threads API `end_time` marks the *end* of the period (e.g. `2024-07-12T08:00:00+0000` means the day ending at that time, which is July 11). We subtract one day to get the actual date.

- [ ] **Step 5: Compile check**

Run: `cargo check --workspace`
Expected: Errors in `sync.rs` (it still references `UserInsights`). That's expected — we fix it in Task 3.

- [ ] **Step 6: Commit**

```
git add postgraph-server/src/threads.rs
git commit -m "refactor: parse end_time from user insights API, return dated pairs"
```

---

## Task 3: Database Functions — Add daily_views helpers, remove user_insights helpers

**Files:**
- Modify: `postgraph-server/src/db.rs:476-492` (remove get_user_insights_total, update_user_insights)
- Modify: `postgraph-server/src/db.rs` (add new functions)

- [ ] **Step 1: Remove `get_user_insights_total` and `update_user_insights`**

Delete lines 476-492 from `postgraph-server/src/db.rs` (the two functions that reference the `user_insights` table).

- [ ] **Step 2: Add `upsert_daily_views` function**

Add to `postgraph-server/src/db.rs`:

```rust
/// Upsert a daily views entry. If the date already exists, update the views count.
pub async fn upsert_daily_views(
    pool: &PgPool,
    date: chrono::NaiveDate,
    views: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"INSERT INTO daily_views (date, views, fetched_at)
           VALUES ($1, $2, NOW())
           ON CONFLICT (date) DO UPDATE SET views = $2, fetched_at = NOW()"#,
    )
    .bind(date)
    .bind(views)
    .execute(pool)
    .await?;
    Ok(())
}
```

- [ ] **Step 3: Add `get_max_daily_views_date` function**

Add to `postgraph-server/src/db.rs`:

```rust
/// Get the most recent date in daily_views, or None if table is empty.
pub async fn get_max_daily_views_date(pool: &PgPool) -> sqlx::Result<Option<chrono::NaiveDate>> {
    let row: (Option<chrono::NaiveDate>,) =
        sqlx::query_as("SELECT MAX(date) FROM daily_views")
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}
```

- [ ] **Step 4: Add `get_daily_views_total` function**

Add to `postgraph-server/src/db.rs`:

```rust
/// Get the total views from daily_views table.
pub async fn get_daily_views_total(pool: &PgPool) -> sqlx::Result<i64> {
    let (total,): (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(views), 0)::bigint FROM daily_views")
            .fetch_one(pool)
            .await?;
    Ok(total)
}
```

- [ ] **Step 5: Compile check**

Run: `cargo check --workspace`
Expected: Errors in `sync.rs` and `analytics.rs` (they still reference removed functions). Expected — fixed in Tasks 4 and 5.

- [ ] **Step 6: Commit**

```
git add postgraph-server/src/db.rs
git commit -m "refactor: add daily_views db helpers, remove user_insights helpers"
```

---

## Task 4: Sync Pipeline — Split into Three Tasks

**Files:**
- Modify: `postgraph-server/src/sync.rs` (full rewrite)

- [ ] **Step 1: Rewrite sync.rs with three public functions**

Replace the entire contents of `postgraph-server/src/sync.rs` with:

```rust
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
/// Returns the number of posts processed.
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
/// Returns the number of posts successfully updated.
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
/// On subsequent runs, fetches only since the most recent stored date.
pub async fn sync_daily_views(
    pool: &PgPool,
    client: &ThreadsClient,
) -> Result<u32, AppError> {
    let max_date = db::get_max_daily_views_date(pool).await?;

    let max_days = if max_date.is_some() {
        // Incremental: fetch last 7 days to catch any late-arriving data
        7
    } else {
        // Backfill: fetch up to 730 days
        730
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
```

- [ ] **Step 2: Compile check**

Run: `cargo check --workspace`
Expected: Errors in `main.rs` and `routes/sync.rs` (they reference old `run_sync` and `refresh_all_metrics`). Expected — fixed in Tasks 5 and 7.

- [ ] **Step 3: Commit**

```
git add postgraph-server/src/sync.rs
git commit -m "refactor: split sync into three independent tasks, remove GREATEST"
```

---

## Task 5: Route Handler — Update sync trigger and reset

**Files:**
- Modify: `postgraph-server/src/routes/sync.rs`

- [ ] **Step 1: Update trigger_sync to call new sync functions**

In `postgraph-server/src/routes/sync.rs`, replace the import (line 8):

```rust
use crate::sync;
```

Replace the spawned task body inside `trigger_sync` (lines 42-81) with:

```rust
    let bg = state.clone();
    tokio::spawn(async move {
        let mut status_parts: Vec<String> = Vec::new();
        let progress = Some((&bg.sync_progress, &bg.sync_total));

        // Phase 1: discover posts
        {
            *bg.sync_message.write().await = "Discovering posts from Threads...".to_string();
        }
        match sync::sync_posts(&bg.pool, &bg.threads, progress).await {
            Ok(n) => {
                status_parts.push(format!("{n} discovered"));
                info!("Post discovery complete: {n} posts");
            }
            Err(e) => {
                tracing::error!("Post discovery failed: {e}");
                *bg.sync_message.write().await = format!("Sync failed: {e}");
                bg.sync_running.store(false, Ordering::SeqCst);
                return;
            }
        }

        // Phase 2: refresh per-post metrics
        {
            *bg.sync_message.write().await = "Refreshing per-post metrics...".to_string();
        }
        match sync::sync_post_metrics(&bg.pool, &bg.threads, progress).await {
            Ok(n) => {
                status_parts.push(format!("{n} metrics refreshed"));
                info!("Metrics refresh complete: {n} posts");
            }
            Err(e) => {
                tracing::error!("Metrics refresh failed: {e}");
                status_parts.push(format!("metrics failed: {e}"));
            }
        }

        // Phase 3: sync daily views
        {
            *bg.sync_message.write().await = "Syncing daily views...".to_string();
        }
        match sync::sync_daily_views(&bg.pool, &bg.threads).await {
            Ok(n) => {
                status_parts.push(format!("{n} days synced"));
                info!("Daily views sync complete: {n} days");
            }
            Err(e) => {
                tracing::error!("Daily views sync failed: {e}");
                status_parts.push(format!("daily views failed: {e}"));
            }
        }

        let done_msg = format!("Done! {}", status_parts.join(", "));
        *bg.sync_message.write().await = done_msg;
        bg.sync_running.store(false, Ordering::SeqCst);
    });
```

- [ ] **Step 2: Update reset_database to include daily_views**

In the `reset_database` function, replace the `user_insights` reset (lines 143-147) with:

```rust
    // Clear daily views
    sqlx::query("TRUNCATE daily_views")
        .execute(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
```

- [ ] **Step 3: Compile check**

Run: `cargo check --workspace`
Expected: Errors in `main.rs` only (still references old sync functions). Expected — fixed in Task 7.

- [ ] **Step 4: Commit**

```
git add postgraph-server/src/routes/sync.rs
git commit -m "refactor: update sync trigger for three-task pipeline"
```

---

## Task 6: Analytics Endpoints — Rewrite

**Files:**
- Modify: `postgraph-server/src/routes/analytics.rs` (major rewrite)
- Modify: `postgraph-server/src/main.rs:227-270` (route registration)

This is the largest task. Replace the entire `analytics.rs` file.

- [ ] **Step 1: Rewrite analytics.rs**

Replace the entire contents of `postgraph-server/src/routes/analytics.rs` with:

```rust
use crate::db;
use crate::state::AppState;
use axum::{Json, extract::Path, extract::Query, extract::State};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Types ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct AnalyticsData {
    pub total_posts: usize,
    pub analyzed_posts: usize,
    pub total_subjects: usize,
    pub total_intents: usize,
    pub total_views: i64,
    pub subjects: Vec<SubjectSummary>,
    pub engagement_over_time: Vec<EngagementPoint>,
}

#[derive(Serialize)]
pub struct SubjectSummary {
    pub name: String,
    pub post_count: i64,
    pub avg_engagement: f64,
}

#[derive(Serialize)]
pub struct EngagementPoint {
    pub date: String,
    pub likes: i64,
    pub replies: i64,
    pub reposts: i64,
}

#[derive(Serialize)]
pub struct ViewsPoint {
    pub date: String,
    pub views: i64,
}

#[derive(Serialize)]
pub struct CumulativeViewsPoint {
    pub date: String,
    pub cumulative_views: i64,
}

#[derive(Deserialize)]
pub struct ViewsQuery {
    pub since: Option<String>,
    pub grouping: Option<String>,
}

#[derive(Serialize)]
pub struct ViewsRangeSums {
    pub sums: HashMap<String, i64>,
}

#[derive(Serialize)]
pub struct PostEngagementPoint {
    pub date: String,
    pub views: i32,
    pub likes: i32,
    pub replies: i32,
    pub reposts: i32,
    pub quotes: i32,
}

#[derive(Deserialize)]
pub struct HeatmapQuery {
    pub range: Option<String>,
}

#[derive(Serialize)]
pub struct HeatmapDay {
    pub date: String,
    pub posts: i64,
    pub likes: i64,
    pub replies: i64,
    pub reposts: i64,
    pub views: i64,
    pub media_types: HashMap<String, i64>,
}

#[derive(Serialize)]
pub struct HeatmapResponse {
    pub days: Vec<HeatmapDay>,
}

#[derive(Serialize)]
pub struct HistogramBucket {
    pub bucket_min: i64,
    pub bucket_max: i64,
    pub label: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct HistogramResponse {
    pub engagement: Vec<HistogramBucket>,
    pub views: Vec<HistogramBucket>,
}

#[derive(Deserialize)]
pub struct HistogramQuery {
    pub since: Option<String>,
}

// ── Chart A: Daily Reach (from daily_views) ─────────────────────────

pub async fn get_views(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<ViewsPoint>>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.date_naive());

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT date::text, views
           FROM daily_views
           WHERE ($1::date IS NULL OR date >= $1)
           ORDER BY date"#,
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let points: Vec<ViewsPoint> = rows
        .into_iter()
        .map(|(date, views)| ViewsPoint { date, views })
        .collect();

    Ok(Json(points))
}

// ── Chart C: Growth Trajectory (cumulative daily_views) ─────────────

pub async fn get_views_cumulative(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<CumulativeViewsPoint>>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.date_naive());

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT date::text,
                  SUM(views) OVER (ORDER BY date)::bigint AS cumulative_views
           FROM daily_views
           WHERE ($1::date IS NULL OR date >= $1)
           ORDER BY date"#,
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let points: Vec<CumulativeViewsPoint> = rows
        .into_iter()
        .map(|(date, cumulative_views)| CumulativeViewsPoint {
            date,
            cumulative_views,
        })
        .collect();

    Ok(Json(points))
}

// ── Engagement Over Time (capture-time attribution) ─────────────────

pub async fn get_engagement(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<EngagementPoint>>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let is_hourly = query.grouping.as_deref() == Some("hourly");
    let (date_expr, date_format) = if is_hourly {
        (
            "DATE_TRUNC('hour', captured_at)",
            "TO_CHAR(DATE_TRUNC('hour', captured_at), 'YYYY-MM-DD HH24:00')",
        )
    } else {
        ("DATE(captured_at)", "DATE(captured_at)::text")
    };

    // Attribution: always use captured_at (when we observed the delta)
    let sql = format!(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.likes,
                      es.replies_count,
                      es.reposts,
                      MAX(es.likes) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_likes,
                      MAX(es.replies_count) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_replies,
                      MAX(es.reposts) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_reposts
               FROM engagement_snapshots es
           ),
           with_deltas AS (
               SELECT captured_at,
                      GREATEST(likes - COALESCE(prev_likes, 0), 0) AS like_delta,
                      GREATEST(replies_count - COALESCE(prev_replies, 0), 0) AS reply_delta,
                      GREATEST(reposts - COALESCE(prev_reposts, 0), 0) AS repost_delta
               FROM ordered_snapshots
           )
           SELECT {date_format} AS date,
                  SUM(like_delta)::bigint,
                  SUM(reply_delta)::bigint,
                  SUM(repost_delta)::bigint
           FROM with_deltas
           WHERE ($1::timestamptz IS NULL OR captured_at >= $1)
           GROUP BY {date_expr}
           ORDER BY date"#,
    );

    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(&sql)
        .bind(since)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let points: Vec<EngagementPoint> = rows
        .into_iter()
        .map(|(date, likes, replies, reposts)| EngagementPoint {
            date,
            likes,
            replies,
            reposts,
        })
        .collect();

    Ok(Json(points))
}

// ── Range Sums (single pass over daily_views) ───────────────────────

pub async fn get_views_range_sums(
    State(state): State<AppState>,
) -> Result<Json<ViewsRangeSums>, axum::http::StatusCode> {
    let now = chrono::Utc::now().date_naive();
    let b365 = now - chrono::Duration::days(365);
    let b270 = now - chrono::Duration::days(270);
    let b180 = now - chrono::Duration::days(180);
    let b90 = now - chrono::Duration::days(90);
    let b60 = now - chrono::Duration::days(60);
    let b30 = now - chrono::Duration::days(30);
    let b14 = now - chrono::Duration::days(14);
    let b7 = now - chrono::Duration::days(7);
    let b1 = now - chrono::Duration::days(1);

    let row: (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT
               COALESCE(SUM(views), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $1 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $2 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $3 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $4 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $5 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $6 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $7 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $8 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $9 THEN views END), 0)::bigint
           FROM daily_views"#,
    )
    .bind(b365)
    .bind(b270)
    .bind(b180)
    .bind(b90)
    .bind(b60)
    .bind(b30)
    .bind(b14)
    .bind(b7)
    .bind(b1)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut sums = HashMap::new();
    sums.insert("all".to_string(), row.0);
    sums.insert("365d".to_string(), row.1);
    sums.insert("270d".to_string(), row.2);
    sums.insert("180d".to_string(), row.3);
    sums.insert("90d".to_string(), row.4);
    sums.insert("60d".to_string(), row.5);
    sums.insert("30d".to_string(), row.6);
    sums.insert("14d".to_string(), row.7);
    sums.insert("7d".to_string(), row.8);
    sums.insert("24h".to_string(), row.9);

    Ok(Json(ViewsRangeSums { sums }))
}

// ── Analytics Summary ───────────────────────────────────────────────

pub async fn get_analytics(
    State(state): State<AppState>,
) -> Result<Json<AnalyticsData>, axum::http::StatusCode> {
    let posts = db::get_all_posts(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let subjects = db::get_all_subjects(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let intents = db::get_all_intents(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let subject_summaries: Vec<SubjectSummary> = sqlx::query_as::<_, (String, i64, f64)>(
        r#"SELECT s.name, COUNT(p.id)::bigint AS post_count,
           COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes), 0)::float8 AS avg_engagement
           FROM subjects s
           LEFT JOIN posts p ON p.subject_id = s.id AND p.analyzed_at IS NOT NULL
           GROUP BY s.name
           ORDER BY post_count DESC"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(|(name, count, avg)| SubjectSummary {
        name,
        post_count: count,
        avg_engagement: avg,
    })
    .collect();

    // Engagement over time: capture-time attribution, no backdating
    let engagement_over_time: Vec<EngagementPoint> = sqlx::query_as::<_, (String, i64, i64, i64)>(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.likes,
                      es.replies_count,
                      es.reposts,
                      MAX(es.likes) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_likes,
                      MAX(es.replies_count) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_replies,
                      MAX(es.reposts) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_reposts
               FROM engagement_snapshots es
           ),
           with_deltas AS (
               SELECT captured_at,
                      GREATEST(likes - COALESCE(prev_likes, 0), 0) AS like_delta,
                      GREATEST(replies_count - COALESCE(prev_replies, 0), 0) AS reply_delta,
                      GREATEST(reposts - COALESCE(prev_reposts, 0), 0) AS repost_delta
               FROM ordered_snapshots
           )
           SELECT DATE(captured_at)::text AS date,
                  SUM(like_delta)::bigint,
                  SUM(reply_delta)::bigint,
                  SUM(repost_delta)::bigint
           FROM with_deltas
           GROUP BY DATE(captured_at)
           ORDER BY date"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(|(date, likes, replies, reposts)| EngagementPoint {
        date,
        likes,
        replies,
        reposts,
    })
    .collect();

    let analyzed_count = posts.iter().filter(|p| p.analyzed_at.is_some()).count();

    // Total views from the authoritative source: daily_views
    let total_views = db::get_daily_views_total(&state.pool)
        .await
        .unwrap_or(0);

    Ok(Json(AnalyticsData {
        total_posts: posts.len(),
        analyzed_posts: analyzed_count,
        total_subjects: subjects.len(),
        total_intents: intents.len(),
        total_views,
        subjects: subject_summaries,
        engagement_over_time,
    }))
}

// ── Per-Post Engagement (raw snapshots, unchanged) ──────────────────

pub async fn get_post_engagement(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<Vec<PostEngagementPoint>>, axum::http::StatusCode> {
    let snapshots = db::get_engagement_history(&state.pool, &post_id)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let points: Vec<PostEngagementPoint> = snapshots
        .into_iter()
        .map(|s| PostEngagementPoint {
            date: s.captured_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            views: s.views,
            likes: s.likes,
            replies: s.replies_count,
            reposts: s.reposts,
            quotes: s.quotes,
        })
        .collect();

    Ok(Json(points))
}

// ── Heatmap A: Daily Reach (from daily_views) ───────────────────────

pub async fn get_views_heatmap(
    State(state): State<AppState>,
    Query(query): Query<HeatmapQuery>,
) -> Result<Json<HeatmapResponse>, axum::http::StatusCode> {
    let since = match query.range.as_deref() {
        Some("3m") => chrono::Utc::now().date_naive() - chrono::Duration::days(90),
        Some("6m") => chrono::Utc::now().date_naive() - chrono::Duration::days(180),
        Some("all") => chrono::NaiveDate::MIN,
        _ => chrono::Utc::now().date_naive() - chrono::Duration::days(365),
    };

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT date::text, views FROM daily_views WHERE date >= $1 ORDER BY date"#,
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let days: Vec<HeatmapDay> = rows
        .into_iter()
        .map(|(date, views)| HeatmapDay {
            date,
            posts: 0,
            likes: 0,
            replies: 0,
            reposts: 0,
            views,
            media_types: HashMap::new(),
        })
        .collect();

    Ok(Json(HeatmapResponse { days }))
}

// ── Heatmap B: Posting Activity (by publish date, no views) ─────────

pub async fn get_heatmap(
    State(state): State<AppState>,
    Query(query): Query<HeatmapQuery>,
) -> Result<Json<HeatmapResponse>, axum::http::StatusCode> {
    let since = match query.range.as_deref() {
        Some("3m") => chrono::Utc::now() - chrono::Duration::days(90),
        Some("6m") => chrono::Utc::now() - chrono::Duration::days(180),
        Some("all") => chrono::DateTime::<chrono::Utc>::MIN_UTC,
        _ => chrono::Utc::now() - chrono::Duration::days(365),
    };

    let rows: Vec<(String, i64, i64, i64, i64, Option<String>)> = sqlx::query_as(
        r#"SELECT DATE(timestamp)::text AS date,
                  COUNT(*) AS posts,
                  SUM(likes)::bigint AS likes,
                  SUM(replies_count)::bigint AS replies,
                  SUM(reposts)::bigint AS reposts,
                  media_type
           FROM posts
           WHERE timestamp >= $1
           GROUP BY DATE(timestamp), media_type
           ORDER BY date"#,
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut day_map: std::collections::BTreeMap<String, HeatmapDay> =
        std::collections::BTreeMap::new();
    for (date, posts, likes, replies, reposts, media_type) in rows {
        let entry = day_map.entry(date.clone()).or_insert_with(|| HeatmapDay {
            date,
            posts: 0,
            likes: 0,
            replies: 0,
            reposts: 0,
            views: 0,
            media_types: HashMap::new(),
        });
        entry.posts += posts;
        entry.likes += likes;
        entry.replies += replies;
        entry.reposts += reposts;
        if let Some(mt) = media_type {
            *entry.media_types.entry(mt).or_insert(0) += posts;
        }
    }

    Ok(Json(HeatmapResponse {
        days: day_map.into_values().collect(),
    }))
}

// ── Histograms (unchanged — already correct) ────────────────────────

pub async fn get_histograms(
    State(state): State<AppState>,
    Query(query): Query<HistogramQuery>,
) -> Result<Json<HistogramResponse>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let engagement_sql = r#"
        WITH buckets(bucket_min, bucket_max, label, ord) AS (
            VALUES
                (0, 0, '0', 1),
                (1, 5, '1-5', 2),
                (6, 10, '6-10', 3),
                (11, 25, '11-25', 4),
                (26, 50, '26-50', 5),
                (51, 100, '51-100', 6),
                (101, 250, '101-250', 7),
                (251, 500, '251-500', 8),
                (501, 1000, '501-1k', 9),
                (1001, 2147483647, '1k+', 10)
        ),
        post_engagement AS (
            SELECT (likes + replies_count + reposts + quotes) AS total
            FROM posts
            WHERE ($1::timestamptz IS NULL OR timestamp >= $1)
        )
        SELECT b.bucket_min::bigint, b.bucket_max::bigint, b.label,
               COUNT(p.total)::bigint AS count
        FROM buckets b
        LEFT JOIN post_engagement p ON p.total >= b.bucket_min AND p.total <= b.bucket_max
        GROUP BY b.bucket_min, b.bucket_max, b.label, b.ord
        ORDER BY b.ord
    "#;

    let engagement_rows: Vec<(i64, i64, String, i64)> = sqlx::query_as(engagement_sql)
        .bind(since)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let views_sql = r#"
        WITH buckets(bucket_min, bucket_max, label, ord) AS (
            VALUES
                (0, 0, '0', 1),
                (1, 100, '1-100', 2),
                (101, 500, '101-500', 3),
                (501, 1000, '501-1k', 4),
                (1001, 5000, '1k-5k', 5),
                (5001, 10000, '5k-10k', 6),
                (10001, 50000, '10k-50k', 7),
                (50001, 100000, '50k-100k', 8),
                (100001, 2147483647, '100k+', 9)
        )
        SELECT b.bucket_min::bigint, b.bucket_max::bigint, b.label,
               COUNT(p.id)::bigint AS count
        FROM buckets b
        LEFT JOIN posts p ON p.views >= b.bucket_min AND p.views <= b.bucket_max
            AND ($1::timestamptz IS NULL OR p.timestamp >= $1)
        GROUP BY b.bucket_min, b.bucket_max, b.label, b.ord
        ORDER BY b.ord
    "#;

    let views_rows: Vec<(i64, i64, String, i64)> = sqlx::query_as(views_sql)
        .bind(since)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let engagement = engagement_rows
        .into_iter()
        .map(|(bucket_min, bucket_max, label, count)| HistogramBucket {
            bucket_min,
            bucket_max,
            label,
            count,
        })
        .collect();

    let views = views_rows
        .into_iter()
        .map(|(bucket_min, bucket_max, label, count)| HistogramBucket {
            bucket_min,
            bucket_max,
            label,
            count,
        })
        .collect();

    Ok(Json(HistogramResponse { engagement, views }))
}
```

- [ ] **Step 2: Update route registration in main.rs**

In `postgraph-server/src/main.rs`, update the route registration block (around lines 227-270). Add the new routes and remove the debug route:

- Add: `.route("/api/analytics/views/cumulative", get(routes::analytics::get_views_cumulative))`
- Add: `.route("/api/analytics/heatmap/views", get(routes::analytics::get_views_heatmap))`
- Remove: `.route("/api/analytics/views/debug", get(routes::analytics::get_views_debug))`

- [ ] **Step 3: Compile check**

Run: `cargo check --workspace`
Expected: Errors only in `main.rs` sync scheduling (still references old functions). Fixed in Task 7.

- [ ] **Step 4: Commit**

```
git add postgraph-server/src/routes/analytics.rs postgraph-server/src/main.rs
git commit -m "refactor: rewrite analytics endpoints, single data source per chart"
```

---

## Task 7: Main.rs — Restructure Scheduling

**Files:**
- Modify: `postgraph-server/src/main.rs:89-214` (both spawned tasks)

- [ ] **Step 1: Rewrite background sync task (lines 89-160)**

Replace the background sync spawned task with:

```rust
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
            if let Err(e) =
                sync::sync_post_metrics(&bg_state.pool, &bg_state.threads, None).await
            {
                tracing::error!("Background metrics refresh failed: {e}");
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
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
            if let Err(e) = graph::compute_subject_edges(&bg_state.pool).await {
                tracing::error!("Background edge computation failed: {e}");
            }
        }
    });
```

- [ ] **Step 2: Rewrite nightly sync task (lines 162-214)**

Replace the nightly sync task with:

```rust
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
            info!("Nightly sync complete");
        }
    });
```

- [ ] **Step 3: Full compile check**

Run: `cargo check --workspace`
Expected: PASS — all backend references should now resolve.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --workspace --all-targets`
Expected: No errors. Fix any warnings.

- [ ] **Step 5: Commit**

```
git add postgraph-server/src/main.rs
git commit -m "refactor: restructure scheduling for three-task sync pipeline"
```

---

## Task 8: Frontend — API Types and Proxy Routes

**Files:**
- Modify: `web/src/lib/api.ts`
- Create: `web/src/routes/api/analytics/views/cumulative/+server.ts`
- Create: `web/src/routes/api/analytics/heatmap/views/+server.ts`
- Delete: `web/src/routes/api/analytics/views/debug/+server.ts`

- [ ] **Step 1: Read an existing proxy route for the pattern**

Read `web/src/routes/api/analytics/views/range-sums/+server.ts` to see the proxy pattern.

- [ ] **Step 2: Add cumulative views type and function to api.ts**

Add to `web/src/lib/api.ts` after the `ViewsPoint` type:

```typescript
export interface CumulativeViewsPoint {
    date: string;
    cumulative_views: number;
}
```

Add to the api object:

```typescript
getViewsCumulative: (since?: string): Promise<CumulativeViewsPoint[]> => {
    const params = new URLSearchParams();
    if (since) params.set('since', since);
    const qs = params.toString();
    return fetchApi(`/api/analytics/views/cumulative${qs ? `?${qs}` : ''}`);
},
getViewsHeatmap: (range?: string): Promise<HeatmapResponse> => {
    const params = new URLSearchParams();
    if (range) params.set('range', range);
    const qs = params.toString();
    return fetchApi(`/api/analytics/heatmap/views${qs ? `?${qs}` : ''}`);
},
```

- [ ] **Step 3: Create cumulative views proxy**

Create `web/src/routes/api/analytics/views/cumulative/+server.ts` using the same pattern as the range-sums proxy, but hitting `/api/analytics/views/cumulative` on the backend. Forward the `since` query param.

- [ ] **Step 4: Create views heatmap proxy**

Create `web/src/routes/api/analytics/heatmap/views/+server.ts` using the same proxy pattern, hitting `/api/analytics/heatmap/views`. Forward the `range` query param.

- [ ] **Step 5: Delete debug proxy**

Delete `web/src/routes/api/analytics/views/debug/+server.ts`.

- [ ] **Step 6: Verify frontend builds**

Run: `cd web && npx svelte-check`
Expected: PASS

- [ ] **Step 7: Commit**

```
git add web/src/lib/api.ts web/src/routes/api/
git commit -m "feat: add cumulative views and views heatmap API routes"
```

---

## Task 9: Frontend — Dashboard Updates

**Files:**
- Modify: `web/src/lib/components/Dashboard.svelte`

This task updates the dashboard to use the new data sources. The changes are:

- [ ] **Step 1: Read current Dashboard.svelte**

Read the full `web/src/lib/components/Dashboard.svelte` to understand the current chart rendering, data fetching, and component structure.

- [ ] **Step 2: Update views chart data fetching**

The views chart already calls `api.getViews()` — no change needed on the fetch. The backend now returns data from `daily_views` instead of spreading CTEs. The frontend grouping logic (weekly/monthly bucketing) continues to work unchanged since the response shape (`ViewsPoint[]`) is the same.

Verify the chart renders correctly with the new data by running `cd web && npm run dev` and checking the views chart.

- [ ] **Step 3: Add cumulative views chart**

Add a new chart section after the views-over-time chart. Fetch data with `api.getViewsCumulative()` and render as a line chart using Chart.js. Use the same time range as the views chart. This is a simple monotonically increasing line.

- [ ] **Step 4: Split heatmaps**

The current dashboard renders three heatmaps: Posting Activity, Engagement, Views. Update:
- **Views heatmap**: Change data source to `api.getViewsHeatmap(range)` instead of using the views field from `api.getHeatmap(range)`.
- **Posting heatmap**: Remove the views field from the posting activity heatmap display. It now shows only post count and engagement (likes + replies + reposts).
- **Engagement heatmap**: Unchanged.

- [ ] **Step 5: Verify frontend type-checks**

Run: `cd web && npx svelte-check`
Expected: PASS

- [ ] **Step 6: Commit**

```
git add web/src/lib/components/Dashboard.svelte
git commit -m "feat: update dashboard for clean data sources, add cumulative chart"
```

---

## Task 10: Rewrite Tests

**Files:**
- Modify: `postgraph-server/tests/views_accuracy.rs`

- [ ] **Step 1: Read current test file**

Read `postgraph-server/tests/views_accuracy.rs` to understand the test infrastructure (DB setup, helper functions).

- [ ] **Step 2: Rewrite test file**

Replace the entire test file. The new tests verify:

1. **Daily views upsert**: Insert rows into `daily_views`, verify they persist and upsert overwrites.
2. **Range sums monotonicity**: Insert daily_views data across different time periods, call the range-sums endpoint, verify `all >= 365d >= 270d >= ... >= 24h`.
3. **Chart-to-button consistency**: Sum the daily views from the chart endpoint for a given range, verify it matches the corresponding range-sum button value.
4. **Engagement capture-time attribution**: Insert engagement snapshots with known `captured_at` times, verify the engagement endpoint attributes deltas to capture dates (not post creation dates).

Each test should set up its own data, query the endpoint, and assert the result. Use the same test DB infrastructure as the existing file.

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`
Expected: PASS (note: tests require a test database — if not available, verify compile with `cargo test --workspace --no-run`)

- [ ] **Step 4: Commit**

```
git add postgraph-server/tests/views_accuracy.rs
git commit -m "test: rewrite views accuracy tests for daily_views-based pipeline"
```

---

## Task 11: Final Verification

- [ ] **Step 1: Full backend check**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets && cargo check --workspace`
Expected: All pass with no warnings.

- [ ] **Step 2: Full frontend check**

Run: `cd web && npx svelte-check`
Expected: PASS

- [ ] **Step 3: Verify CLAUDE.md is still accurate**

Read `CLAUDE.md` and verify the Key Patterns section still reflects reality. Update if needed (e.g., the sync description now mentions three tasks instead of one loop).

- [ ] **Step 4: Final commit**

```
git add -A
git commit -m "chore: final cleanup for data integrity redesign"
```
