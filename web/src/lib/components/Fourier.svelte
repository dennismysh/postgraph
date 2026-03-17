<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import Chart from 'chart.js/auto';
  import { api, type Post } from '$lib/api';
  import {
    postsToDaily, postsToHourly, computeSpectrum, computeSmoothed, topPeaks,
    type DailyEntry, type HourlyEntry, type SpectrumEntry,
  } from '$lib/fourier';

  let loading = $state(true);
  let error: string | null = $state(null);
  let hasEnoughData = $state(false);

  // All fetched posts (unfiltered)
  let allPosts: Post[] = [];

  // Stats (computed from all posts)
  let dominantEngagement = $state('—');
  let dominantCadence = $state('—');
  let totalLikes = $state(0);
  let avgPostsPerDay = $state('0');

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

  let likesRange = $state('30d');
  let spectrumRange = $state('30d');
  let postsRange = $state('30d');
  let cadenceRange = $state('30d');
  let hourlyRange = $state('30d');

  // Hourly data for bucket stats (tracks hourlyRange)
  let hourly: HourlyEntry[] = $state([]);

  // Canvas refs
  let likesCanvas: HTMLCanvasElement = $state(null!);
  let spectrumCanvas: HTMLCanvasElement = $state(null!);
  let postsCanvas: HTMLCanvasElement = $state(null!);
  let cadenceCanvas: HTMLCanvasElement = $state(null!);
  let hourlyCanvas: HTMLCanvasElement = $state(null!);

  // Chart instances
  let likesChart: Chart | null = null;
  let spectrumChart: Chart | null = null;
  let postsChart: Chart | null = null;
  let cadenceChart: Chart | null = null;
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

  function filterPostsByRange(posts: Post[], range: string): Post[] {
    if (range === 'all') return posts;
    const days = parseInt(range);
    const cutoff = new Date();
    cutoff.setDate(cutoff.getDate() - days);
    return posts.filter(p => new Date(p.timestamp) >= cutoff);
  }

  async function rebuildLikesChart() {
    await tick();
    likesChart?.destroy();
    const filtered = filterPostsByRange(allPosts, likesRange);
    const daily = postsToDaily(filtered);
    const likeSignal = daily.map(d => d.likes);
    const smoothed = computeSmoothed(likeSignal);
    daily.forEach((d, i) => { d.smoothed = smoothed[i]; });

    likesChart = new Chart(likesCanvas, {
      type: 'line',
      data: {
        labels: daily.map(d => formatDate(d.date)),
        datasets: [
          {
            label: 'Likes',
            data: daily.map(d => d.likes),
            borderColor: '#8b5cf6',
            backgroundColor: (ctx: { chart: Chart }) => {
              const gradient = ctx.chart.ctx.createLinearGradient(0, 0, 0, ctx.chart.height);
              gradient.addColorStop(0, 'rgba(139,92,246,0.35)');
              gradient.addColorStop(1, 'rgba(139,92,246,0)');
              return gradient;
            },
            fill: true,
            borderWidth: 1.5,
            pointRadius: daily.length > 30 ? 0 : 2,
            pointHoverRadius: 4,
            cubicInterpolationMode: 'monotone' as const,
          },
          {
            label: 'Trend',
            data: daily.map(d => d.smoothed ?? null),
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
  }

  async function rebuildSpectrumChart() {
    await tick();
    spectrumChart?.destroy();
    const filtered = filterPostsByRange(allPosts, spectrumRange);
    const daily = postsToDaily(filtered);
    if (daily.length < 8) {
      spectrumChart = null;
      return;
    }
    const likeSignal = daily.map(d => d.likes);
    const spectrum = computeSpectrum(likeSignal);
    const peaks = topPeaks(spectrum, 2);
    const filteredSpectrum = spectrum.filter(s => parseFloat(s.period) <= 60);

    spectrumChart = new Chart(spectrumCanvas, {
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
        plugins: {
          legend: { display: false },
          tooltip: darkTooltip,
        },
        scales: {
          x: {
            ...darkScaleX,
            title: { display: true, text: 'period (days)', color: '#888', font: { size: 11 } },
          },
          y: darkScaleY,
        },
      },
      plugins: [peakAnnotationPlugin(peaks, '#f59e0b')],
    });
  }

  async function rebuildPostsChart() {
    await tick();
    postsChart?.destroy();
    const filtered = filterPostsByRange(allPosts, postsRange);
    const daily = postsToDaily(filtered);

    postsChart = new Chart(postsCanvas, {
      type: 'bar',
      data: {
        labels: daily.map(d => formatDate(d.date)),
        datasets: [{
          label: 'Posts',
          data: daily.map(d => d.posts),
          backgroundColor: 'rgba(67,99,216,0.6)',
          borderColor: '#4363d8',
          borderWidth: 1,
        }],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { intersect: false, mode: 'index' as const },
        plugins: {
          legend: { display: false },
          tooltip: darkTooltip,
        },
        scales: {
          x: darkScaleX,
          y: {
            ...darkScaleY,
            ticks: { ...darkScaleY.ticks, stepSize: 1 },
          },
        },
      },
    });
  }

  async function rebuildCadenceChart() {
    await tick();
    cadenceChart?.destroy();
    const filtered = filterPostsByRange(allPosts, cadenceRange);
    const daily = postsToDaily(filtered);
    if (daily.length < 8) {
      cadenceChart = null;
      return;
    }
    const postSignal = daily.map(d => d.posts);
    const spectrum = computeSpectrum(postSignal);
    const peaks = topPeaks(spectrum, 2);
    const filteredSpectrum = spectrum.filter(s => parseFloat(s.period) <= 60);

    cadenceChart = new Chart(cadenceCanvas, {
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
        plugins: {
          legend: { display: false },
          tooltip: darkTooltip,
        },
        scales: {
          x: {
            ...darkScaleX,
            title: { display: true, text: 'period (days)', color: '#888', font: { size: 11 } },
          },
          y: darkScaleY,
        },
      },
      plugins: [peakAnnotationPlugin(peaks, '#f59e0b')],
    });
  }

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
        plugins: {
          legend: { display: false },
          tooltip: darkTooltip,
        },
        scales: {
          x: { ticks: { color: '#666', maxRotation: 0, font: { size: 11 }, maxTicksLimit: 24 }, grid: { color: 'rgba(255,255,255,0.04)' } },
          y: {
            ...darkScaleY,
            ticks: { ...darkScaleY.ticks, stepSize: 1 },
          },
        },
      },
    });
  }

  onMount(async () => {
    try {
      allPosts = await api.getPosts();
      const allDaily = postsToDaily(allPosts);

      totalLikes = allDaily.reduce((s, d) => s + d.likes, 0);
      const totalPostsCount = allDaily.reduce((s, d) => s + d.posts, 0);
      avgPostsPerDay = allDaily.length > 0 ? (totalPostsCount / allDaily.length).toFixed(1) : '0';

      hasEnoughData = allDaily.length >= 8;

      if (hasEnoughData) {
        const likeSignal = allDaily.map(d => d.likes);
        const postSignal = allDaily.map(d => d.posts);

        const allLikeSpectrum = computeSpectrum(likeSignal);
        const allCadenceSpectrum = computeSpectrum(postSignal);

        const engPeaks = topPeaks(allLikeSpectrum, 1);
        dominantEngagement = engPeaks.length > 0 ? `${engPeaks[0].period}d` : '—';

        const cadPeaks = topPeaks(allCadenceSpectrum, 1);
        dominantCadence = cadPeaks.length > 0 ? `${cadPeaks[0].period}d` : '—';

        loading = false;
        await Promise.all([
          rebuildLikesChart(),
          rebuildSpectrumChart(),
          rebuildPostsChart(),
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
    likesChart?.destroy();
    spectrumChart?.destroy();
    postsChart?.destroy();
    cadenceChart?.destroy();
    hourlyChart?.destroy();
  });

  // Hourly bucket stats
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
    <div class="status">Not enough data for Fourier analysis (need at least 8 days of posts).</div>
  {:else}
    <!-- Stats Ribbon -->
    <div class="ribbon">
      <div class="kpi">
        <span class="kpi-label">Engagement Cycle</span>
        <span class="kpi-value">{dominantEngagement}</span>
      </div>
      <div class="kpi">
        <span class="kpi-label">Posting Cycle</span>
        <span class="kpi-value">{dominantCadence}</span>
      </div>
      <div class="kpi">
        <span class="kpi-label">Total Likes</span>
        <span class="kpi-value">{totalLikes.toLocaleString()}</span>
      </div>
      <div class="kpi">
        <span class="kpi-label">Avg Posts/Day</span>
        <span class="kpi-value">{avgPostsPerDay}</span>
      </div>
    </div>

    <!-- Chart Grid -->
    <div class="grid">
      <div class="chart-card">
        <div class="chart-header">
          <h3>Daily Likes</h3>
          <select class="range-select" bind:value={likesRange} onchange={() => rebuildLikesChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-wrap"><canvas bind:this={likesCanvas}></canvas></div>
      </div>
      <div class="chart-card">
        <div class="chart-header">
          <h3>Engagement Spectrum</h3>
          <select class="range-select" bind:value={spectrumRange} onchange={() => rebuildSpectrumChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-wrap"><canvas bind:this={spectrumCanvas}></canvas></div>
      </div>
      <div class="chart-card">
        <div class="chart-header">
          <h3>Posts per Day</h3>
          <select class="range-select" bind:value={postsRange} onchange={() => rebuildPostsChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-wrap"><canvas bind:this={postsCanvas}></canvas></div>
      </div>
      <div class="chart-card">
        <div class="chart-header">
          <h3>Cadence Spectrum</h3>
          <select class="range-select" bind:value={cadenceRange} onchange={() => rebuildCadenceChart()}>
            {#each timeRanges as range}
              <option value={range.value}>{range.label}</option>
            {/each}
          </select>
        </div>
        <div class="chart-wrap"><canvas bind:this={cadenceCanvas}></canvas></div>
      </div>
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
          <span>Morning (6–11): <strong>{bucketCount(6, 11)}</strong></span>
          <span>Evening (18–23): <strong>{bucketCount(18, 23)}</strong></span>
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
