# Data Integrity Redesign

**Date:** 2026-03-31
**Status:** Approved
**Approach:** Replace + Restructure Sync Pipeline (Approach 3)

## Problem

The analytics pipeline has accumulated workarounds that fabricate, inflate, or misattribute data:

1. **User-level views (profile views) conflated with post views** — `max(delta_sum, posts_sum, user_level)` treats different metrics as interchangeable.
2. **Daily view attribution fabricated** — `first_snapshot_spread` divides views evenly across days from post creation to first snapshot capture. A post that went viral on day 1 shows as steady views across 30 days.
3. **Engagement backdated to post creation** — First snapshot's engagement delta attributed to `post_timestamp`, not `captured_at`.
4. **`GREATEST()` ratchet** — Metrics can never decrease, silently ignoring API corrections.
5. **Authoritative daily time-series discarded** — User-level insights API returns daily views with `end_time` timestamps, but the code only stores a total and discards the dates.
6. **Heatmap uses wrong axis** — Views grouped by post publication date, not by date views occurred.

## Decisions

| Decision | Choice |
|----------|--------|
| Charts | A (daily reach), B (per-post performance), C (growth trajectory) — independent |
| Historical data | Backfill ~730 days from user-level insights API |
| Engagement time attribution | Capture time (honest) |
| Metric updates | Trust the API directly (no GREATEST ratchet) |
| Heatmaps | Two: daily reach (from user insights) + posting activity (from posts table) |

## Data Model

### New table: `daily_views`

```sql
CREATE TABLE daily_views (
    date DATE PRIMARY KEY,
    views BIGINT NOT NULL,
    source TEXT NOT NULL DEFAULT 'user_insights',
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

One row per day. Source of truth for charts A and C, the views heatmap, and range-sum buttons. Populated from the user-level insights API time-series response (`values[].end_time`).

### Changes to existing tables

- **`user_insights`** — Drop. Replaced by `daily_views` (total is `SUM(views)`).
- **`posts`** — No schema change. `views/likes/etc` written directly from API (no `GREATEST`).
- **`engagement_snapshots`** — No schema change. Still used for engagement-over-time chart with capture-time attribution.

### No changes to

`intents`, `subjects`, `subject_edges`, `sync_state`, `api_tokens`.

## Sync Pipeline — Three Independent Tasks

Currently `run_sync` does everything in one interleaved loop. Restructure into three independent async tasks:

### Task 1: Post Discovery (`sync_posts`)

- Paginate `GET /me/threads`, upsert posts into `posts` table.
- Skip `REPOST_FACADE`.
- No metrics fetching — just discovers posts and stores metadata.
- Runs every 15 min.

### Task 2: Per-Post Metrics (`sync_post_metrics`)

- Iterate all post IDs, call `GET /{post_id}/insights` for each.
- Write values directly: `UPDATE posts SET views = $1, likes = $2, ...` (no `GREATEST`).
- Insert engagement snapshot with `captured_at = NOW()`.
- Throttle + retry on rate limit (same as today, but isolated).
- Runs after post discovery completes.

### Task 3: Daily Views Collection (`sync_daily_views`)

- Call `GET /me/threads_insights?metric=views&since=X&until=Y`.
- Parse `end_time` from each value entry (requires adding `end_time` to `InsightValue` struct).
- Upsert into `daily_views` using the API's `end_time` as the date.
- On first run: backfill ~730 days in 90-day windows.
- On subsequent runs: only fetch since `MAX(date) FROM daily_views` (typically last 2-3 days).
- Runs daily (nightly sync). Doesn't need 15-min frequency since data is daily granularity.

### Scheduling in `main.rs`

```
Every 15 min:  sync_posts → sync_post_metrics (sequential)
Daily at 2am:  sync_daily_views
On startup:    sync_daily_views (backfill if daily_views is empty)
```

## Analytics Endpoints — Rewritten

Every endpoint has one data source. No fallbacks, no triple-max, no spreading CTEs.

### Chart A — Daily Reach: `GET /api/analytics/views`

```sql
SELECT date::text, views FROM daily_views
WHERE date >= $1 ORDER BY date
```

Grouping (weekly/monthly) done in Rust by summing adjacent rows.

### Chart B — Per-Post Performance: `GET /api/analytics/posts/performance`

```sql
SELECT id, text, views, likes, replies_count, reposts, quotes, timestamp
FROM posts ORDER BY views DESC
```

Current cumulative values straight from `posts` table.

### Chart C — Growth Trajectory: `GET /api/analytics/views/cumulative`

```sql
SELECT date::text, SUM(views) OVER (ORDER BY date) AS cumulative_views
FROM daily_views WHERE date >= $1 ORDER BY date
```

Running sum of `daily_views`.

### Engagement Over Time: `GET /api/analytics/engagement`

```sql
-- Key change: always captured_at, never post_timestamp
SELECT DATE(captured_at)::text AS date,
       SUM(GREATEST(likes - COALESCE(prev_likes, 0), 0)) AS likes,
       SUM(GREATEST(replies_count - COALESCE(prev_replies, 0), 0)) AS replies,
       SUM(GREATEST(reposts - COALESCE(prev_reposts, 0), 0)) AS reposts
FROM ordered_snapshots
GROUP BY DATE(captured_at) ORDER BY date
```

Deltas attributed to when we observed them, not post creation date.

### Heatmap A — Daily Reach: `GET /api/analytics/heatmap/views`

```sql
SELECT date::text, views FROM daily_views WHERE date >= $1
```

Same source as Chart A.

### Heatmap B — Posting Activity: `GET /api/analytics/heatmap/posts`

```sql
SELECT DATE(timestamp)::text, COUNT(*), SUM(likes), SUM(replies_count),
       SUM(reposts), media_type
FROM posts WHERE timestamp >= $1
GROUP BY DATE(timestamp), media_type
```

Grouped by publish date, honestly labeled. Views column removed from this query.

### Range Sums: `GET /api/analytics/views/range-sums`

```sql
SELECT
    COALESCE(SUM(views), 0),
    COALESCE(SUM(CASE WHEN date >= $1 THEN views END), 0),
    ... (one CASE per range)
FROM daily_views
```

Single pass over `daily_views`. No triple-max.

### Removed entirely

- `GET /api/analytics/views/debug` — not needed when data is clean.
- `get_views_from_snapshots` function and all spreading CTEs.
- Triple-max `total_views` calculation in `get_analytics`.

### Unchanged

- Histograms — already correct (uses `posts.views` directly).
- Per-post engagement detail — raw snapshots, already honest.

## Frontend Changes

| Component | Status | Change |
|-----------|--------|--------|
| Stats cards | Keep | `total_views` from `SUM(daily_views)` |
| Views over time chart | Rewrite | Chart A — reads from `daily_views` |
| Growth trajectory chart | New | Chart C — cumulative line |
| Engagement charts (3x) | Fix | Attribution now at capture time |
| Views heatmap | Rewrite | Source from `daily_views` |
| Posting heatmap | Fix | Remove views column, rename honestly |
| Engagement heatmap | Keep | Already uses post-level data |
| Range buttons | Simplify | Single query on `daily_views` |
| Histograms | Keep | Already correct |
| Subjects breakdown | Keep | Unchanged |
| Recent posts table | Keep | Unchanged |
| Post detail engagement | Keep | Raw snapshots, already honest |
| Graph page | Keep | Unchanged |

New frontend proxy routes: `/api/analytics/views/cumulative`, `/api/analytics/heatmap/views`.

## Migration & Cleanup

### Database migration (011)

```sql
CREATE TABLE daily_views (
    date DATE PRIMARY KEY,
    views BIGINT NOT NULL,
    source TEXT NOT NULL DEFAULT 'user_insights',
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

DROP TABLE IF EXISTS user_insights;
```

### Threads API client (`threads.rs`)

- Add `end_time: Option<String>` to `InsightValue` struct.
- `get_user_insights` returns `Vec<(NaiveDate, i64)>` — each entry is a date+views pair parsed from `end_time`.
- Remove `UserInsights` and `UserDailyViews` structs.

### Sync (`sync.rs`)

- Split into three public functions: `sync_posts`, `sync_post_metrics`, `sync_daily_views`.
- `sync_post_metrics`: `UPDATE posts SET views = $1` (no `GREATEST`).
- `sync_daily_views`: upsert into `daily_views` using `ON CONFLICT (date) DO UPDATE SET views = $2, fetched_at = NOW()`.
- Remove old `run_sync` and `refresh_all_metrics`.

### Code deleted from `analytics.rs`

- `get_views_from_snapshots` (spreading CTE) — ~90 lines.
- `get_views_debug` (debug endpoint) — ~125 lines.
- Triple-max logic in `get_analytics` — ~20 lines.
- Spreading CTE in `get_views_range_sums` — ~65 lines.
- Total: ~300 lines of workaround SQL removed.

### Tests (`views_accuracy.rs`)

- Rewrite to test against `daily_views` table.
- Layer 1 (storage): verify `sync_daily_views` upserts correctly.
- Layer 2 (queries): verify range sums are monotonically decreasing.
- Layer 3 (consistency): chart sum matches range button value.
- Remove tests for spreading logic and migration-004 regression.

### Legacy tables left in place

- `topics`, `post_topics`, `post_edges` — already unused, not worth a migration to drop.
- `engagement_snapshots` — still actively used for engagement charts.

## Data Source Map

Every number shown in the UI traces to exactly one authoritative source:

| What's displayed | Source | Table |
|-----------------|--------|-------|
| Daily views chart | User-level insights API | `daily_views` |
| Cumulative views chart | User-level insights API | `daily_views` (window sum) |
| Per-post views/engagement | Per-post insights API | `posts` |
| Engagement over time | Per-post insights API | `engagement_snapshots` (capture-time deltas) |
| Views heatmap | User-level insights API | `daily_views` |
| Posting heatmap | Post metadata | `posts` (publish date) |
| Range sum buttons | User-level insights API | `daily_views` |
| Total views stat | User-level insights API | `daily_views` (SUM) |
| Histograms | Per-post insights API | `posts` |
