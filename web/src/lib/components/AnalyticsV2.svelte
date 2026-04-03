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
