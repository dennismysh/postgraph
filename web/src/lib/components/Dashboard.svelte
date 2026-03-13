<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Chart from 'chart.js/auto';
  import { api, type AnalyticsData } from '$lib/api';

  let analytics: AnalyticsData | null = $state(null);
  let engagementCanvas: HTMLCanvasElement = $state(null!);
  let topicsCanvas: HTMLCanvasElement = $state(null!);
  let engagementChart: Chart | null = $state(null);
  let topicsChart: Chart | null = $state(null);

  onMount(async () => {
    analytics = await api.getAnalytics();
    if (!analytics) return;

    // Wait for Svelte to render the canvas elements inside {#if analytics}
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
  .charts { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }
  .chart-card {
    background: #111;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 1rem;
  }
  h3 { margin: 0 0 0.5rem; font-size: 1rem; }
</style>
