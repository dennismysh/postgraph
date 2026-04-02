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

  let showViewsSpectrum = $state(false);
  let showEngagementSpectrum = $state(false);
  let showCadenceSpectrum = $state(false);

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
    showViewsSpectrum = filtered.length >= 8;
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
      await tick(); // wait for {#if showViewsSpectrum} to render canvas
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
    showEngagementSpectrum = filtered.length >= 8;
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
      await tick(); // wait for {#if showEngagementSpectrum} to render canvas
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
    showCadenceSpectrum = cadence.length >= 8;
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
      await tick(); // wait for {#if showCadenceSpectrum} to render canvas
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
        {#if showViewsSpectrum}
          <div class="spectrum-wrap"><canvas bind:this={viewsSpectrumCanvas}></canvas></div>
        {/if}
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
        {#if showEngagementSpectrum}
          <div class="spectrum-wrap"><canvas bind:this={engagementSpectrumCanvas}></canvas></div>
        {/if}
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
        {#if showCadenceSpectrum}
          <div class="spectrum-wrap"><canvas bind:this={cadenceSpectrumCanvas}></canvas></div>
        {/if}
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

<style>
  .fourier-page {
    padding: 1.5rem;
    max-width: 1200px;
    margin: 0 auto;
  }
  .status {
    text-align: center;
    color: #888;
    padding: 4rem 1rem;
    font-size: 1rem;
  }
  .status.error {
    color: #f87171;
  }

  /* Stats Ribbon */
  .ribbon {
    display: flex;
    gap: 16px;
    margin-bottom: 1.5rem;
  }
  .kpi {
    flex: 1;
    background: #111;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .kpi-label {
    color: #888;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .kpi-value {
    color: #eee;
    font-size: 1.5rem;
    font-weight: 600;
  }

  /* Chart Grid */
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }
  .full-width {
    grid-column: 1 / -1;
  }
  .chart-card {
    background: #111;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 1rem;
  }
  .chart-card h3 {
    margin: 0 0 0.75rem;
    font-size: 0.85rem;
    color: #aaa;
    font-weight: 500;
  }
  .chart-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }
  .chart-header h3 {
    margin: 0;
  }
  .range-select {
    background: #222;
    color: #ccc;
    border: 1px solid #333;
    border-radius: 4px;
    padding: 0.25rem 0.5rem;
    font-size: 0.75rem;
    cursor: pointer;
  }
  .range-select:hover { border-color: #555; }
  .range-select:focus { outline: none; border-color: #8b5cf6; }
  .chart-wrap {
    position: relative;
    height: 260px;
  }
  .spectrum-wrap {
    position: relative;
    height: 160px;
    margin-top: 0.75rem;
    border-top: 1px solid #222;
    padding-top: 0.75rem;
  }

  /* Hourly stats */
  .hour-stats {
    display: flex;
    gap: 2rem;
    margin-top: 0.75rem;
    font-size: 0.8rem;
    color: #888;
  }
  .hour-stats strong {
    color: #ccc;
  }

  @media (max-width: 768px) {
    .ribbon {
      flex-wrap: wrap;
    }
    .kpi {
      min-width: calc(50% - 8px);
    }
    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>
