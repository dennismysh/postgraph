# Fourier Data Integrity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the Fourier page to use honest data sources (daily_views, engagement_snapshots deltas, post counts) instead of fabricated time series.

**Architecture:** Add one new backend endpoint for engagement daily deltas. Rewrite the frontend Fourier component to fetch from three honest sources: existing views endpoint, new engagement deltas endpoint, and posts list for cadence. Keep FFT math and chart infrastructure unchanged.

**Tech Stack:** Rust/axum (backend endpoint), Svelte 5 + Chart.js (frontend), TypeScript

**Spec:** `docs/superpowers/specs/2026-04-01-fourier-data-integrity-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `postgraph-server/src/routes/analytics.rs` | Modify | Add `DailyEngagementDelta` type + `get_engagement_daily_deltas` handler |
| `postgraph-server/src/main.rs` | Modify | Register new route |
| `web/src/routes/api/analytics/engagement/daily-deltas/+server.ts` | Create | SvelteKit proxy to backend |
| `web/src/lib/api.ts` | Modify | Add `DailyEngagementDelta` interface + `getEngagementDailyDeltas` function |
| `web/src/lib/fourier.ts` | Rewrite | Remove `postsToDaily`, add `postsToCadence` |
| `web/src/lib/components/Fourier.svelte` | Rewrite | New data flow: 3 API calls, 4 chart sections |

**Unchanged:** `web/src/lib/fft.ts`, `web/src/routes/fourier/+page.svelte`

---

### Task 1: Backend — Add engagement daily deltas endpoint

**Files:**
- Modify: `postgraph-server/src/routes/analytics.rs`
- Modify: `postgraph-server/src/main.rs`

This follows the existing pattern in analytics.rs: local types + inline SQL in the handler.

- [ ] **Step 1: Add the response type**

Add after the `HistogramQuery` struct (line 106) in `postgraph-server/src/routes/analytics.rs`:

```rust
#[derive(Serialize)]
pub struct DailyEngagementDelta {
    pub date: String,
    pub likes: i64,
    pub replies: i64,
    pub reposts: i64,
    pub quotes: i64,
}
```

- [ ] **Step 2: Add the handler function**

Add after the `get_views_range_sums` function (after line 300) in `postgraph-server/src/routes/analytics.rs`:

```rust
// ── Engagement Daily Deltas (for Fourier analysis) ─────────────────

pub async fn get_engagement_daily_deltas(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<DailyEngagementDelta>>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.date_naive());

    let rows: Vec<(String, i64, i64, i64, i64)> = sqlx::query_as(
        r#"WITH daily_snapshots AS (
               SELECT DISTINCT ON (post_id, captured_at::date)
                   post_id,
                   captured_at::date AS capture_date,
                   likes, replies_count, reposts, quotes
               FROM engagement_snapshots
               ORDER BY post_id, captured_at::date, captured_at DESC
           ),
           deltas AS (
               SELECT
                   capture_date,
                   likes - LAG(likes) OVER w AS d_likes,
                   replies_count - LAG(replies_count) OVER w AS d_replies,
                   reposts - LAG(reposts) OVER w AS d_reposts,
                   quotes - LAG(quotes) OVER w AS d_quotes
               FROM daily_snapshots
               WINDOW w AS (PARTITION BY post_id ORDER BY capture_date)
           )
           SELECT
               capture_date::text AS date,
               COALESCE(SUM(d_likes), 0)::bigint AS likes,
               COALESCE(SUM(d_replies), 0)::bigint AS replies,
               COALESCE(SUM(d_reposts), 0)::bigint AS reposts,
               COALESCE(SUM(d_quotes), 0)::bigint AS quotes
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

    let points: Vec<DailyEngagementDelta> = rows
        .into_iter()
        .map(|(date, likes, replies, reposts, quotes)| DailyEngagementDelta {
            date,
            likes,
            replies,
            reposts,
            quotes,
        })
        .collect();

    Ok(Json(points))
}
```

Note: Uses `replies_count` (not `replies`) to match the actual column name in `engagement_snapshots`. Uses `LAG()` instead of `MAX()` for true previous-value deltas. Does not clamp with `GREATEST()` — negative deltas from API corrections are kept honest.

- [ ] **Step 3: Register the route**

In `postgraph-server/src/main.rs`, add after the `.route("/api/analytics/engagement", ...)` block (after line 271):

```rust
        .route(
            "/api/analytics/engagement/daily-deltas",
            get(routes::analytics::get_engagement_daily_deltas),
        )
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/routes/analytics.rs postgraph-server/src/main.rs
git commit -m "feat: add engagement daily deltas endpoint for Fourier page"
```

---

### Task 2: Frontend — Add proxy route and API client function

**Files:**
- Create: `web/src/routes/api/analytics/engagement/daily-deltas/+server.ts`
- Modify: `web/src/lib/api.ts`

- [ ] **Step 1: Create SvelteKit proxy route**

Create `web/src/routes/api/analytics/engagement/daily-deltas/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const since = url.searchParams.get('since');
  const searchParams = new URLSearchParams();
  if (since) searchParams.set('since', since);
  return proxyToBackend('/api/analytics/engagement/daily-deltas', { searchParams });
};
```

This follows the exact same pattern as the existing `web/src/routes/api/analytics/engagement/+server.ts`.

- [ ] **Step 2: Add TypeScript interface to api.ts**

In `web/src/lib/api.ts`, add after the `PostEngagementPoint` interface (after line 159):

```typescript
export interface DailyEngagementDelta {
  date: string;
  likes: number;
  replies: number;
  reposts: number;
  quotes: number;
}
```

- [ ] **Step 3: Add API client function**

In `web/src/lib/api.ts`, add inside the `api` object (after the `getEngagement` method, after line 259):

```typescript
  getEngagementDailyDeltas: (since?: string) => {
    const params = new URLSearchParams();
    if (since) params.set('since', since);
    const qs = params.toString();
    return fetchApi<DailyEngagementDelta[]>(`/api/analytics/engagement/daily-deltas${qs ? `?${qs}` : ''}`);
  },
```

- [ ] **Step 4: Verify frontend compiles**

Run: `cd web && npx svelte-check`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add web/src/routes/api/analytics/engagement/daily-deltas/+server.ts web/src/lib/api.ts
git commit -m "feat: add engagement daily deltas proxy route and API client"
```

---

### Task 3: Frontend — Rewrite fourier.ts

**Files:**
- Modify: `web/src/lib/fourier.ts`

Replace the fabricated `postsToDaily` with an honest `postsToCadence` that only counts posts per day. Keep `postsToHourly`, `computeSpectrum`, `computeSmoothed`, `topPeaks` unchanged.

- [ ] **Step 1: Rewrite fourier.ts**

Replace the entire contents of `web/src/lib/fourier.ts` with:

```typescript
import type { Post } from '$lib/api';
import { fft, ifft, powerSpectrum, lowPassFilter, type SpectrumEntry } from '$lib/fft';

export type CadenceEntry = {
  date: string;
  posts: number;
};

export type HourlyEntry = {
  hour: number;
  count: number;
};

/** Count posts per day, gap-filled with honest zeros (no posts = 0 posts) */
export function postsToCadence(posts: Post[]): CadenceEntry[] {
  const map = new Map<string, number>();
  for (const p of posts) {
    const date = p.timestamp.slice(0, 10);
    map.set(date, (map.get(date) ?? 0) + 1);
  }
  const dates = [...map.keys()].sort();
  if (dates.length === 0) return [];

  const result: CadenceEntry[] = [];
  const start = new Date(dates[0]);
  const end = new Date(dates[dates.length - 1]);
  for (let d = new Date(start); d <= end; d.setDate(d.getDate() + 1)) {
    const key = d.toISOString().slice(0, 10);
    result.push({ date: key, posts: map.get(key) ?? 0 });
  }
  return result;
}

/** Aggregate posts into 24 hourly buckets */
export function postsToHourly(posts: Post[]): HourlyEntry[] {
  const buckets = Array.from({ length: 24 }, (_, i) => ({ hour: i, count: 0 }));
  for (const p of posts) {
    const hour = new Date(p.timestamp).getHours();
    buckets[hour].count += 1;
  }
  return buckets;
}

/** Compute power spectrum with DC removal and zero-padding */
export function computeSpectrum(signal: number[]): SpectrumEntry[] {
  const totalDays = signal.length;
  const mean = signal.reduce((a, b) => a + b, 0) / totalDays;
  const centered = signal.map(v => v - mean);

  let N = 1;
  while (N < totalDays) N <<= 1;
  const re = new Array(N).fill(0);
  const im = new Array(N).fill(0);
  for (let i = 0; i < totalDays; i++) re[i] = centered[i];

  fft(re, im);
  return powerSpectrum(re, im, totalDays);
}

/** Compute smoothed trend via low-pass filter */
export function computeSmoothed(signal: number[], cutoffRatio = 0.06): number[] {
  return lowPassFilter(signal, cutoffRatio);
}

/** Return top n spectrum entries by magnitude */
export function topPeaks(spectrum: SpectrumEntry[], n: number): SpectrumEntry[] {
  return [...spectrum].sort((a, b) => b.magnitude - a.magnitude).slice(0, n);
}

export type { SpectrumEntry };
```

Key changes:
- Removed `DailyEntry` type and `postsToDaily()` (the fabrication function)
- Added `CadenceEntry` type and `postsToCadence()` that only counts posts per day
- Gap-filling with 0 is honest for posting cadence: no posts that day means 0 posts
- All other functions unchanged

- [ ] **Step 2: Verify frontend compiles**

Run: `cd web && npx svelte-check`
Expected: errors in `Fourier.svelte` (references to removed `postsToDaily` and `DailyEntry`). This is expected — we fix it in Task 4.

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/fourier.ts
git commit -m "refactor: replace fabricated postsToDaily with honest postsToCadence"
```

---

### Task 4: Frontend — Rewrite Fourier.svelte

**Files:**
- Modify: `web/src/lib/components/Fourier.svelte`

Complete rewrite of the data flow. The chart infrastructure (Chart.js config, dark theme, peak annotations, styling) stays. What changes: data fetching, chart wiring, stats ribbon.

- [ ] **Step 1: Rewrite Fourier.svelte**

Replace the entire `<script>` block and template of `web/src/lib/components/Fourier.svelte`. Keep the `<style>` block unchanged.

```svelte
<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import Chart from 'chart.js/auto';
  import { api, type Post, type ViewsPoint, type DailyEngagementDelta } from '$lib/api';
  import {
    postsToCadence, postsToHourly, computeSpectrum, computeSmoothed, topPeaks,
    type CadenceEntry, type HourlyEntry, type SpectrumEntry,
  } from '$lib/fourier';

  let loading = $state(true);
  let error: string | null = $state(null);
  let hasEnoughData = $state(false);

  // Raw data from API
  let allViews: ViewsPoint[] = [];
  let allDeltas: DailyEngagementDelta[] = [];
  let allPosts: Post[] = [];

  // Stats ribbon
  let dominantViewsCycle = $state('—');
  let dominantCadenceCycle = $state('—');
  let avgPostsPerDay = $state('0');
  let totalViews = $state(0);

  // Per-card time range state
  const timeRanges = [
    { label: 'Last 7 Days', value: '7d' },
    { label: 'Last 14 Days', value: '14d' },
    { label: 'Last 30 Days', value: '30d' },
    { label: 'Last 60 Days', value: '60d' },
    { label: 'Last 90 Days', value: '90d' },
    { label: 'Last 180 Days', value: '180d' },
    { label: 'Last 365 Days', value: '365d' },
    { label: 'All Time', value: 'all' },
  ];

  let viewsRange = $state('90d');
  let engagementRange = $state('90d');
  let cadenceRange = $state('90d');
  let hourlyRange = $state('30d');

  // Hourly data for bucket stats
  let hourly: HourlyEntry[] = $state([]);

  // Canvas refs
  let viewsCanvas: HTMLCanvasElement = $state(null!);
  let viewsSpectrumCanvas: HTMLCanvasElement = $state(null!);
  let engagementCanvas: HTMLCanvasElement = $state(null!);
  let engagementSpectrumCanvas: HTMLCanvasElement = $state(null!);
  let cadenceCanvas: HTMLCanvasElement = $state(null!);
  let cadenceSpectrumCanvas: HTMLCanvasElement = $state(null!);
  let hourlyCanvas: HTMLCanvasElement = $state(null!);

  // Chart instances
  let viewsChart: Chart | null = null;
  let viewsSpectrumChart: Chart | null = null;
  let engagementChart: Chart | null = null;
  let engagementSpectrumChart: Chart | null = null;
  let cadenceChart: Chart | null = null;
  let cadenceSpectrumChart: Chart | null = null;
  let hourlyChart: Chart | null = null;

  const darkTooltip = {
    backgroundColor: '#1a1a1a',
    borderColor: '#333',
    borderWidth: 1,
    titleColor: '#ccc',
    bodyColor: '#eee',
    padding: 10,
    displayColors: false,
  };

  const darkScaleX = {
    ticks: { color: '#666', maxRotation: 45, maxTicksLimit: 8, font: { size: 11 } },
    grid: { color: 'rgba(255,255,255,0.04)' },
  };

  const darkScaleY = {
    ticks: { color: '#666', font: { size: 11 } },
    grid: { color: 'rgba(255,255,255,0.06)' },
    beginAtZero: true,
  };

  function peakAnnotationPlugin(peaks: SpectrumEntry[], color: string) {
    return {
      id: 'peakAnnotations',
      afterDraw(chart: Chart) {
        const { ctx, scales } = chart;
        const xScale = scales['x'];
        const yScale = scales['y'];
        if (!xScale || !yScale) return;
        ctx.save();
        for (const peak of peaks) {
          const labels = chart.data.labels as string[];
          const idx = labels.indexOf(peak.period);
          if (idx === -1) continue;
          const x = xScale.getPixelForValue(idx);
          const yTop = yScale.getPixelForValue(peak.magnitude);
          const yBottom = yScale.getPixelForValue(0);
          ctx.strokeStyle = color;
          ctx.lineWidth = 1.5;
          ctx.setLineDash([4, 3]);
          ctx.beginPath();
          ctx.moveTo(x, yTop - 8);
          ctx.lineTo(x, yBottom);
          ctx.stroke();
          ctx.setLineDash([]);
          ctx.fillStyle = color;
          ctx.font = '11px -apple-system, sans-serif';
          ctx.textAlign = 'center';
          ctx.fillText(`${peak.period}d`, x, yTop - 12);
        }
        ctx.restore();
      },
    };
  }

  function formatDate(d: string): string {
    return d.slice(5); // MM-DD
  }

  function filterByRange<T extends { date: string }>(data: T[], range: string): T[] {
    if (range === 'all') return data;
    const days = parseInt(range);
    const cutoff = new Date();
    cutoff.setDate(cutoff.getDate() - days);
    const cutoffStr = cutoff.toISOString().slice(0, 10);
    return data.filter(d => d.date >= cutoffStr);
  }

  function filterPostsByRange(posts: Post[], range: string): Post[] {
    if (range === 'all') return posts;
    const days = parseInt(range);
    const cutoff = new Date();
    cutoff.setDate(cutoff.getDate() - days);
    return posts.filter(p => new Date(p.timestamp) >= cutoff);
  }

  // ── Chart 1: Daily Views ──────────────────────────────────────────

  async function rebuildViewsChart() {
    await tick();
    viewsChart?.destroy();
    viewsSpectrumChart?.destroy();
    const filtered = filterByRange(allViews, viewsRange);
    if (filtered.length < 2) { viewsChart = null; viewsSpectrumChart = null; return; }

    const signal = filtered.map(d => d.views);
    const smoothed = computeSmoothed(signal);

    viewsChart = new Chart(viewsCanvas, {
      type: 'line',
      data: {
        labels: filtered.map(d => formatDate(d.date)),
        datasets: [
          {
            label: 'Views',
            data: signal,
            borderColor: '#8b5cf6',
            backgroundColor: (ctx: { chart: Chart }) => {
              const gradient = ctx.chart.ctx.createLinearGradient(0, 0, 0, ctx.chart.height);
              gradient.addColorStop(0, 'rgba(139,92,246,0.35)');
              gradient.addColorStop(1, 'rgba(139,92,246,0)');
              return gradient;
            },
            fill: true,
            borderWidth: 1.5,
            pointRadius: filtered.length > 30 ? 0 : 2,
            pointHoverRadius: 4,
            cubicInterpolationMode: 'monotone' as const,
          },
          {
            label: 'Trend',
            data: smoothed,
            borderColor: '#f59e0b',
            borderWidth: 2.5,
            pointRadius: 0,
            pointHoverRadius: 0,
            fill: false,
            cubicInterpolationMode: 'monotone' as const,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { intersect: false, mode: 'index' as const },
        plugins: {
          legend: { labels: { color: '#ccc', boxWidth: 12, font: { size: 11 } } },
          tooltip: darkTooltip,
        },
        scales: { x: darkScaleX, y: darkScaleY },
      },
    });

    // Spectrum
    if (filtered.length >= 8) {
      const spectrum = computeSpectrum(signal);
      const peaks = topPeaks(spectrum, 2);
      const filteredSpectrum = spectrum.filter(s => parseFloat(s.period) <= 60);

      viewsSpectrumChart = new Chart(viewsSpectrumCanvas, {
        type: 'bar',
        data: {
          labels: filteredSpectrum.map(s => s.period),
          datasets: [{
            label: 'Magnitude',
            data: filteredSpectrum.map(s => s.magnitude),
            backgroundColor: 'rgba(139,92,246,0.6)',
            borderColor: '#8b5cf6',
            borderWidth: 1,
          }],
        },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          interaction: { intersect: false, mode: 'index' as const },
          plugins: { legend: { display: false }, tooltip: darkTooltip },
          scales: {
            x: { ...darkScaleX, title: { display: true, text: 'period (days)', color: '#888', font: { size: 11 } } },
            y: darkScaleY,
          },
        },
        plugins: [peakAnnotationPlugin(peaks, '#f59e0b')],
      });
    }
  }

  // ── Chart 2: Engagement Velocity ──────────────────────────────────

  async function rebuildEngagementChart() {
    await tick();
    engagementChart?.destroy();
    engagementSpectrumChart?.destroy();
    const filtered = filterByRange(allDeltas, engagementRange);
    if (filtered.length < 2) { engagementChart = null; engagementSpectrumChart = null; return; }

    engagementChart = new Chart(engagementCanvas, {
      type: 'line',
      data: {
        labels: filtered.map(d => formatDate(d.date)),
        datasets: [
          {
            label: 'Likes',
            data: filtered.map(d => d.likes),
            borderColor: '#f472b6',
            borderWidth: 1.5,
            pointRadius: filtered.length > 30 ? 0 : 2,
            pointHoverRadius: 4,
            fill: false,
            cubicInterpolationMode: 'monotone' as const,
          },
          {
            label: 'Replies',
            data: filtered.map(d => d.replies),
            borderColor: '#60a5fa',
            borderWidth: 1.5,
            pointRadius: filtered.length > 30 ? 0 : 2,
            pointHoverRadius: 4,
            fill: false,
            cubicInterpolationMode: 'monotone' as const,
          },
          {
            label: 'Reposts',
            data: filtered.map(d => d.reposts),
            borderColor: '#4ade80',
            borderWidth: 1.5,
            pointRadius: filtered.length > 30 ? 0 : 2,
            pointHoverRadius: 4,
            fill: false,
            cubicInterpolationMode: 'monotone' as const,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { intersect: false, mode: 'index' as const },
        plugins: {
          legend: { labels: { color: '#ccc', boxWidth: 12, font: { size: 11 } } },
          tooltip: darkTooltip,
        },
        scales: { x: darkScaleX, y: darkScaleY },
      },
    });

    // Spectrum on combined engagement signal (likes + replies + reposts)
    if (filtered.length >= 8) {
      const combined = filtered.map(d => d.likes + d.replies + d.reposts);
      const spectrum = computeSpectrum(combined);
      const peaks = topPeaks(spectrum, 2);
      const filteredSpectrum = spectrum.filter(s => parseFloat(s.period) <= 60);

      engagementSpectrumChart = new Chart(engagementSpectrumCanvas, {
        type: 'bar',
        data: {
          labels: filteredSpectrum.map(s => s.period),
          datasets: [{
            label: 'Magnitude',
            data: filteredSpectrum.map(s => s.magnitude),
            backgroundColor: 'rgba(244,114,182,0.6)',
            borderColor: '#f472b6',
            borderWidth: 1,
          }],
        },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          interaction: { intersect: false, mode: 'index' as const },
          plugins: { legend: { display: false }, tooltip: darkTooltip },
          scales: {
            x: { ...darkScaleX, title: { display: true, text: 'period (days)', color: '#888', font: { size: 11 } } },
            y: darkScaleY,
          },
        },
        plugins: [peakAnnotationPlugin(peaks, '#f59e0b')],
      });
    }
  }

  // ── Chart 3: Posting Cadence + Spectrum ────────────────────────────

  async function rebuildCadenceChart() {
    await tick();
    cadenceChart?.destroy();
    cadenceSpectrumChart?.destroy();
    const filtered = filterPostsByRange(allPosts, cadenceRange);
    const cadence = postsToCadence(filtered);
    if (cadence.length < 2) { cadenceChart = null; cadenceSpectrumChart = null; return; }

    const signal = cadence.map(d => d.posts);
    const smoothed = computeSmoothed(signal);

    cadenceChart = new Chart(cadenceCanvas, {
      type: 'bar',
      data: {
        labels: cadence.map(d => formatDate(d.date)),
        datasets: [
          {
            label: 'Posts',
            data: signal,
            backgroundColor: 'rgba(67,99,216,0.6)',
            borderColor: '#4363d8',
            borderWidth: 1,
          },
          {
            label: 'Trend',
            data: smoothed,
            type: 'line',
            borderColor: '#f59e0b',
            borderWidth: 2.5,
            pointRadius: 0,
            pointHoverRadius: 0,
            fill: false,
            cubicInterpolationMode: 'monotone' as const,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { intersect: false, mode: 'index' as const },
        plugins: {
          legend: { labels: { color: '#ccc', boxWidth: 12, font: { size: 11 } } },
          tooltip: darkTooltip,
        },
        scales: {
          x: darkScaleX,
          y: { ...darkScaleY, ticks: { ...darkScaleY.ticks, stepSize: 1 } },
        },
      },
    });

    // Spectrum
    if (cadence.length >= 8) {
      const spectrum = computeSpectrum(signal);
      const peaks = topPeaks(spectrum, 2);
      const filteredSpectrum = spectrum.filter(s => parseFloat(s.period) <= 60);

      cadenceSpectrumChart = new Chart(cadenceSpectrumCanvas, {
        type: 'bar',
        data: {
          labels: filteredSpectrum.map(s => s.period),
          datasets: [{
            label: 'Magnitude',
            data: filteredSpectrum.map(s => s.magnitude),
            backgroundColor: 'rgba(67,99,216,0.6)',
            borderColor: '#4363d8',
            borderWidth: 1,
          }],
        },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          interaction: { intersect: false, mode: 'index' as const },
          plugins: { legend: { display: false }, tooltip: darkTooltip },
          scales: {
            x: { ...darkScaleX, title: { display: true, text: 'period (days)', color: '#888', font: { size: 11 } } },
            y: darkScaleY,
          },
        },
        plugins: [peakAnnotationPlugin(peaks, '#f59e0b')],
      });
    }
  }

  // ── Chart 4: Hourly Distribution (unchanged logic) ────────────────

  async function rebuildHourlyChart() {
    await tick();
    hourlyChart?.destroy();
    const filtered = filterPostsByRange(allPosts, hourlyRange);
    hourly = postsToHourly(filtered);

    hourlyChart = new Chart(hourlyCanvas, {
      type: 'bar',
      data: {
        labels: hourly.map(h => `${h.hour}:00`),
        datasets: [{
          label: 'Posts',
          data: hourly.map(h => h.count),
          backgroundColor: 'rgba(74,222,128,0.5)',
          borderColor: '#4ade80',
          borderWidth: 1,
        }],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { intersect: false, mode: 'index' as const },
        plugins: { legend: { display: false }, tooltip: darkTooltip },
        scales: {
          x: { ticks: { color: '#666', maxRotation: 0, font: { size: 11 }, maxTicksLimit: 24 }, grid: { color: 'rgba(255,255,255,0.04)' } },
          y: { ...darkScaleY, ticks: { ...darkScaleY.ticks, stepSize: 1 } },
        },
      },
    });
  }

  // ── Init ──────────────────────────────────────────────────────────

  onMount(async () => {
    try {
      [allViews, allDeltas, allPosts] = await Promise.all([
        api.getViews(),
        api.getEngagementDailyDeltas(),
        api.getPosts(),
      ]);

      // Stats ribbon
      totalViews = allViews.reduce((s, d) => s + d.views, 0);
      const allCadence = postsToCadence(allPosts);
      const totalPostsCount = allCadence.reduce((s, d) => s + d.posts, 0);
      avgPostsPerDay = allCadence.length > 0 ? (totalPostsCount / allCadence.length).toFixed(1) : '0';

      hasEnoughData = allViews.length >= 8 || allCadence.length >= 8;

      if (hasEnoughData) {
        if (allViews.length >= 8) {
          const viewsSpectrum = computeSpectrum(allViews.map(d => d.views));
          const viewsPeaks = topPeaks(viewsSpectrum, 1);
          dominantViewsCycle = viewsPeaks.length > 0 ? `${viewsPeaks[0].period}d` : '—';
        }
        if (allCadence.length >= 8) {
          const cadenceSpectrum = computeSpectrum(allCadence.map(d => d.posts));
          const cadPeaks = topPeaks(cadenceSpectrum, 1);
          dominantCadenceCycle = cadPeaks.length > 0 ? `${cadPeaks[0].period}d` : '—';
        }

        loading = false;
        await Promise.all([
          rebuildViewsChart(),
          rebuildEngagementChart(),
          rebuildCadenceChart(),
          rebuildHourlyChart(),
        ]);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load data';
    } finally {
      loading = false;
    }
  });

  onDestroy(() => {
    viewsChart?.destroy();
    viewsSpectrumChart?.destroy();
    engagementChart?.destroy();
    engagementSpectrumChart?.destroy();
    cadenceChart?.destroy();
    cadenceSpectrumChart?.destroy();
    hourlyChart?.destroy();
  });

  function bucketCount(from: number, to: number): number {
    return hourly.slice(from, to + 1).reduce((s, h) => s + h.count, 0);
  }
</script>

<div class="fourier-page">
  {#if loading}
    <div class="status">Loading...</div>
  {:else if error}
    <div class="status error">{error}</div>
  {:else if !hasEnoughData}
    <div class="status">Not enough data for Fourier analysis (need at least 8 days).</div>
  {:else}
    <!-- Stats Ribbon -->
    <div class="ribbon">
      <div class="kpi">
        <span class="kpi-label">Views Cycle</span>
        <span class="kpi-value">{dominantViewsCycle}</span>
      </div>
      <div class="kpi">
        <span class="kpi-label">Posting Cycle</span>
        <span class="kpi-value">{dominantCadenceCycle}</span>
      </div>
      <div class="kpi">
        <span class="kpi-label">Total Views</span>
        <span class="kpi-value">{totalViews.toLocaleString()}</span>
      </div>
      <div class="kpi">
        <span class="kpi-label">Avg Posts/Day</span>
        <span class="kpi-value">{avgPostsPerDay}</span>
      </div>
    </div>

    <!-- Chart Grid -->
    <div class="grid">
      <!-- Daily Views + Spectrum -->
      <div class="chart-card">
        <div class="chart-header">
          <h3>Daily Views</h3>
          <select class="range-select" bind:value={viewsRange} onchange={() => rebuildViewsChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-wrap"><canvas bind:this={viewsCanvas}></canvas></div>
        <div class="spectrum-wrap"><canvas bind:this={viewsSpectrumCanvas}></canvas></div>
      </div>

      <!-- Engagement Velocity + Spectrum -->
      <div class="chart-card">
        <div class="chart-header">
          <h3>Engagement Velocity</h3>
          <select class="range-select" bind:value={engagementRange} onchange={() => rebuildEngagementChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-wrap"><canvas bind:this={engagementCanvas}></canvas></div>
        <div class="spectrum-wrap"><canvas bind:this={engagementSpectrumCanvas}></canvas></div>
      </div>

      <!-- Posting Cadence + Spectrum -->
      <div class="chart-card full-width">
        <div class="chart-header">
          <h3>Posting Cadence</h3>
          <select class="range-select" bind:value={cadenceRange} onchange={() => rebuildCadenceChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-wrap"><canvas bind:this={cadenceCanvas}></canvas></div>
        <div class="spectrum-wrap"><canvas bind:this={cadenceSpectrumCanvas}></canvas></div>
      </div>

      <!-- Hourly Distribution (full width) -->
      <div class="chart-card full-width">
        <div class="chart-header">
          <h3>Posting Hour Distribution</h3>
          <select class="range-select" bind:value={hourlyRange} onchange={() => rebuildHourlyChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-wrap"><canvas bind:this={hourlyCanvas}></canvas></div>
        <div class="hour-stats">
          <span>Morning (6-11): <strong>{bucketCount(6, 11)}</strong></span>
          <span>Evening (18-23): <strong>{bucketCount(18, 23)}</strong></span>
          <span>Other: <strong>{bucketCount(0, 5) + bucketCount(12, 17)}</strong></span>
        </div>
      </div>
    </div>
  {/if}
</div>
```

- [ ] **Step 2: Add spectrum-wrap CSS**

Add to the existing `<style>` block in Fourier.svelte, after the `.chart-wrap` rule:

```css
  .spectrum-wrap {
    position: relative;
    height: 160px;
    margin-top: 0.75rem;
    border-top: 1px solid #222;
    padding-top: 0.75rem;
  }
```

- [ ] **Step 3: Verify frontend compiles**

Run: `cd web && npx svelte-check`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add web/src/lib/components/Fourier.svelte
git commit -m "feat: rewrite Fourier page for honest data sources (views, engagement deltas, cadence)"
```

---

### Task 5: Verify end-to-end

- [ ] **Step 1: Backend compiles**

Run: `cargo check --workspace`
Expected: compiles cleanly

- [ ] **Step 2: Frontend compiles**

Run: `cd web && npx svelte-check`
Expected: no type errors

- [ ] **Step 3: Frontend builds**

Run: `cd web && npm run build`
Expected: builds successfully

- [ ] **Step 4: Cargo clippy**

Run: `cargo clippy --workspace --all-targets`
Expected: no warnings

- [ ] **Step 5: Cargo fmt**

Run: `cargo fmt --all --check`
Expected: no formatting issues (or run `cargo fmt --all` to fix)

- [ ] **Step 6: Commit any final fixes**

If any of the above checks produced issues, fix and commit:

```bash
git add -A
git commit -m "chore: fix lint/format issues from Fourier rewrite"
```
