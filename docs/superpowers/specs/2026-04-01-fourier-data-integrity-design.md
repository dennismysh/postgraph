# Fourier Page Data Integrity Redesign

**Date:** 2026-04-01
**Branch:** feat/data-integrity-redesign
**Status:** Approved

## Problem

The Fourier page has the same class of data integrity problems that were fixed in the analytics tab:

1. **Backdated engagement** — `postsToDaily()` groups all of a post's cumulative likes onto its creation date. A post with 1000 likes shows them all on day 1.
2. **Cumulative values, not deltas** — Sums `post.likes` (lifetime total) per day, not how much engagement arrived each day. No use of `engagement_snapshots`.
3. **Fabricated zeros** — Gap-fills every day between first and last post with `{ likes: 0, posts: 0 }`. FFT then detects patterns in this synthetic data.
4. **Ignores authoritative sources** — Doesn't use `daily_views` or `engagement_snapshots`. Reads `posts.likes` and invents a time series.

## Design

### Three Honest Signals

Replace the fabricated engagement time series with three signals, each backed by a single authoritative source:

| Signal | Source | Attribution | Gap handling |
|--------|--------|-------------|--------------|
| Daily views | `daily_views` table | API-reported date | No gaps (row per day) |
| Engagement velocity | `engagement_snapshots` deltas | Capture date | No gaps (daily sync) |
| Posting cadence | `posts.timestamp` | Post creation date | Zeros are honest (no posts = 0) |

### New Backend Endpoint

**`GET /api/analytics/engagement/daily-deltas?since=YYYY-MM-DD`**

Returns daily engagement deltas (new likes/replies/reposts/quotes received per day) computed server-side from `engagement_snapshots`.

Response shape:
```json
[{ "date": "2026-03-15", "likes": 12, "replies": 3, "reposts": 1, "quotes": 0 }]
```

SQL:
```sql
WITH daily_snapshots AS (
    SELECT DISTINCT ON (post_id, captured_at::date)
        post_id,
        captured_at::date AS capture_date,
        likes, replies, reposts, quotes
    FROM engagement_snapshots
    ORDER BY post_id, captured_at::date, captured_at DESC
),
deltas AS (
    SELECT
        capture_date,
        likes - LAG(likes) OVER w AS d_likes,
        replies - LAG(replies) OVER w AS d_replies,
        reposts - LAG(reposts) OVER w AS d_reposts,
        quotes - LAG(quotes) OVER w AS d_quotes
    FROM daily_snapshots
    WINDOW w AS (PARTITION BY post_id ORDER BY capture_date)
)
SELECT
    capture_date AS date,
    COALESCE(SUM(d_likes), 0) AS likes,
    COALESCE(SUM(d_replies), 0) AS replies,
    COALESCE(SUM(d_reposts), 0) AS reposts,
    COALESCE(SUM(d_quotes), 0) AS quotes
FROM deltas
WHERE capture_date >= $1
GROUP BY capture_date
ORDER BY capture_date
```

Notes:
- `DISTINCT ON` picks the latest snapshot per post per day (handles multiple syncs)
- First snapshot per post has NULL delta — `COALESCE` handles this
- Negative deltas kept (API corrections are honest)

### Reused Endpoints

- `GET /api/analytics/views?since=YYYY-MM-DD` — daily views from `daily_views` table (no changes)
- `GET /api/posts` — post list for client-side posting cadence count (no changes)

### Frontend Chart Layout

4 chart sections, each with its own time-range selector (7d, 30d, 90d, 1y, all):

1. **Daily Views** — Line chart (raw daily views) + low-pass smoothed overlay (toggle, default on). Below: frequency spectrum showing dominant periods. Source: `GET /api/analytics/views`.

2. **Engagement Velocity** — Line chart with toggleable series for likes/replies/reposts per day. Low-pass overlay available. Below: spectrum analysis. Source: `GET /api/analytics/engagement/daily-deltas`.

3. **Posting Cadence** — Line chart of posts-per-day. Zeros are honest (no posts that day = 0). Low-pass overlay available. Spectrum below. Source: client-side count from `GET /api/posts`, grouped by `timestamp` date.

4. **Hourly Distribution** — Bar chart of posts by hour-of-day. No spectrum. **Unchanged** from current implementation.

### Low-Pass Filter

Kept as a toggle (default on). Legitimate analysis tool for spotting macro trends underneath noisy daily data. No changes to the filter implementation.

### What Gets Deleted

- `postsToDaily()` in `fourier.ts` — the fabrication function that grouped cumulative likes by post creation date and gap-filled with zeros
- Any dead imports/types only used by the old approach

### What Stays Unchanged

- `fft.ts` — FFT math is correct (Cooley-Tukey)
- `postsToHourly()` — honest (groups by post creation hour)
- Time-range selectors, peak detection annotations, page structure/styling
- `sync.rs`, migrations, existing analytics endpoints

## File Changes

### Backend (Rust)
- **`routes/analytics.rs`** — Add `get_engagement_daily_deltas` handler + route registration
- **`db.rs`** — Add `get_engagement_daily_deltas(pool, since)` query function
- **`types.rs`** — Add `DailyEngagementDelta` response struct

### Frontend (Svelte)
- **`lib/fourier.ts`** — Rewrite: remove `postsToDaily()`, add `postsToCadence()` (posts-per-day count only)
- **`lib/api.ts`** — Add `getViews()`, `getEngagementDeltas()` API client functions (or reuse existing)
- **`lib/components/Fourier.svelte`** — Rewrite data fetching to call views + engagement deltas + posts endpoints. Wire up 4 chart sections. Keep FFT/spectrum/low-pass/time-range logic.
- **`routes/api/`** — Add server-side proxy route for engagement deltas if required by existing proxy pattern

### No Changes
- `lib/fft.ts`
- `routes/fourier/+page.svelte`
- `sync.rs`, migrations
