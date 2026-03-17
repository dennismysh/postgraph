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

  let daily: DailyEntry[] = $state([]);
  let hourly: HourlyEntry[] = $state([]);
  let likeSpectrum: SpectrumEntry[] = $state([]);
  let cadenceSpectrum: SpectrumEntry[] = $state([]);
  let hasEnoughData = $state(false);

  // Stats
  let dominantEngagement = $state('—');
  let dominantCadence = $state('—');
  let totalLikes = $state(0);
  let avgPostsPerDay = $state('0');

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

  async function buildCharts() {
    await tick();

    // Chart 1: Daily Likes (area + smoothed line)
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

    // Chart 2: Engagement Spectrum
    const likePeaks = topPeaks(likeSpectrum, 2);
    const filteredLikeSpectrum = likeSpectrum.filter(s => parseFloat(s.period) <= 60);
    spectrumChart = new Chart(spectrumCanvas, {
      type: 'bar',
      data: {
        labels: filteredLikeSpectrum.map(s => s.period),
        datasets: [{
          label: 'Magnitude',
          data: filteredLikeSpectrum.map(s => s.magnitude),
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
      plugins: [peakAnnotationPlugin(likePeaks, '#f59e0b')],
    });

    // Chart 3: Posts per Day
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

    // Chart 4: Cadence Spectrum
    const cadencePeaks = topPeaks(cadenceSpectrum, 2);
    const filteredCadenceSpectrum = cadenceSpectrum.filter(s => parseFloat(s.period) <= 60);
    cadenceChart = new Chart(cadenceCanvas, {
      type: 'bar',
      data: {
        labels: filteredCadenceSpectrum.map(s => s.period),
        datasets: [{
          label: 'Magnitude',
          data: filteredCadenceSpectrum.map(s => s.magnitude),
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
      plugins: [peakAnnotationPlugin(cadencePeaks, '#f59e0b')],
    });

    // Chart 5: Hourly Distribution
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
      const posts = await api.getPosts();
      daily = postsToDaily(posts);
      hourly = postsToHourly(posts);

      totalLikes = daily.reduce((s, d) => s + d.likes, 0);
      const totalPosts = daily.reduce((s, d) => s + d.posts, 0);
      avgPostsPerDay = daily.length > 0 ? (totalPosts / daily.length).toFixed(1) : '0';

      hasEnoughData = daily.length >= 8;

      if (hasEnoughData) {
        const likeSignal = daily.map(d => d.likes);
        const postSignal = daily.map(d => d.posts);

        likeSpectrum = computeSpectrum(likeSignal);
        cadenceSpectrum = computeSpectrum(postSignal);

        const smoothed = computeSmoothed(likeSignal);
        daily = daily.map((d, i) => ({ ...d, smoothed: smoothed[i] }));

        const engPeaks = topPeaks(likeSpectrum, 1);
        dominantEngagement = engPeaks.length > 0 ? `${engPeaks[0].period}d` : '—';

        const cadPeaks = topPeaks(cadenceSpectrum, 1);
        dominantCadence = cadPeaks.length > 0 ? `${cadPeaks[0].period}d` : '—';

        await buildCharts();
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
        <h3>Daily Likes</h3>
        <div class="chart-wrap"><canvas bind:this={likesCanvas}></canvas></div>
      </div>
      <div class="chart-card">
        <h3>Engagement Spectrum</h3>
        <div class="chart-wrap"><canvas bind:this={spectrumCanvas}></canvas></div>
      </div>
      <div class="chart-card">
        <h3>Posts per Day</h3>
        <div class="chart-wrap"><canvas bind:this={postsCanvas}></canvas></div>
      </div>
      <div class="chart-card">
        <h3>Cadence Spectrum</h3>
        <div class="chart-wrap"><canvas bind:this={cadenceCanvas}></canvas></div>
      </div>
      <div class="chart-card full-width">
        <h3>Posting Hour Distribution</h3>
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
