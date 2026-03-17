# GitHub-Style Activity Heatmaps

## Overview

Add three GitHub-style contribution heatmaps to the analytics dashboard, showing posting frequency, engagement, and views over time as color-coded day grids. Each heatmap is an independent card with its own time-range dropdown.

## Data Source

All three heatmaps are powered by a single new backend endpoint that aggregates data from the existing `posts` table using latest snapshot values (current totals on each post, grouped by publish date).

### New Endpoint

`GET /api/analytics/heatmap?range=1y`

**Query parameter:** `range` — one of `3m`, `6m`, `1y`, `all` (default: `1y`)

**Response:**

```json
{
  "days": [
    {
      "date": "2026-03-15",
      "posts": 2,
      "likes": 45,
      "replies": 12,
      "reposts": 8,
      "views": 340,
      "media_types": { "IMAGE": 1, "TEXT": 1 }
    }
  ]
}
```

**SQL:**

```sql
SELECT
  DATE(timestamp) AS date,
  COUNT(*) AS posts,
  SUM(likes) AS likes,
  SUM(replies_count) AS replies,
  SUM(reposts) AS reposts,
  SUM(views) AS views,
  media_type
FROM posts
WHERE timestamp >= $1
GROUP BY DATE(timestamp), media_type
```

The backend post-processes the media_type grouping into the `media_types` map per day. Days with no posts are absent from the response — the frontend fills gaps with zeros.

### Frontend Proxy

`web/src/routes/api/analytics/heatmap/+server.ts` — follows the same proxy pattern as existing analytics routes. Forwards `range` query param to the Rust backend with server-side API key auth.

## Heatmap Component

**New file:** `web/src/lib/components/Heatmap.svelte`

A single reusable SVG-based component, instantiated three times with different props.

### Props

| Prop | Type | Description |
|------|------|-------------|
| `title` | `string` | Card header label ("Posting Activity", "Engagement", "Views") |
| `data` | `HeatmapDay[]` | The `days` array from the API |
| `valueKey` | `string` | Field to visualize: `"posts"`, `"engagement"` (likes+replies+reposts), `"views"` |
| `colorScale` | `string[]` | Array of 5 hex colors: [empty, low, medium, high, max] |
| `tooltipFormatter` | `(day: HeatmapDay) => string` | Returns tooltip HTML for a given day |

### Color Scales

- **Posts:** GitHub green — `["#161b22", "#0e4429", "#006d32", "#26a641", "#39d353"]`
- **Engagement:** Amber/orange — `["#161b22", "#4a2800", "#7a4500", "#b36b00", "#f59e0b"]`
- **Views:** Blue — `["#161b22", "#0a2647", "#144a7a", "#1e6fbf", "#3b82f6"]`

### Color Quantiles

Colors are assigned using quartile boundaries computed from the data:
- Value = 0 → color[0] (background/empty)
- Value ≤ p25 → color[1]
- Value ≤ p50 → color[2]
- Value ≤ p75 → color[3]
- Value > p75 → color[4]

This ensures the color distribution adapts to the actual data range regardless of absolute values.

### SVG Layout

GitHub contribution graph style:
- **Columns** = weeks (Sunday-aligned)
- **Rows** = days of week (7 rows: Mon–Sun)
- **Cell size:** ~11px rounded rectangles with ~2px gap
- **Month labels** along the top (Jan, Feb, Mar...)
- **Day-of-week labels** on the left (Mon, Wed, Fri)

### Scrolling

- Container has `overflow-x: auto` for horizontal scrolling
- On mount (and when data changes), scroll position is set to far-right so the most recent data is visible
- Each heatmap scrolls independently

### Tooltip

Positioned absolutely near the hovered SVG cell. Content varies by heatmap type:

- **Posts:** "Mar 15, 2026: 3 posts (2 IMAGE, 1 TEXT)"
- **Engagement:** "Mar 15, 2026: 65 total (45 likes, 12 replies, 8 reposts)"
- **Views:** "Mar 15, 2026: 340 views (across 2 posts)"

### Time Range Dropdown

Each heatmap card has its own `<select>` dropdown in the card header. Options:
- 3 months
- 6 months
- **1 year** (default)
- All time

Changing the dropdown re-fetches data for that heatmap only.

## Dashboard Integration

The three heatmap cards are placed in `Dashboard.svelte` below the Likes/Replies/Reposts engagement charts and above the Subjects Breakdown bar chart.

```
┌─────────────────────────────────────────────────┐
│  Stats cards (Total Posts, Analyzed, Subjects)   │
├─────────────────────────────────────────────────┤
│  Sync / Analysis status                         │
├─────────────────────────────────────────────────┤
│  Views Over Time chart                          │
├─────────────────────────────────────────────────┤
│  Likes Over Time | Replies Over Time | Reposts  │
├─────────────────────────────────────────────────┤
│  Posting Activity               [1 year ▾]      │
│  ░░▓▓░░█░░░░▓░░░░░░░░░▓▓░░░░░                  │
├─────────────────────────────────────────────────┤
│  Engagement                     [1 year ▾]      │
│  ░░▓░░░▓▓░░░░█░░░░░░▓░░░░░░░░                  │
├─────────────────────────────────────────────────┤
│  Views                          [1 year ▾]      │
│  ░░░▓▓░░▓░░░░▓▓░░░░░░░▓░░░░░░                  │
├─────────────────────────────────────────────────┤
│  Subjects Breakdown                             │
├─────────────────────────────────────────────────┤
│  Recent Posts table                             │
└─────────────────────────────────────────────────┘
```

Each heatmap is fully independent — own data fetch, own dropdown state, own scroll position.

## Files Changed

| File | Change |
|------|--------|
| `postgraph-server/src/routes/analytics.rs` | New `heatmap` handler, `HeatmapDay` and `HeatmapResponse` types |
| `postgraph-server/src/routes/mod.rs` | Register `/api/analytics/heatmap` route |
| `web/src/routes/api/analytics/heatmap/+server.ts` | Frontend proxy route |
| `web/src/lib/components/Heatmap.svelte` | New reusable SVG heatmap component |
| `web/src/lib/components/Dashboard.svelte` | Add three `<Heatmap>` instances |
| `web/src/lib/api.ts` | Add `fetchHeatmapData()` function |

## No New Dependencies

The heatmap is rendered as custom SVG — no new npm packages or Rust crates. Chart.js is not used for this component.

## No Database Migrations

All data already exists in the `posts` table. The endpoint queries `posts` directly with GROUP BY on the date.
