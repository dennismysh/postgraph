<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import Chart from 'chart.js/auto';
  import { api, type AnalyticsData, type AnalyzeStatus, type ViewsPoint, type EngagementPoint, type Post, type SyncStatus, type PostEngagementPoint } from '$lib/api';

  let analytics: AnalyticsData | null = $state(null);
  let topicsCanvas: HTMLCanvasElement = $state(null!);
  let viewsCanvas: HTMLCanvasElement = $state(null!);
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
  let viewsGrouping = $state('weekly');
  let expandedPostId: string | null = $state(null);
  let postEngagementData: PostEngagementPoint[] = $state([]);
  let postEngagementChart: Chart | null = $state(null);
  let postEngagementCanvas: HTMLCanvasElement = $state(null!);

  // Independent engagement charts
  let likesCanvas: HTMLCanvasElement = $state(null!);
  let repliesCanvas: HTMLCanvasElement = $state(null!);
  let repostsCanvas: HTMLCanvasElement = $state(null!);
  let likesChart: Chart | null = $state(null);
  let repliesChart: Chart | null = $state(null);
  let repostsChart: Chart | null = $state(null);
  let likesRange = $state('30d');
  let repliesRange = $state('30d');
  let repostsRange = $state('30d');
  let allEngagementData: EngagementPoint[] = $state([]);

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

  const viewsGroupingOptions = [
    { label: 'Daily', value: 'daily' },
    { label: 'Weekly', value: 'weekly' },
    { label: '14 Days', value: 'biweekly' },
    { label: 'Monthly', value: 'monthly' },
    { label: 'Quarterly', value: 'quarterly' },
    { label: '6 Months', value: '6months' },
    { label: 'Yearly', value: 'yearly' },
  ];

  function getGrouping(range: string): 'hourly' | 'daily' | 'weekly' {
    if (range === '24h') return 'hourly';
    if (range === '7d' || range === '14d') return 'daily';
    return 'weekly';
  }

  function getEffectiveViewsGrouping(): string {
    if (selectedRange === '24h') return 'hourly';
    return viewsGrouping;
  }

  function getWeekStart(dateStr: string): string {
    const d = new Date(dateStr + 'T00:00:00');
    const day = d.getDay();
    d.setDate(d.getDate() - day); // Sunday start
    return d.toISOString().slice(0, 10);
  }

  function fillHourlyGaps(data: ViewsPoint[]): ViewsPoint[] {
    const lookup = new Map<string, number>();
    for (const point of data) {
      lookup.set(point.date, point.views);
    }
    const result: ViewsPoint[] = [];
    const now = new Date();
    now.setMinutes(0, 0, 0);
    for (let i = 23; i >= 0; i--) {
      const hour = new Date(now.getTime() - i * 3600_000);
      const key = hour.toISOString().slice(0, 13).replace('T', ' ') + ':00';
      const label = key.slice(5);
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

  function getViewsGroupKey(dateStr: string, grouping: string): string {
    switch (grouping) {
      case 'weekly': return getWeekStart(dateStr);
      case 'biweekly': {
        const epoch = new Date('2024-01-01T00:00:00').getTime();
        const d = new Date(dateStr + 'T00:00:00').getTime();
        const daysSinceEpoch = Math.floor((d - epoch) / 86400_000);
        const periodStart = daysSinceEpoch - (daysSinceEpoch % 14);
        const startDate = new Date(epoch + periodStart * 86400_000);
        return startDate.toISOString().slice(0, 10);
      }
      case 'monthly': return dateStr.slice(0, 7);
      case 'quarterly': {
        const year = dateStr.slice(0, 4);
        const month = parseInt(dateStr.slice(5, 7));
        return `${year}-Q${Math.ceil(month / 3)}`;
      }
      case '6months': {
        const year = dateStr.slice(0, 4);
        const month = parseInt(dateStr.slice(5, 7));
        return `${year}-${month <= 6 ? 'H1' : 'H2'}`;
      }
      case 'yearly': return dateStr.slice(0, 4);
      default: return dateStr;
    }
  }

  function formatViewsLabel(key: string, grouping: string, showYear: boolean): string {
    switch (grouping) {
      case 'daily': {
        const d = new Date(key + 'T00:00:00');
        const label = d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
        return showYear ? `${label} '${d.getFullYear().toString().slice(2)}` : label;
      }
      case 'weekly':
      case 'biweekly': {
        const d = new Date(key + 'T00:00:00');
        const label = d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
        return showYear ? `${label} '${d.getFullYear().toString().slice(2)}` : label;
      }
      case 'monthly': {
        const [year, month] = key.split('-');
        const d = new Date(parseInt(year), parseInt(month) - 1);
        const label = d.toLocaleDateString('en-US', { month: 'short' });
        return showYear ? `${label} '${year.slice(2)}` : label;
      }
      case 'quarterly': {
        const [year, q] = key.split('-');
        return `${q} '${year.slice(2)}`;
      }
      case '6months': {
        const [year, h] = key.split('-');
        return `${h} '${year.slice(2)}`;
      }
      case 'yearly': return key;
      default: return key;
    }
  }

  function groupViewsData(data: ViewsPoint[], grouping: string): ViewsPoint[] {
    if (grouping === 'hourly') return fillHourlyGaps(data);

    const showYear = data.length > 1 && new Set(data.map(p => p.date.slice(0, 4))).size > 1;

    if (grouping === 'daily') {
      const filled = ['7d', '14d', '30d'].includes(selectedRange)
        ? fillDailyGaps(data, selectedRange)
        : data;
      return filled.map(p => ({
        date: formatViewsLabel(p.date, 'daily', showYear),
        views: p.views,
      }));
    }

    const grouped = new Map<string, number>();
    for (const point of data) {
      const key = getViewsGroupKey(point.date, grouping);
      grouped.set(key, (grouped.get(key) ?? 0) + point.views);
    }

    return [...grouped.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, views]) => ({
        date: formatViewsLabel(key, grouping, showYear),
        views,
      }));
  }

  interface SingleMetricPoint { date: string; value: number; }

  function extractMetric(data: EngagementPoint[], metric: 'likes' | 'replies' | 'reposts'): SingleMetricPoint[] {
    return data.map(d => ({ date: d.date, value: d[metric] }));
  }

  function fillHourlyGapsMetric(data: SingleMetricPoint[]): SingleMetricPoint[] {
    const lookup = new Map<string, number>();
    for (const point of data) lookup.set(point.date, point.value);
    const result: SingleMetricPoint[] = [];
    const now = new Date();
    now.setMinutes(0, 0, 0);
    for (let i = 23; i >= 0; i--) {
      const hour = new Date(now.getTime() - i * 3600_000);
      const key = hour.toISOString().slice(0, 13).replace('T', ' ') + ':00';
      const label = key.slice(5);
      result.push({ date: label, value: lookup.get(key) ?? 0 });
    }
    return result;
  }

  function fillDailyGapsMetric(data: SingleMetricPoint[], range: string): SingleMetricPoint[] {
    const lookup = new Map<string, number>();
    for (const point of data) lookup.set(point.date, point.value);
    const days = parseInt(range);
    const result: SingleMetricPoint[] = [];
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    for (let i = days - 1; i >= 0; i--) {
      const d = new Date(today.getTime() - i * 86400_000);
      const key = d.toISOString().slice(0, 10);
      result.push({ date: key, value: lookup.get(key) ?? 0 });
    }
    return result;
  }

  function groupMetricData(data: SingleMetricPoint[], grouping: 'hourly' | 'daily' | 'weekly', range: string): SingleMetricPoint[] {
    if (grouping === 'hourly') return fillHourlyGapsMetric(data);
    if (grouping === 'daily') return fillDailyGapsMetric(data, range);
    const grouped = new Map<string, number>();
    for (const point of data) {
      const key = getWeekStart(point.date);
      grouped.set(key, (grouped.get(key) ?? 0) + point.value);
    }
    return [...grouped.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([date, value]) => {
        const d = new Date(date + 'T00:00:00');
        const label = `Week of ${d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}`;
        return { date: label, value };
      });
  }

  async function loadAllEngagement() {
    allEngagementData = await api.getEngagement();
  }

  async function loadEngagementChart(
    metric: 'likes' | 'replies' | 'reposts',
    range: string,
    canvas: HTMLCanvasElement,
    existingChart: Chart | null,
    color: string,
    label: string,
  ): Promise<Chart | null> {
    if (existingChart) existingChart.destroy();
    if (!canvas) return null;

    const grouping = getGrouping(range);
    let engData: EngagementPoint[];

    if (grouping === 'hourly') {
      const since = getSinceDate(range);
      engData = await api.getEngagement(since, 'hourly');
    } else {
      const since = getSinceDate(range);
      if (since) {
        const sinceDate = since.slice(0, 10);
        engData = allEngagementData.filter(p => p.date >= sinceDate);
      } else {
        engData = allEngagementData;
      }
    }

    const raw = extractMetric(engData, metric);
    const chartData = groupMetricData(raw, grouping, range);

    return new Chart(canvas, {
      type: 'line',
      data: {
        labels: chartData.map(v => v.date),
        datasets: [{
          label,
          data: chartData.map(v => v.value),
          borderColor: color,
          backgroundColor: (ctx: { chart: Chart }) => {
            const gradient = ctx.chart.ctx.createLinearGradient(0, 0, 0, ctx.chart.height);
            gradient.addColorStop(0, color + '59');
            gradient.addColorStop(1, color + '00');
            return gradient;
          },
          fill: true,
          cubicInterpolationMode: 'monotone' as const,
          borderWidth: 2.5,
          pointRadius: chartData.length > 30 ? 0 : 3,
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
        plugins: {
          legend: { display: false },
          tooltip: {
            backgroundColor: '#1a1a1a',
            borderColor: '#333',
            borderWidth: 1,
            titleColor: '#ccc',
            bodyColor: color,
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

  async function renderLikesChart() {
    await tick();
    likesChart = await loadEngagementChart('likes', likesRange, likesCanvas, likesChart, '#e6194b', 'Likes');
  }

  async function renderRepliesChart() {
    await tick();
    repliesChart = await loadEngagementChart('replies', repliesRange, repliesCanvas, repliesChart, '#3cb44b', 'Replies');
  }

  async function renderRepostsChart() {
    await tick();
    repostsChart = await loadEngagementChart('reposts', repostsRange, repostsCanvas, repostsChart, '#4363d8', 'Reposts');
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

  async function togglePostEngagement(postId: string) {
    if (expandedPostId === postId) {
      expandedPostId = null;
      if (postEngagementChart) { postEngagementChart.destroy(); postEngagementChart = null; }
      return;
    }
    expandedPostId = postId;
    postEngagementData = await api.getPostEngagement(postId);
    await tick();
    renderPostEngagementChart();
  }

  function renderPostEngagementChart() {
    if (postEngagementChart) postEngagementChart.destroy();
    if (!postEngagementCanvas || postEngagementData.length === 0) return;

    const labels = postEngagementData.map(p => {
      const d = new Date(p.date);
      return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', hour: 'numeric' });
    });

    postEngagementChart = new Chart(postEngagementCanvas, {
      type: 'line',
      data: {
        labels,
        datasets: [
          { label: 'Views', data: postEngagementData.map(p => p.views), borderColor: '#f58231', borderWidth: 2, pointRadius: 2, cubicInterpolationMode: 'monotone' as const },
          { label: 'Likes', data: postEngagementData.map(p => p.likes), borderColor: '#e6194b', borderWidth: 1.5, pointRadius: 1, cubicInterpolationMode: 'monotone' as const },
          { label: 'Replies', data: postEngagementData.map(p => p.replies), borderColor: '#3cb44b', borderWidth: 1.5, pointRadius: 1, cubicInterpolationMode: 'monotone' as const },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { intersect: false, mode: 'index' as const },
        plugins: {
          legend: { labels: { color: '#ccc', boxWidth: 12, font: { size: 11 } } },
          tooltip: {
            backgroundColor: '#1a1a1a',
            borderColor: '#333',
            borderWidth: 1,
            titleColor: '#ccc',
            padding: 8,
          },
        },
        scales: {
          x: { ticks: { color: '#666', maxRotation: 45, maxTicksLimit: 10, font: { size: 10 } }, grid: { color: 'rgba(255,255,255,0.04)' } },
          y: { ticks: { color: '#666', font: { size: 10 } }, grid: { color: 'rgba(255,255,255,0.06)' }, beginAtZero: true },
        },
      },
    });
  }

  async function loadAllViews() {
    allViewsData = await api.getViews();
    rangeSums = computeRangeSums(allViewsData);
  }

  async function loadViews() {
    const grouping = getEffectiveViewsGrouping();

    if (grouping === 'hourly') {
      const since = getSinceDate(selectedRange);
      viewsData = await api.getViews(since, 'hourly');
    } else {
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

    const grouping = getEffectiveViewsGrouping();
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
      loadAllEngagement(),
    ]);
    analytics = analyticsResult;
    recentPosts = postsResult.slice(0, 10);
    await loadViews();
    await Promise.all([renderLikesChart(), renderRepliesChart(), renderRepostsChart()]);
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
    const [analyticsResult, statusResult, postsResult] = await Promise.all([
      api.getAnalytics(),
      api.getAnalyzeStatus(),
      api.getPosts(),
    ]);
    analytics = analyticsResult;
    analyzeStatus = statusResult;
    recentPosts = postsResult.slice(0, 10);

    if (analyzeStatus?.running) {
      analyzing = true;
      pollStatus();
    }
    if (!analytics) return;

    await tick();

    // Views over time – load all data once, compute per-range sums client-side
    await loadAllViews();
    await loadViews();

    // Engagement charts – load all data once, render 3 independent charts
    await loadAllEngagement();
    await Promise.all([renderLikesChart(), renderRepliesChart(), renderRepostsChart()]);

    // Subjects breakdown — show top 15 subjects, dynamic height
    const topSubjects = analytics.subjects.slice(0, 15);
    topicsChart = new Chart(topicsCanvas, {
      type: 'bar',
      data: {
        labels: topSubjects.map(t => t.name),
        datasets: [{
          label: 'Posts',
          data: topSubjects.map(t => t.post_count),
          backgroundColor: '#4363d8',
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
        <span class="value">{analytics.total_subjects}</span>
        <span class="label">Subjects</span>
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
        <div class="chart-title-row">
          <h3>Views Over Time</h3>
          {#if selectedRange !== '24h'}
            <select class="range-select" bind:value={viewsGrouping} onchange={() => renderViewsChart()}>
              {#each viewsGroupingOptions as opt}
                <option value={opt.value}>{opt.label}</option>
              {/each}
            </select>
          {/if}
        </div>
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

    <div class="engagement-charts">
      <div class="chart-card engagement-card">
        <div class="chart-header">
          <h3>Likes Over Time {#if getGrouping(likesRange) !== 'daily'}<span class="grouping-label">({getGrouping(likesRange)})</span>{/if}</h3>
          <select class="range-select" bind:value={likesRange} onchange={() => renderLikesChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-container">
          <canvas bind:this={likesCanvas}></canvas>
        </div>
      </div>
      <div class="chart-card engagement-card">
        <div class="chart-header">
          <h3>Replies Over Time {#if getGrouping(repliesRange) !== 'daily'}<span class="grouping-label">({getGrouping(repliesRange)})</span>{/if}</h3>
          <select class="range-select" bind:value={repliesRange} onchange={() => renderRepliesChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-container">
          <canvas bind:this={repliesCanvas}></canvas>
        </div>
      </div>
      <div class="chart-card engagement-card">
        <div class="chart-header">
          <h3>Reposts Over Time {#if getGrouping(repostsRange) !== 'daily'}<span class="grouping-label">({getGrouping(repostsRange)})</span>{/if}</h3>
          <select class="range-select" bind:value={repostsRange} onchange={() => renderRepostsChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-container">
          <canvas bind:this={repostsCanvas}></canvas>
        </div>
      </div>
    </div>

    <div class="charts">
      <div class="chart-card">
        <h3>Subjects Breakdown</h3>
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
                <tr class="post-row" class:expanded={expandedPostId === post.id} onclick={() => togglePostEngagement(post.id)}>
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
                {#if expandedPostId === post.id}
                  <tr class="engagement-row">
                    <td colspan="6">
                      <div class="post-engagement-chart">
                        {#if postEngagementData.length === 0}
                          <p class="no-data">No engagement history yet</p>
                        {:else}
                          <canvas bind:this={postEngagementCanvas}></canvas>
                        {/if}
                      </div>
                    </td>
                  </tr>
                {/if}
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
  .engagement-charts {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 1rem;
    margin-bottom: 1rem;
  }
  @media (max-width: 1200px) {
    .engagement-charts { grid-template-columns: 1fr; }
  }
  .engagement-card .chart-container { height: 250px; }
  .range-select {
    background: #222;
    color: #ccc;
    border: 1px solid #333;
    padding: 0.25rem 0.5rem;
    border-radius: 4px;
    font-size: 0.75rem;
    cursor: pointer;
  }
  .range-select:hover { border-color: #555; }
  .range-select:focus { outline: none; border-color: #4363d8; }
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
  .chart-title-row { display: flex; align-items: center; gap: 0.5rem; }
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
  tbody tr.post-row { cursor: pointer; }
  tbody tr.post-row:hover { background: #1a1a1a; }
  tbody tr.post-row.expanded { background: #1a1a2a; border-bottom: none; }
  tbody tr.post-row.expanded td { border-bottom: none; }
  .engagement-row td { padding: 0 0.75rem 0.75rem; border-bottom: 1px solid #222; }
  .post-engagement-chart { height: 200px; position: relative; background: #0d0d0d; border-radius: 6px; padding: 0.5rem; }
  .no-data { color: #666; font-size: 0.8rem; text-align: center; padding: 2rem 0; margin: 0; }
</style>
