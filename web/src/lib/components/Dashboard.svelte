<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import Chart from 'chart.js/auto';
  import { api, type AnalyticsData, type AnalyzeStatus } from '$lib/api';

  let analytics: AnalyticsData | null = $state(null);
  let engagementCanvas: HTMLCanvasElement = $state(null!);
  let topicsCanvas: HTMLCanvasElement = $state(null!);
  let engagementChart: Chart | null = $state(null);
  let topicsChart: Chart | null = $state(null);
  let analyzeStatus: AnalyzeStatus | null = $state(null);
  let analyzing = $state(false);
  let statusInterval: ReturnType<typeof setInterval> | null = null;

  function getUnanalyzedCount(): number {
    return analytics ? analytics.total_posts - analytics.analyzed_posts : 0;
  }

  function getProgressPercent(): number {
    if (analyzeStatus && analyzeStatus.total > 0) {
      return Math.round((analyzeStatus.analyzed / analyzeStatus.total) * 100);
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

  onDestroy(() => {
    if (statusInterval) clearInterval(statusInterval);
  });

  onMount(async () => {
    analytics = await api.getAnalytics();
    analyzeStatus = await api.getAnalyzeStatus();
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
          { label: 'Likes', data: analytics.engagement_over_time.map(e => e.likes), borderColor: '#e6194b' },
          { label: 'Replies', data: analytics.engagement_over_time.map(e => e.replies), borderColor: '#3cb44b' },
          { label: 'Reposts', data: analytics.engagement_over_time.map(e => e.reposts), borderColor: '#4363d8' },
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

    // Topics breakdown
    topicsChart = new Chart(topicsCanvas, {
      type: 'bar',
      data: {
        labels: analytics.topics.map(t => t.name),
        datasets: [{
          label: 'Posts',
          data: analytics.topics.map(t => t.post_count),
          backgroundColor: '#4363d8',
        }],
      },
      options: {
        responsive: true,
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

    <div class="charts">
      <div class="chart-card">
        <h3>Engagement Over Time</h3>
        <canvas bind:this={engagementCanvas}></canvas>
      </div>
      <div class="chart-card">
        <h3>Topics Breakdown</h3>
        <canvas bind:this={topicsCanvas}></canvas>
      </div>
    </div>
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
  h3 { margin: 0 0 0.5rem; font-size: 1rem; }
</style>
