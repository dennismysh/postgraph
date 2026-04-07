<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { EmotionsSummaryResponse, EmotionNarrativeResponse, Post } from '$lib/api';
  import { Chart, RadarController, RadialLinearScale, PointElement, LineElement, Filler, Tooltip } from 'chart.js';

  Chart.register(RadarController, RadialLinearScale, PointElement, LineElement, Filler, Tooltip);

  let summary: EmotionsSummaryResponse | null = $state(null);
  let narrative: EmotionNarrativeResponse | null = $state(null);
  let posts: Post[] = $state([]);
  let loading = $state(true);
  let regenerating = $state(false);
  let error = $state('');
  let canvas: HTMLCanvasElement = $state(null as unknown as HTMLCanvasElement);
  let chart: Chart | null = $state(null);

  const EMOTION_COLORS: Record<string, string> = {
    vulnerable: '#c084fc',
    curious: '#60a5fa',
    playful: '#4ade80',
    confident: '#facc15',
    reflective: '#a78bfa',
    frustrated: '#f87171',
    provocative: '#fb923c',
  };

  const EMOTION_ORDER = ['vulnerable', 'curious', 'playful', 'confident', 'reflective', 'frustrated', 'provocative'];

  function timeAgo(dateStr: string): string {
    const diff = Date.now() - new Date(dateStr).getTime();
    const hours = Math.floor(diff / 3600000);
    if (hours < 1) return 'just now';
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  }

  function getPostById(id: string): Post | undefined {
    return posts.find(p => p.id === id);
  }

  function truncate(text: string, len: number): string {
    if (text.length <= len) return text;
    return text.slice(0, len).trimEnd() + '\u2026';
  }

  function fetchWithTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
    return Promise.race([
      promise,
      new Promise<T>((_, reject) => setTimeout(() => reject(new Error('Timeout')), ms)),
    ]);
  }

  function buildChart() {
    if (!canvas || !summary) return;
    if (chart) chart.destroy();

    const labels = EMOTION_ORDER.map(e => e.charAt(0).toUpperCase() + e.slice(1));
    const data = EMOTION_ORDER.map(e => {
      const stat = summary!.emotions.find(s => s.name.toLowerCase() === e);
      return stat ? stat.percentage : 0;
    });

    chart = new Chart(canvas, {
      type: 'radar',
      data: {
        labels,
        datasets: [{
          data,
          backgroundColor: 'rgba(96, 165, 250, 0.15)',
          borderColor: 'rgba(96, 165, 250, 0.8)',
          borderWidth: 2,
          pointBackgroundColor: EMOTION_ORDER.map(e => EMOTION_COLORS[e]),
          pointBorderColor: EMOTION_ORDER.map(e => EMOTION_COLORS[e]),
          pointRadius: 5,
          pointHoverRadius: 7,
        }],
      },
      options: {
        responsive: true,
        maintainAspectRatio: true,
        plugins: {
          legend: { display: false },
          tooltip: {
            callbacks: {
              label: (ctx) => {
                const emotion = EMOTION_ORDER[ctx.dataIndex];
                const stat = summary!.emotions.find(s => s.name.toLowerCase() === emotion);
                if (!stat) return `${ctx.parsed.r.toFixed(1)}%`;
                return `${ctx.parsed.r.toFixed(1)}% (${stat.post_count} posts, ${Math.round(stat.avg_views)} avg views)`;
              },
            },
          },
        },
        scales: {
          r: {
            beginAtZero: true,
            grid: { color: 'rgba(255, 255, 255, 0.08)' },
            angleLines: { color: 'rgba(255, 255, 255, 0.08)' },
            pointLabels: {
              color: '#999',
              font: { size: 12 },
            },
            ticks: {
              display: false,
            },
          },
        },
      },
    });
  }

  async function loadData() {
    try {
      summary = await fetchWithTimeout(api.getEmotionsSummary(), 10000);
    } catch {
      summary = null;
    }

    try {
      narrative = await fetchWithTimeout(api.getEmotionNarrative(), 10000);
    } catch {
      narrative = null;
    }

    if (narrative) {
      try {
        posts = await fetchWithTimeout(api.getPosts(), 10000);
      } catch {
        posts = [];
      }
    }

    loading = false;
  }

  async function regenerate() {
    regenerating = true;
    error = '';
    try {
      narrative = await api.generateEmotionNarrative();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to generate narrative';
    } finally {
      regenerating = false;
    }
  }

  onMount(() => {
    loadData();
  });

  $effect(() => {
    if (summary && canvas) {
      buildChart();
    }
  });
</script>

<div class="emotional-pulse">
  {#if loading}
    <div class="header">
      <div>
        <h3>Emotional Pulse</h3>
        <span class="subtitle">Loading...</span>
      </div>
    </div>
    <div class="skeleton-block"></div>
  {:else if !summary || summary.total_posts === 0}
    <div class="header">
      <div>
        <h3>Emotional Pulse</h3>
        <span class="subtitle">No emotion data yet</span>
      </div>
    </div>
    <div class="empty">
      <p>Posts haven't been classified with emotions yet.</p>
    </div>
  {:else}
    <div class="header">
      <div>
        <h3>Emotional Pulse</h3>
        <span class="subtitle">
          {summary.total_posts} posts &middot; last 30 days
        </span>
      </div>
      {#if narrative}
        <button class="regen-btn" onclick={regenerate} disabled={regenerating}>
          {regenerating ? 'Generating...' : '\u21BB Regenerate'}
        </button>
      {/if}
    </div>

    {#if error}
      <div class="error-toast">{error}</div>
    {/if}

    <div class="content" class:regenerating>
      <div class="chart-container">
        <canvas bind:this={canvas}></canvas>
      </div>

      {#if narrative}
        <div class="narrative-card">
          <p class="narrative-headline">{narrative.narrative.headline}</p>
          <div class="observations">
            {#each narrative.narrative.observations as obs}
              <div class="observation">
                <span class="emotion-tag" style="color: {EMOTION_COLORS[obs.emotion.toLowerCase()] ?? '#888'}">
                  {obs.emotion}
                </span>
                <p class="obs-text">{obs.text}</p>
                {#each obs.cited_posts as postId}
                  {@const post = getPostById(postId)}
                  {#if post}
                    <a
                      class="cited-post"
                      href={post.permalink ?? '#'}
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      &rarr; {truncate(post.text ?? '(no text)', 80)} &middot; {post.views.toLocaleString()} views
                    </a>
                  {/if}
                {/each}
              </div>
            {/each}
          </div>
        </div>
      {:else}
        <div class="empty narrative-empty">
          <p>No emotion narrative generated yet.</p>
          <button class="generate-btn" onclick={regenerate} disabled={regenerating}>
            {regenerating ? 'Generating...' : 'Generate Narrative'}
          </button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .emotional-pulse {
    margin-top: 32px;
    padding-top: 24px;
    border-top: 1px solid #222;
  }
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 20px;
  }
  h3 {
    font-size: 18px;
    font-weight: 600;
    color: #fff;
    margin: 0;
  }
  .subtitle {
    font-size: 13px;
    color: #888;
  }
  .regen-btn {
    background: #222;
    border: 1px solid #333;
    color: #ccc;
    padding: 8px 16px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }
  .regen-btn:hover {
    background: #2a2a2a;
    border-color: #444;
  }
  .regen-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .content {
    transition: opacity 0.3s;
  }
  .content.regenerating {
    opacity: 0.4;
    pointer-events: none;
  }
  .chart-container {
    max-width: 400px;
    margin: 0 auto 24px;
  }
  .narrative-card {
    background: #111;
    border: 1px solid #222;
    border-radius: 8px;
    padding: 20px;
  }
  .narrative-headline {
    font-size: 15px;
    color: #ddd;
    line-height: 1.5;
    font-weight: 500;
    margin: 0 0 16px 0;
  }
  .observations {
    border-top: 1px solid #222;
    padding-top: 12px;
  }
  .observation {
    margin-bottom: 12px;
  }
  .emotion-tag {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .obs-text {
    font-size: 13px;
    color: #bbb;
    line-height: 1.6;
    margin: 4px 0;
  }
  .cited-post {
    display: block;
    font-size: 12px;
    color: #60a5fa;
    text-decoration: none;
    margin-bottom: 4px;
  }
  .cited-post:hover {
    text-decoration: underline;
  }
  .empty {
    text-align: center;
    padding: 40px 20px;
    color: #888;
  }
  .narrative-empty {
    padding: 24px 20px;
  }
  .generate-btn {
    margin-top: 12px;
    background: #333;
    border: 1px solid #444;
    color: #ddd;
    padding: 8px 20px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }
  .generate-btn:hover {
    background: #3a3a3a;
  }
  .generate-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .skeleton-block {
    height: 300px;
    background: #111;
    border-radius: 8px;
    animation: pulse 1.5s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.7; }
  }
  .error-toast {
    background: #2a1515;
    border: 1px solid #3a1a1a;
    color: #f87171;
    padding: 10px 16px;
    border-radius: 6px;
    font-size: 13px;
    margin-bottom: 16px;
  }
</style>
