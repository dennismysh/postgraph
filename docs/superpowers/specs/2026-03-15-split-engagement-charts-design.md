# Split Engagement Over Time into 3 Independent Charts

**Date:** 2026-03-15

## Summary

Replace the single combined "Engagement Over Time" multi-line chart with 3 independent single-line charts: Likes Over Time, Replies Over Time, Reposts Over Time. Each chart has its own dropdown time range selector defaulting to 30 days.

## Backend

### New Endpoint: `GET /api/analytics/engagement`

Query parameters:
- `since` (optional): ISO timestamp filter
- `grouping` (optional): `"hourly"` for 24h range, otherwise daily

Returns `Vec<EngagementPoint>` (existing struct: `{ date, likes, replies, reposts }`).

Uses the same delta-computation pattern as `/api/analytics/views`:
- First snapshot per post → attributed to post publication date
- Subsequent snapshots → attributed to snapshot `captured_at`
- Aggregated by `DATE_TRUNC` based on grouping

The existing `engagement_over_time` field in `/api/analytics` remains unchanged.

### SvelteKit Proxy

New proxy route at `web/src/routes/api/analytics/engagement/+server.ts` forwarding to the backend with auth headers.

## Frontend

### 3 Chart Sections (replacing combined engagement chart)

Each chart section contains:
1. **Header row**: chart title + dropdown `<select>` for time range
2. **Chart.js line chart**: single dataset, gradient fill

| Chart | Color | Label |
|-------|-------|-------|
| Likes Over Time | #e6194b (red) | Likes |
| Replies Over Time | #3cb44b (green) | Replies |
| Reposts Over Time | #4363d8 (blue) | Reposts |

### Dropdown Time Range Options

Same as views chart: 24h, 7d, 14d, 30d, 60d, 90d, 180d, 270d, 365d, All Time. Default: 30d.

### Data Flow Per Chart

1. On mount and on dropdown change, fetch `/api/analytics/engagement?since=<iso>&grouping=<hourly|daily>`
2. Extract the relevant metric (likes/replies/reposts) from the response
3. Apply gap-filling (hourly for 24h, daily for short ranges, weekly for long ranges)
4. Render single-line Chart.js chart

### Layout

Stacked vertically in the dashboard, in order: Likes, Replies, Reposts. Placed where the combined engagement chart currently sits.
