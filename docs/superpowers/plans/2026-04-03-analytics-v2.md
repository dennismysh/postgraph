# Analytics V2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an Analytics V2 page that shows views charts derived from per-post engagement snapshot deltas instead of the daily_views table, enabling side-by-side comparison of the two data sources.

**Architecture:** Three new backend endpoints compute daily view deltas, cumulative views, and range sums from `engagement_snapshots` using the same LAG pattern already used for engagement deltas. A new Svelte component renders the same three views charts as the existing Dashboard but wired to the per-post endpoints.

**Tech Stack:** Rust/axum (backend), Svelte 5 + Chart.js (frontend), TypeScript

**Spec:** `docs/superpowers/specs/2026-04-03-analytics-v2-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `postgraph-server/src/routes/analytics.rs` | Modify | Add 3 handlers for per-post views |
| `postgraph-server/src/main.rs` | Modify | Register 3 routes |
| `web/src/routes/api/analytics/views/per-post/+server.ts` | Create | Proxy |
| `web/src/routes/api/analytics/views/per-post/cumulative/+server.ts` | Create | Proxy |
| `web/src/routes/api/analytics/views/per-post/range-sums/+server.ts` | Create | Proxy |
| `web/src/lib/api.ts` | Modify | Add 3 API client functions |
| `web/src/lib/components/AnalyticsV2.svelte` | Create | Views charts component |
| `web/src/routes/analytics-v2/+page.svelte` | Create | Page wrapper |
| `web/src/routes/+layout.svelte` | Modify | Add nav link |

---

### Task 1: Backend — Add per-post views endpoints

**Files:**
- Modify: `postgraph-server/src/routes/analytics.rs`
- Modify: `postgraph-server/src/main.rs`

All three handlers reuse existing response types (`ViewsPoint`, `CumulativeViewsPoint`, `ViewsRangeSums`).

- [ ] **Step 1: Add the daily views per-post handler**

Add after the `get_engagement_daily_deltas` function in `postgraph-server/src/routes/analytics.rs`:

```rust
// ── Per-Post Views (from engagement_snapshots) ─────────────────────

pub async fn get_views_per_post(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<ViewsPoint>>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.date_naive());

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"WITH daily_snapshots AS (
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
           ORDER BY capture_date"#,
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
```

- [ ] **Step 2: Add the cumulative per-post views handler**

Add immediately after `get_views_per_post`:

```rust
pub async fn get_views_per_post_cumulative(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<CumulativeViewsPoint>>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.date_naive());

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"WITH daily_snapshots AS (
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
           ),
           daily AS (
               SELECT
                   capture_date,
                   COALESCE(SUM(d_views), 0)::bigint AS views
               FROM deltas
               WHERE capture_date IS NOT NULL
                 AND ($1::date IS NULL OR capture_date >= $1)
               GROUP BY capture_date
           )
           SELECT
               capture_date::text AS date,
               SUM(views) OVER (ORDER BY capture_date)::bigint AS cumulative_views
           FROM daily
           ORDER BY capture_date"#,
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
```

- [ ] **Step 3: Add the range sums per-post handler**

Add immediately after `get_views_per_post_cumulative`:

```rust
pub async fn get_views_per_post_range_sums(
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
        r#"WITH daily_snapshots AS (
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
           ),
           daily AS (
               SELECT capture_date, COALESCE(SUM(d_views), 0)::bigint AS views
               FROM deltas
               WHERE capture_date IS NOT NULL
               GROUP BY capture_date
           )
           SELECT
               COALESCE(SUM(views), 0)::bigint,
               COALESCE(SUM(CASE WHEN capture_date >= $1 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN capture_date >= $2 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN capture_date >= $3 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN capture_date >= $4 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN capture_date >= $5 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN capture_date >= $6 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN capture_date >= $7 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN capture_date >= $8 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN capture_date >= $9 THEN views END), 0)::bigint
           FROM daily"#,
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
```

- [ ] **Step 4: Register routes**

In `postgraph-server/src/main.rs`, add after the existing `/api/analytics/views/cumulative` route:

```rust
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
```

- [ ] **Step 5: Verify**

Run: `cargo check --workspace`
Expected: compiles with no new errors

- [ ] **Step 6: Commit**

```bash
git add postgraph-server/src/routes/analytics.rs postgraph-server/src/main.rs
git commit -m "feat: add per-post views endpoints (daily, cumulative, range-sums)"
```

---

### Task 2: Frontend — Add proxy routes and API client functions

**Files:**
- Create: `web/src/routes/api/analytics/views/per-post/+server.ts`
- Create: `web/src/routes/api/analytics/views/per-post/cumulative/+server.ts`
- Create: `web/src/routes/api/analytics/views/per-post/range-sums/+server.ts`
- Modify: `web/src/lib/api.ts`

- [ ] **Step 1: Create proxy for per-post views**

Create `web/src/routes/api/analytics/views/per-post/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const since = url.searchParams.get('since');
  const searchParams = new URLSearchParams();
  if (since) searchParams.set('since', since);
  return proxyToBackend('/api/analytics/views/per-post', { searchParams });
};
```

- [ ] **Step 2: Create proxy for per-post cumulative**

Create `web/src/routes/api/analytics/views/per-post/cumulative/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const since = url.searchParams.get('since');
  const searchParams = new URLSearchParams();
  if (since) searchParams.set('since', since);
  return proxyToBackend('/api/analytics/views/per-post/cumulative', { searchParams });
};
```

- [ ] **Step 3: Create proxy for per-post range-sums**

Create `web/src/routes/api/analytics/views/per-post/range-sums/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  return proxyToBackend('/api/analytics/views/per-post/range-sums');
};
```

- [ ] **Step 4: Add API client functions**

In `web/src/lib/api.ts`, add three methods inside the `api` object after `getViewsHeatmap`:

```typescript
  getViewsPerPost: (since?: string) => {
    const params = new URLSearchParams();
    if (since) params.set('since', since);
    const qs = params.toString();
    return fetchApi<ViewsPoint[]>(`/api/analytics/views/per-post${qs ? `?${qs}` : ''}`);
  },
  getViewsPerPostCumulative: (since?: string): Promise<CumulativeViewsPoint[]> => {
    const params = new URLSearchParams();
    if (since) params.set('since', since);
    const qs = params.toString();
    return fetchApi(`/api/analytics/views/per-post/cumulative${qs ? `?${qs}` : ''}`);
  },
  getViewsPerPostRangeSums: () => fetchApi<ViewsRangeSums>('/api/analytics/views/per-post/range-sums'),
```

- [ ] **Step 5: Commit**

```bash
git add web/src/routes/api/analytics/views/per-post/ web/src/lib/api.ts
git commit -m "feat: add per-post views proxy routes and API client functions"
```

---

### Task 3: Frontend — Create AnalyticsV2 component and page

**Files:**
- Create: `web/src/lib/components/AnalyticsV2.svelte`
- Create: `web/src/routes/analytics-v2/+page.svelte`
- Modify: `web/src/routes/+layout.svelte`

This is the big task. The component has three charts (range sums, views over time, cumulative views) using the same Chart.js patterns as the existing Dashboard but calling the per-post endpoints.

- [ ] **Step 1: Create AnalyticsV2.svelte**

Create `web/src/lib/components/AnalyticsV2.svelte`:

```svelte
<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import Chart from 'chart.js/auto';
  import { api, type ViewsPoint, type CumulativeViewsPoint } from '$lib/api';

  let loading = $state(true);
  let error: string | null = $state(null);

  let allViewsData: ViewsPoint[] = $state([]);
  let viewsData: ViewsPoint[] = $state([]);
  let rangeSums: Record<string, number> = $state({});

  let viewsCanvas: HTMLCanvasElement = $state(null!);
  let cumulativeCanvas: HTMLCanvasElement = $state(null!);
  let viewsChart: Chart | null = null;
  let cumulativeChart: Chart | null = null;

  const timeRanges = [
    { label: 'Last 24 Hours', key: '24h' },
    { label: 'Last 7 Days', key: '7d' },
    { label: 'Last 2 Weeks', key: '14d' },
    { label: 'Last 30 Days', key: '30d' },
    { label: 'Last 2 Months', key: '60d' },
    { label: 'Last 3 Months', key: '90d' },
    { label: 'Last 6 Months', key: '180d' },
    { label: 'Last 9 Months', key: '270d' },
    { label: 'Last 12 Months', key: '365d' },
    { label: 'All Time', key: 'all' },
  ];

  let selectedRange = $state('30d');

  const viewsGroupingOptions = [
    { label: 'Daily', value: 'daily' },
    { label: 'Weekly', value: 'weekly' },
    { label: 'Monthly', value: 'monthly' },
  ];
  let viewsGrouping = $state('weekly');

  function getSinceDate(range: string): string | undefined {
    if (range === 'all') return undefined;
    const now = new Date();
    const days = range === '24h' ? 1 : parseInt(range);
    now.setDate(now.getDate() - days);
    return now.toISOString();
  }

  function formatLabel(key: string): string {
    const d = new Date(key + 'T00:00:00');
    return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
  }

  function getWeekStart(dateStr: string): string {
    const d = new Date(dateStr + 'T00:00:00');
    d.setDate(d.getDate() - d.getDay());
    return d.toISOString().slice(0, 10);
  }

  function groupViewsData(data: ViewsPoint[], grouping: string): ViewsPoint[] {
    if (grouping === 'daily') {
      return data.map(p => ({ date: formatLabel(p.date), views: p.views }));
    }
    const grouped = new Map<string, number>();
    for (const point of data) {
      const key = grouping === 'monthly' ? point.date.slice(0, 7) : getWeekStart(point.date);
      grouped.set(key, (grouped.get(key) ?? 0) + point.views);
    }
    return [...grouped.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, views]) => {
        const label = grouping === 'monthly'
          ? new Date(parseInt(key.slice(0, 4)), parseInt(key.slice(5, 7)) - 1)
              .toLocaleDateString('en-US', { month: 'short', year: '2-digit' })
          : formatLabel(key);
        return { date: label, views };
      });
  }

  function formatCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
    return n.toString();
  }

  const darkTooltip = {
    backgroundColor: '#1a1a1a',
    borderColor: '#333',
    borderWidth: 1,
    titleColor: '#ccc',
    bodyColor: '#eee',
    padding: 10,
    displayColors: false,
  };

  function renderViewsChart() {
    viewsChart?.destroy();
    if (!viewsCanvas) return;

    const chartData = groupViewsData(viewsData, viewsGrouping);

    viewsChart = new Chart(viewsCanvas, {
      type: 'line',
      data: {
        labels: chartData.map(v => v.date),
        datasets: [{
          label: 'Views (per-post)',
          data: chartData.map(v => v.views),
          borderColor: '#f58231',
          backgroundColor: (ctx: { chart: Chart }) => {
            const gradient = ctx.chart.ctx.createLinearGradient(0, 0, 0, ctx.chart.height);
            gradient.addColorStop(0, 'rgba(245, 130, 49, 0.35)');
            gradient.addColorStop(1, 'rgba(245, 130, 49, 0.0)');
            return gradient;
          },
          fill: true,
          cubicInterpolationMode: 'monotone' as const,
          borderWidth: 2.5,
          pointRadius: chartData.length > 30 ? 0 : 3,
          pointHoverRadius: 6,
          pointBackgroundColor: '#f58231',
          pointBorderColor: '#111',
          pointBorderWidth: 2,
        }],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { intersect: false, mode: 'index' as const },
        plugins: { legend: { display: false }, tooltip: darkTooltip },
        scales: {
          x: {
            ticks: { color: '#666', maxRotation: 45, maxTicksLimit: 12, font: { size: 11 } },
            grid: { color: 'rgba(255,255,255,0.04)' },
          },
          y: {
            ticks: { color: '#666', font: { size: 11 } },
            grid: { color: 'rgba(255,255,255,0.06)' },
            beginAtZero: true,
          },
        },
      },
    });
  }

  async function renderCumulativeChart() {
    cumulativeChart?.destroy();
    if (!cumulativeCanvas) return;

    const since = getSinceDate(selectedRange);
    const data: CumulativeViewsPoint[] = await api.getViewsPerPostCumulative(since);
    if (data.length === 0) return;

    const labels = data.map(p => formatLabel(p.date));
    const color = '#a855f7';

    cumulativeChart = new Chart(cumulativeCanvas, {
      type: 'line',
      data: {
        labels,
        datasets: [{
          label: 'Cumulative Views (per-post)',
          data: data.map(p => p.cumulative_views),
          borderColor: color,
          backgroundColor: (ctx: { chart: Chart }) => {
            const gradient = ctx.chart.ctx.createLinearGradient(0, 0, 0, ctx.chart.height);
            gradient.addColorStop(0, color + '40');
            gradient.addColorStop(1, color + '00');
            return gradient;
          },
          fill: true,
          cubicInterpolationMode: 'monotone' as const,
          borderWidth: 2.5,
          pointRadius: data.length > 60 ? 0 : 3,
          pointHoverRadius: 6,
          pointBackgroundColor: color,
          pointBorderColor: '#111',
          pointBorderWidth: 2,
        }],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { intersect: false, mode: 'index' as const },
        plugins: { legend: { display: false }, tooltip: darkTooltip },
        scales: {
          x: {
            ticks: { color: '#666', maxRotation: 45, maxTicksLimit: 12, font: { size: 11 } },
            grid: { color: 'rgba(255,255,255,0.04)' },
          },
          y: {
            ticks: { color: '#666', font: { size: 11 } },
            grid: { color: 'rgba(255,255,255,0.06)' },
            beginAtZero: true,
          },
        },
      },
    });
  }

  async function loadViews() {
    const since = getSinceDate(selectedRange);
    if (since) {
      const sinceDate = since.slice(0, 10);
      viewsData = allViewsData.filter(p => p.date >= sinceDate);
    } else {
      viewsData = allViewsData;
    }
    await tick();
    renderViewsChart();
    renderCumulativeChart();
  }

  async function changeRange(key: string) {
    selectedRange = key;
    await loadViews();
  }

  onMount(async () => {
    try {
      [allViewsData, rangeSums] = await Promise.all([
        api.getViewsPerPost(),
        api.getViewsPerPostRangeSums().then(r => r.sums),
      ]);
      loading = false;
      await tick();
      await loadViews();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load data';
      loading = false;
    }
  });

  onDestroy(() => {
    viewsChart?.destroy();
    cumulativeChart?.destroy();
  });
</script>

<div class="v2-page">
  {#if loading}
    <div class="status">Loading...</div>
  {:else if error}
    <div class="status error">{error}</div>
  {:else}
    <div class="header">
      <h2>Analytics V2 <span class="subtitle">per-post snapshot deltas</span></h2>
    </div>

    <!-- Views Over Time -->
    <div class="card">
      <div class="card-header">
        <h3>Views Over Time</h3>
        <select class="grouping-select" bind:value={viewsGrouping} onchange={() => { renderViewsChart(); }}>
          {#each viewsGroupingOptions as opt}
            <option value={opt.value}>{opt.label}</option>
          {/each}
        </select>
      </div>
      <div class="range-buttons">
        {#each timeRanges as r}
          <button
            class:active={selectedRange === r.key}
            onclick={() => changeRange(r.key)}
          >{r.label}({formatCount(rangeSums[r.key] ?? 0)})</button>
        {/each}
      </div>
      <div class="chart-wrap"><canvas bind:this={viewsCanvas}></canvas></div>
    </div>

    <!-- Cumulative Views -->
    <div class="card">
      <div class="card-header">
        <h3>Cumulative Views</h3>
      </div>
      <div class="chart-wrap"><canvas bind:this={cumulativeCanvas}></canvas></div>
    </div>
  {/if}
</div>

<style>
  .v2-page {
    padding: 1.5rem;
    max-width: 1200px;
    margin: 0 auto;
  }
  .status {
    text-align: center;
    color: #888;
    padding: 4rem 1rem;
  }
  .status.error { color: #f87171; }
  .header {
    margin-bottom: 1.5rem;
  }
  .header h2 {
    margin: 0;
    font-size: 1.2rem;
    color: #eee;
    font-weight: 600;
  }
  .subtitle {
    color: #666;
    font-weight: 400;
    font-size: 0.85rem;
  }
  .card {
    background: #111;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 1rem;
    margin-bottom: 1.5rem;
  }
  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.75rem;
  }
  .card-header h3 {
    margin: 0;
    font-size: 0.9rem;
    color: #aaa;
    font-weight: 500;
  }
  .grouping-select {
    background: #222;
    color: #ccc;
    border: 1px solid #333;
    border-radius: 4px;
    padding: 0.25rem 0.5rem;
    font-size: 0.75rem;
    cursor: pointer;
  }
  .grouping-select:hover { border-color: #555; }
  .range-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 0.75rem;
  }
  .range-buttons button {
    background: #1a1a1a;
    color: #999;
    border: 1px solid #333;
    border-radius: 4px;
    padding: 0.3rem 0.6rem;
    font-size: 0.7rem;
    cursor: pointer;
    white-space: nowrap;
  }
  .range-buttons button:hover { border-color: #555; color: #ccc; }
  .range-buttons button.active {
    background: #1d4ed8;
    color: #fff;
    border-color: #1d4ed8;
  }
  .chart-wrap {
    position: relative;
    height: 300px;
  }

  @media (max-width: 768px) {
    .v2-page { padding: 0.75rem; }
    .range-buttons button { font-size: 0.65rem; padding: 0.2rem 0.4rem; }
  }
</style>
```

- [ ] **Step 2: Create the page wrapper**

Create `web/src/routes/analytics-v2/+page.svelte`:

```svelte
<script lang="ts">
  import AnalyticsV2 from '$lib/components/AnalyticsV2.svelte';
</script>

<AnalyticsV2 />
```

- [ ] **Step 3: Add to nav bar**

In `web/src/routes/+layout.svelte`, add the V2 link after Analytics:

Change:
```svelte
      <a href="/analytics" class:active={$page.url.pathname === '/analytics'}>Analytics</a>
      <a href="/fourier" class:active={$page.url.pathname === '/fourier'}>ƒ(t)</a>
```

To:
```svelte
      <a href="/analytics" class:active={$page.url.pathname === '/analytics'}>Analytics</a>
      <a href="/analytics-v2" class:active={$page.url.pathname === '/analytics-v2'}>V2</a>
      <a href="/fourier" class:active={$page.url.pathname === '/fourier'}>ƒ(t)</a>
```

- [ ] **Step 4: Verify**

Run: `cd web && npx svelte-check && npm run build`
Expected: no new errors, build succeeds

- [ ] **Step 5: Commit**

```bash
git add web/src/lib/components/AnalyticsV2.svelte web/src/routes/analytics-v2/+page.svelte web/src/routes/+layout.svelte
git commit -m "feat: add Analytics V2 page with per-post views charts"
```

---

### Task 4: Verify end-to-end

- [ ] **Step 1: Backend checks**

Run: `cargo check --workspace && cargo clippy --workspace --all-targets && cargo fmt --all --check`
Fix any fmt issues with `cargo fmt --all`.

- [ ] **Step 2: Frontend checks**

Run: `cd web && npx svelte-check && npm run build`
Expected: no new errors, build succeeds

- [ ] **Step 3: Commit any fixes**

```bash
git add -A
git commit -m "chore: fix lint/format issues from analytics v2"
```
