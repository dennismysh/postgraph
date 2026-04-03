# Analytics V2 Design — Per-Post Views

**Date:** 2026-04-03
**Status:** Approved

## Problem

The existing Analytics page uses the Threads user-level insights API (`daily_views` table) for views charts. This API has a ~24 hour lag — it only reports completed day buckets (midnight-to-midnight PT). Per-post insights are near-real-time (15-min sync), so building an alternate views page from per-post snapshot deltas lets us compare accuracy vs freshness.

Known trade-off: per-post deltas undercount because a post's first snapshot has N views with no previous value to diff against — those views are lost from the delta calculation.

## Design

### Backend — Three new endpoints

All three reuse existing response types (`ViewsPoint`, `CumulativeViewsPoint`, `ViewsRangeSums`).

**1. `GET /api/analytics/views/per-post?since=YYYY-MM-DDTHH:MM:SSZ`**

Daily view deltas from `engagement_snapshots`:

```sql
WITH daily_snapshots AS (
    SELECT DISTINCT ON (post_id, captured_at::date)
        post_id,
        captured_at::date AS capture_date,
        views
    FROM engagement_snapshots
    ORDER BY post_id, captured_at::date, captured_at DESC
),
deltas AS (
    SELECT
        capture_date,
        views - LAG(views) OVER (PARTITION BY post_id ORDER BY capture_date) AS d_views
    FROM daily_snapshots
)
SELECT
    capture_date::text AS date,
    COALESCE(SUM(d_views), 0)::bigint AS views
FROM deltas
WHERE capture_date IS NOT NULL
  AND ($1::date IS NULL OR capture_date >= $1)
GROUP BY capture_date
ORDER BY capture_date
```

**2. `GET /api/analytics/views/per-post/cumulative?since=YYYY-MM-DDTHH:MM:SSZ`**

Same base query, with cumulative sum:

```sql
-- Same CTEs as above, then:
SELECT
    date,
    SUM(views) OVER (ORDER BY date)::bigint AS cumulative_views
FROM (
    SELECT capture_date::text AS date,
           COALESCE(SUM(d_views), 0)::bigint AS views
    FROM deltas
    WHERE capture_date IS NOT NULL
      AND ($1::date IS NULL OR capture_date >= $1)
    GROUP BY capture_date
) daily
ORDER BY date
```

**3. `GET /api/analytics/views/per-post/range-sums`**

Same CASE/SUM pattern as existing `get_views_range_sums` but over per-post delta data. Returns the same `ViewsRangeSums` response with keys: all, 365d, 270d, 180d, 90d, 60d, 30d, 14d, 7d, 24h.

### Frontend — Analytics V2 page

**Route:** `/analytics-v2` — nav link between Analytics and ƒ(t).

**Component:** `AnalyticsV2.svelte` with three charts:

1. **Range sum buttons** — Last 24h, 7d, 14d, 30d, 2mo, 3mo, 6mo, 9mo, 12mo, All Time (with totals). Calls per-post range-sums endpoint.
2. **Views Over Time** — Daily line chart with grouping selector. Calls per-post views endpoint.
3. **Cumulative Views** — Running total line chart. Calls per-post cumulative endpoint.

Same Chart.js styling, dark theme, and time-range patterns as existing Dashboard.

### File Changes

**Backend (Rust):**
- `routes/analytics.rs` — Add `get_views_per_post`, `get_views_per_post_cumulative`, `get_views_per_post_range_sums`
- `main.rs` — Register 3 routes

**Frontend (Svelte):**
- `web/src/lib/api.ts` — Add `getViewsPerPost`, `getViewsPerPostCumulative`, `getViewsPerPostRangeSums`
- `web/src/routes/api/analytics/views/per-post/+server.ts` — Proxy
- `web/src/routes/api/analytics/views/per-post/cumulative/+server.ts` — Proxy
- `web/src/routes/api/analytics/views/per-post/range-sums/+server.ts` — Proxy
- `web/src/lib/components/AnalyticsV2.svelte` — New component
- `web/src/routes/analytics-v2/+page.svelte` — Page wrapper
- `web/src/routes/+layout.svelte` — Add nav link

**No new types needed** — all endpoints reuse existing response shapes.
