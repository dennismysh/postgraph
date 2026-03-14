<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import Chart from 'chart.js/auto';
  import { api, type AnalyticsData, type AnalyzeStatus, type ViewsPoint, type Post, type SyncStatus } from '$lib/api';

  let analytics: AnalyticsData | null = $state(null);
  let engagementCanvas: HTMLCanvasElement = $state(null!);
  let topicsCanvas: HTMLCanvasElement = $state(null!);
  let viewsCanvas: HTMLCanvasElement = $state(null!);
  let engagementChart: Chart | null = $state(null);
  let topicsChart: Chart | null = $state(null);
  let viewsChart: Chart | null = $state(null);
  let viewsData: ViewsPoint[] = $state([]);
  let recentPosts: Post[] = $state([]);
  let analyzeStatus: AnalyzeStatus | null = $state(null);
  let analyzing = $state(false);
  let syncing = $state(false);
  let syncStatus = $state('');
  let syncStatusData: SyncStatus | null = $state(null);
  let statusInterval: ReturnType<typeof setInterval> | null = null;
  let syncInterval: ReturnType<typeof setInterval> | null = null;
  let allViewsData: ViewsPoint[] = $state([]);
  let rangeSums: Record<string, number> = $state({});

  const timeRanges = [
    { label: 'Last 24 Hours', value: '24h' },
    { label: 'Last 7 Days', value: '7d' },
    { label: 'Last 2 Weeks', value: '14d' },
    { label: 'Last 30 Days', value: '30d' },
    { label: 'Last 2 Months', value: '60d' },
    { label: 'Last 3 Months', value: '90d' },
    { label: 'Last 6 Months', value: '180d' },
    { label: 'Last 9 Months', value: '270d' },
    { label: 'Last 12 Months', value: '365d' },
    { label: 'All Time', value: 'all' },
  ];
  let selectedRange = $state('30d');

  function getSinceDate(range: string): string | undefined {
    if (range === 'all') return undefined;
    const now = new Date();
    if (range === '24h') {
      now.setHours(now.getHours() - 24);
    } else {
      const days = parseInt(range);
      now.setDate(now.getDate() - days);
    }
    return now.toISOString();
  }

  function getGrouping(range: string): 'hourly' | 'daily' | 'weekly' {
    if (range === '24h') return 'hourly';
    if (range === '7d' || range === '14d') return 'daily';
    return 'weekly';
  }

  function getWeekStart(dateStr: string): string {
    const d = new Date(dateStr + 'T00:00:00');
    const day = d.getDay();
    d.setDate(d.getDate() - day); // Sunday start
    return d.toISOString().slice(0, 10);
  }

  function fillHourlyGaps(data: ViewsPoint[]): ViewsPoint[] {
    // Build a lookup from the backend data (format: "YYYY-MM-DD HH:00")
    const lookup = new Map<string, number>();
    for (const point of data) {
      lookup.set(point.date, point.views);
    }

    // Generate all 24 hours from now back
    const result: ViewsPoint[] = [];
    const now = new Date();
    now.setMinutes(0, 0, 0);
    for (let i = 23; i >= 0; i--) {
      const hour = new Date(now.getTime() - i * 3600_000);
      const key = hour.toISOString().slice(0, 13).replace('T', ' ') + ':00';
      const label = key.slice(5); // "MM-DD HH:00"
      result.push({ date: label, views: lookup.get(key) ?? 0 });
    }
    return result;
  }

  function fillDailyGaps(data: ViewsPoint[], range: string): ViewsPoint[] {
    const lookup = new Map<string, number>();
    for (const point of data) {
      lookup.set(point.date, point.views);
    }

    const days = parseInt(range);
    const result: ViewsPoint[] = [];
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    for (let i = days - 1; i >= 0; i--) {
      const d = new Date(today.getTime() - i * 86400_000);
      const key = d.toISOString().slice(0, 10);
      result.push({ date: key, views: lookup.get(key) ?? 0 });
    }
    return result;
  }

  function groupViewsData(data: ViewsPoint[], grouping: 'hourly' | 'daily' | 'weekly'): ViewsPoint[] {
    if (grouping === 'hourly') {
      return fillHourlyGaps(data);
    }

    if (grouping === 'daily') {
      return fillDailyGaps(data, selectedRange);
    }

    const grouped = new Map<string, number>();
    for (const point of data) {
      const key = getWeekStart(point.date);
      grouped.set(key, (grouped.get(key) ?? 0) + point.views);
    }

    return [...grouped.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([date, views]) => {
        // Format as "Week of Mar 1" for readability
        const d = new Date(date + 'T00:00:00');
        const label = `Week of ${d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}`;
        return { date: label, views };
      });
  }

  function computeRangeSums(allData: ViewsPoint[]) {
    const sums: Record<string, number> = {};
    for (const range of timeRanges) {
      const since = getSinceDate(range.value);
      if (!since) {
        sums[range.value] = allData.reduce((s, p) => s + p.views, 0);
      } else {
        const sinceDate = since.slice(0, 10);
        sums[range.value] = allData
          .filter(p => p.date >= sinceDate)
          .reduce((s, p) => s + p.views, 0);
      }
    }
    return sums;
  }

  function formatNum(n: number): string {
    if (n >= 1_000_000) return (n / 1_000_000).toFixed(1).replace(/\.0$/, '') + 'M';
    if (n >= 1_000) return (n / 1_000).toFixed(1).replace(/\.0$/, '') + 'K';
    return n.toLocaleString();
  }

  async function loadAllViews() {
    allViewsData = await api.getViews();
    rangeSums = computeRangeSums(allViewsData);
  }

  async function loadViews() {
    const grouping = getGrouping(selectedRange);

    if (grouping === 'hourly') {
      // Fetch hourly data from backend for 24h range
      const since = getSinceDate(selectedRange);
      viewsData = await api.getViews(since, 'hourly');
    } else {
      // Use cached daily data, filter client-side
      const since = getSinceDate(selectedRange);
      if (since) {
        const sinceDate = since.slice(0, 10);
        viewsData = allViewsData.filter(p => p.date >= sinceDate);
      } else {
        viewsData = allViewsData;
      }
    }
    await tick();
    renderViewsChart();
  }

  function renderViewsChart() {
    if (viewsChart) viewsChart.destroy();
    if (!viewsCanvas) return;

    const grouping = getGrouping(selectedRange);
    const chartData = groupViewsData(viewsData, grouping);

    viewsChart = new Chart(viewsCanvas, {
      type: 'line',
      data: {
        labels: chartData.map(v => v.date),
        datasets: [{
          label: 'Views',
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
        plugins: {
          legend: { display: false },
          tooltip: {
            backgroundColor: '#1a1a1a',
            borderColor: '#333',
            borderWidth: 1,
            titleColor: '#ccc',
            bodyColor: '#f58231',
            padding: 10,
            displayColors: false,
          },
        },
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

  function getUnanalyzedCount(): number {
    return analytics ? analytics.total_posts - analytics.analyzed_posts : 0;
  }

  function getProgressPercent(): number {
    if (analyzeStatus && analyzeStatus.total > 0) {
      return Math.round((analyzeStatus.analyzed / analyzeStatus.total) * 100);
    }
    return 0;
  }

  function getSyncProgressPercent(): number {
    if (syncStatusData && syncStatusData.total > 0) {
      return Math.round((syncStatusData.synced / syncStatusData.total) * 100);
    }
    return 0;
  }

  async function startAnalysis() {
    analyzing = true;
    await api.startAnalyze();
    pollStatus();
  }

  function pollStatus() {
    if (statusInterval) clearInterval(statusInterval);
    statusInterval = setInterval(async () => {
      analyzeStatus = await api.getAnalyzeStatus();
      analytics = await api.getAnalytics();
      if (analyzeStatus && !analyzeStatus.running) {
        analyzing = false;
        if (statusInterval) clearInterval(statusInterval);
        statusInterval = null;
      }
    }, 3000);
  }

  async function refreshAll() {
    const [analyticsResult, postsResult] = await Promise.all([
      api.getAnalytics(),
      api.getPosts(),
      loadAllViews(),
    ]);
    analytics = analyticsResult;
    recentPosts = postsResult.slice(0, 10);
    await loadViews();
  }

  async function handleSync() {
    syncing = true;
    syncStatus = 'Starting sync...';
    try {
      const result = await api.triggerSync();
      if (!result.started) {
        syncStatus = result.message;
        syncing = false;
        return;
      }
      pollSyncStatus();
    } catch (e) {
      syncStatus = `Failed: ${e instanceof Error ? e.message : 'Unknown error'}`;
      syncing = false;
      setTimeout(() => { syncStatus = ''; }, 5000);
    }
  }

  function pollSyncStatus() {
    if (syncInterval) clearInterval(syncInterval);
    syncInterval = setInterval(async () => {
      try {
        const status: SyncStatus = await api.getSyncStatus();
        syncStatus = status.message;
        syncStatusData = status;
        if (!status.running) {
          syncing = false;
          syncStatusData = null;
          if (syncInterval) clearInterval(syncInterval);
          syncInterval = null;
          await refreshAll();
          setTimeout(() => { syncStatus = ''; }, 5000);
        }
      } catch {
        // Keep polling on transient errors
      }
    }, 2000);
  }

  onDestroy(() => {
    if (statusInterval) clearInterval(statusInterval);
    if (syncInterval) clearInterval(syncInterval);
  });

  onMount(async () => {
    const [analyticsResult, statusResult, postsResult, categoriesResult] = await Promise.all([
      api.getAnalytics(),
      api.getAnalyzeStatus(),
      api.getPosts(),
      api.getCategories().catch(() => ({ categories: [] })),
    ]);
    analytics = analyticsResult;
    analyzeStatus = statusResult;
    recentPosts = postsResult.slice(0, 10);

    // Build topic-to-color map from categories
    const topicColorMap: Record<string, string> = {};
    for (const cat of categoriesResult.categories) {
      const color = cat.color || '#888';
      for (const topic of cat.topics) {
        topicColorMap[topic] = color;
      }
    }

    if (analyzeStatus?.running) {
      analyzing = true;
      pollStatus();
    }
    if (!analytics) return;

    await tick();

    // Engagement over time
    engagementChart = new Chart(engagementCanvas, {
      type: 'line',
      data: {
        labels: analytics.engagement_over_time.map(e => e.date),
        datasets: [
          { label: 'Likes', data: analytics.engagement_over_time.map(e => e.likes), borderColor: '#e6194b', cubicInterpolationMode: 'monotone' as const },
          { label: 'Replies', data: analytics.engagement_over_time.map(e => e.replies), borderColor: '#3cb44b', cubicInterpolationMode: 'monotone' as const },
          { label: 'Reposts', data: analytics.engagement_over_time.map(e => e.reposts), borderColor: '#4363d8', cubicInterpolationMode: 'monotone' as const },
        ],
      },
      options: {
        responsive: true,
        plugins: { legend: { labels: { color: '#ccc' } } },
        scales: {
          x: { ticks: { color: '#888' }, grid: { color: '#222' } },
          y: { ticks: { color: '#888' }, grid: { color: '#222' } },
        },
      },
    });

    // Views over time – load all data once, compute per-range sums client-side
    await loadAllViews();
    await loadViews();

    // Topics breakdown — show top 15 topics, dynamic height
    const topTopics = analytics.topics.slice(0, 15);
    topicsChart = new Chart(topicsCanvas, {
      type: 'bar',
      data: {
        labels: topTopics.map(t => t.name),
        datasets: [{
          label: 'Posts',
          data: topTopics.map(t => t.post_count),
          backgroundColor: topTopics.map(t => topicColorMap[t.name] || '#4363d8'),
        }],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        indexAxis: 'y',
        plugins: { legend: { display: false } },
        scales: {
          x: { ticks: { color: '#888' }, grid: { color: '#222' } },
          y: { ticks: { color: '#ccc' }, grid: { color: '#222' } },
        },
      },
    });
  });
</script>

<div class="dashboard">
  {#if analytics}
    <div class="stats-row">
      <div class="stat">
        <span class="value">{analytics.total_posts}</span>
        <span class="label">Total Posts</span>
      </div>
      <div class="stat">
        <span class="value">{analytics.analyzed_posts}</span>
        <span class="label">Analyzed</span>
      </div>
      <div class="stat">
        <span class="value">{analytics.total_topics}</span>
        <span class="label">Topics</span>
      </div>
      <div class="sync-actions">
        {#if syncing}
          <div class="sync-progress-section">
            <div class="progress-bar-container sync-progress-bar-container">
              <div class="progress-bar sync-progress-bar" style="width: {getSyncProgressPercent()}%"></div>
            </div>
            <p class="progress-text sync-progress-text">
              {syncStatus}{#if syncStatusData && syncStatusData.total > 0} &mdash; {syncStatusData.synced} / {syncStatusData.total} ({getSyncProgressPercent()}%){/if}
            </p>
          </div>
        {:else}
          <button class="sync-btn" onclick={handleSync}>
            Sync
          </button>
          {#if syncStatus}
            <span class="sync-status">{syncStatus}</span>
          {/if}
        {/if}
      </div>
    </div>

    {#if getUnanalyzedCount() > 0 || analyzing}
      <div class="analyze-section">
        {#if analyzing}
          <div class="progress-bar-container">
            <div class="progress-bar" style="width: {getProgressPercent()}%"></div>
          </div>
          <p class="progress-text">
            Analyzing... {analyzeStatus?.analyzed ?? 0} / {analyzeStatus?.total ?? '?'} posts ({getProgressPercent()}%)
          </p>
        {:else}
          <button class="analyze-btn" onclick={startAnalysis}>
            Analyze All ({getUnanalyzedCount()} remaining)
          </button>
        {/if}
      </div>
    {/if}

    <div class="chart-card views-card">
      <div class="chart-header">
        <h3>Views Over Time {#if getGrouping(selectedRange) !== 'daily'}<span class="grouping-label">({getGrouping(selectedRange)})</span>{/if}</h3>
        <div class="time-filters">
          {#each timeRanges as range}
            <button
              class="filter-btn"
              class:active={selectedRange === range.value}
              onclick={() => { selectedRange = range.value; loadViews(); }}
            >{range.label}{#if rangeSums[range.value] != null} <span class="range-sum">({formatNum(rangeSums[range.value])})</span>{/if}</button>
          {/each}
        </div>
      </div>
      <div class="chart-container">
        <canvas bind:this={viewsCanvas}></canvas>
      </div>
    </div>

    <div class="charts">
      <div class="chart-card">
        <h3>Engagement Over Time</h3>
        <canvas bind:this={engagementCanvas}></canvas>
      </div>
      <div class="chart-card">
        <h3>Topics Breakdown</h3>
        <div class="topics-container">
          <canvas bind:this={topicsCanvas}></canvas>
        </div>
      </div>
    </div>

    {#if recentPosts.length > 0}
      <div class="chart-card posts-card">
        <h3>Recent Posts</h3>
        <div class="table-wrapper">
          <table>
            <thead>
              <tr>
                <th class="col-post">Post</th>
                <th class="col-num">Views</th>
                <th class="col-num">Likes</th>
                <th class="col-num">Comments</th>
                <th class="col-num">Quotes</th>
                <th class="col-num">Shares</th>
              </tr>
            </thead>
            <tbody>
              {#each recentPosts as post}
                <tr>
                  <td class="col-post">
                    <span class="post-text">{post.text?.slice(0, 80) ?? '(no text)'}{(post.text?.length ?? 0) > 80 ? '...' : ''}</span>
                    <span class="post-date">{new Date(post.timestamp).toLocaleDateString()}</span>
                  </td>
                  <td class="col-num">{post.views.toLocaleString()}</td>
                  <td class="col-num">{post.likes.toLocaleString()}</td>
                  <td class="col-num">{post.replies_count.toLocaleString()}</td>
                  <td class="col-num">{post.quotes.toLocaleString()}</td>
                  <td class="col-num">{post.shares.toLocaleString()}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </div>
    {/if}
  {:else}
    <p>Loading analytics...</p>
  {/if}
</div>

<style>
  .dashboard { padding: 1rem; }
  .stats-row { display: flex; gap: 2rem; margin-bottom: 1.5rem; }
  .stat { text-align: center; }
  .value { display: block; font-size: 2rem; font-weight: bold; }
  .label { color: #888; font-size: 0.85rem; }
  .sync-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-left: auto;
  }
  .sync-btn {
    background: #2563eb;
    border: 1px solid #1d4ed8;
    color: white;
    padding: 0.4rem 1rem;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.85rem;
  }
  .sync-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .sync-btn:hover:not(:disabled) { background: #1d4ed8; }
  .sync-status { font-size: 0.75rem; color: #aaa; }
  .sync-progress-section {
    flex: 1;
    min-width: 200px;
  }
  .sync-progress-bar-container {
    background: #222;
    border-radius: 4px;
    height: 8px;
    overflow: hidden;
  }
  .sync-progress-bar {
    background: #2563eb;
    height: 100%;
    transition: width 0.3s ease;
    border-radius: 4px;
  }
  .sync-progress-text {
    color: #aaa;
    font-size: 0.75rem;
    margin: 0.3rem 0 0;
  }
  .analyze-section {
    margin-bottom: 1.5rem;
    padding: 1rem;
    background: #111;
    border: 1px solid #333;
    border-radius: 8px;
  }
  .analyze-btn {
    background: #4363d8;
    color: white;
    border: none;
    padding: 0.75rem 1.5rem;
    border-radius: 6px;
    font-size: 1rem;
    cursor: pointer;
    width: 100%;
  }
  .analyze-btn:hover { background: #3251b8; }
  .progress-bar-container {
    background: #222;
    border-radius: 4px;
    height: 8px;
    overflow: hidden;
  }
  .progress-bar {
    background: #4363d8;
    height: 100%;
    transition: width 0.3s ease;
    border-radius: 4px;
  }
  .progress-text {
    color: #aaa;
    font-size: 0.85rem;
    margin: 0.5rem 0 0;
    text-align: center;
  }
  .charts { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }
  @media (max-width: 768px) {
    .charts { grid-template-columns: 1fr; }
  }
  .chart-card {
    background: #111;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 1rem;
  }
  .topics-container { position: relative; height: 400px; }
  h3 { margin: 0 0 0.5rem; font-size: 1rem; }
  .grouping-label { font-size: 0.75rem; color: #888; font-weight: normal; }
  .views-card { margin-bottom: 1rem; }
  .chart-container { position: relative; height: 300px; }
  .chart-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }
  .chart-header h3 { margin: 0; }
  .time-filters {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
  }
  .filter-btn {
    background: #222;
    color: #888;
    border: 1px solid #333;
    padding: 0.25rem 0.5rem;
    border-radius: 4px;
    font-size: 0.75rem;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .filter-btn:hover { color: #ccc; border-color: #555; }
  .filter-btn.active {
    background: #4363d8;
    color: white;
    border-color: #4363d8;
  }
  .range-sum {
    opacity: 0.7;
    font-size: 0.65rem;
  }
  .posts-card { margin-top: 1rem; }
  .table-wrapper { overflow-x: auto; }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.85rem;
  }
  th, td {
    padding: 0.5rem 0.75rem;
    text-align: left;
    border-bottom: 1px solid #222;
  }
  th {
    color: #888;
    font-weight: 600;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  td { color: #ccc; }
  .col-num { text-align: right; white-space: nowrap; }
  .col-post { max-width: 400px; }
  .post-text {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .post-date {
    display: block;
    color: #666;
    font-size: 0.75rem;
    margin-top: 0.15rem;
  }
  tbody tr:hover { background: #1a1a1a; }
</style>
