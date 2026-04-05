<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { InsightsResponse, Post } from '$lib/api';

  let report: InsightsResponse | null = null;
  let posts: Post[] = [];
  let loading = true;
  let regenerating = false;
  let error = '';

  const SECTION_STYLES: Record<string, { color: string; icon: string; border: string }> = {
    working: { color: '#4ade80', icon: '●', border: '#1a3a1a' },
    not_working: { color: '#f87171', icon: '●', border: '#3a1a1a' },
    on_brand: { color: '#60a5fa', icon: '●', border: '#1a2a3a' },
    off_pattern: { color: '#facc15', icon: '●', border: '#3a2a1a' },
  };

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
    return text.slice(0, len).trimEnd() + '…';
  }

  function fetchWithTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
    return Promise.race([
      promise,
      new Promise<T>((_, reject) => setTimeout(() => reject(new Error('Timeout')), ms)),
    ]);
  }

  async function loadReport() {
    try {
      report = await fetchWithTimeout(api.getInsightsLatest(), 10000);
    } catch {
      report = null;
    }

    if (report) {
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
      report = await api.generateInsights();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to generate insights';
    } finally {
      regenerating = false;
    }
  }

  onMount(loadReport);
</script>

<div class="insights">
  {#if loading}
    <div class="header">
      <div>
        <h2>Monthly Insights</h2>
        <span class="subtitle">Loading...</span>
      </div>
    </div>
    <div class="grid">
      {#each Array(4) as _}
        <div class="card skeleton"></div>
      {/each}
    </div>
  {:else if error && !report}
    <div class="header">
      <div>
        <h2>Monthly Insights</h2>
      </div>
    </div>
    <div class="empty">
      <p>{error}</p>
    </div>
  {:else if !report}
    <div class="header">
      <div>
        <h2>Monthly Insights</h2>
        <span class="subtitle">No report generated yet</span>
      </div>
    </div>
    <div class="empty">
      <p>No insights have been generated yet.</p>
      <button class="generate-btn" onclick={regenerate} disabled={regenerating}>
        {regenerating ? 'Generating...' : 'Generate Now'}
      </button>
    </div>
  {:else}
    <div class="header">
      <div>
        <h2>Monthly Insights</h2>
        <span class="subtitle">
          Generated {timeAgo(report.generated_at)}
          · {report.trigger_type === 'nightly' ? 'Auto' : 'Manual'}
        </span>
      </div>
      <button class="regen-btn" onclick={regenerate} disabled={regenerating}>
        {regenerating ? 'Generating...' : '↻ Regenerate'}
      </button>
    </div>

    {#if error}
      <div class="error-toast">{error}</div>
    {/if}

    <div class="headline">
      {report.report.headline}
    </div>

    <div class="grid" class:regenerating>
      {#each report.report.sections as section}
        {@const style = SECTION_STYLES[section.key] ?? SECTION_STYLES.working}
        <div class="card" style="border-color: {style.border}">
          <div class="card-header">
            <span class="card-icon" style="color: {style.color}">{style.icon}</span>
            <span class="card-title" style="color: {style.color}">{section.title}</span>
          </div>
          <p class="card-summary">{section.summary}</p>
          {#if section.items.length > 0}
            <div class="card-items">
              {#each section.items as item}
                <div class="item">
                  <p class="observation">{item.observation}</p>
                  {#each item.cited_posts as postId}
                    {@const post = getPostById(postId)}
                    {#if post}
                      <a
                        class="cited-post"
                        href={post.permalink ?? '#'}
                        target="_blank"
                        rel="noopener noreferrer"
                        style="color: {style.color}"
                      >
                        → {truncate(post.text ?? '(no text)', 80)} · {post.views.toLocaleString()} views
                      </a>
                    {/if}
                  {/each}
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .insights {
    max-width: 900px;
    margin: 0 auto;
    padding: 24px;
  }
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 20px;
  }
  h2 {
    font-size: 20px;
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
  .headline {
    background: #111;
    border: 1px solid #222;
    border-radius: 8px;
    padding: 16px 20px;
    margin-bottom: 20px;
    font-size: 15px;
    color: #ddd;
    line-height: 1.5;
    font-weight: 500;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    transition: opacity 0.3s;
  }
  .grid.regenerating {
    opacity: 0.4;
    pointer-events: none;
  }
  .card {
    background: #111;
    border: 1px solid #222;
    border-radius: 8px;
    padding: 20px;
    min-height: 180px;
  }
  .card.skeleton {
    animation: pulse 1.5s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.7; }
  }
  .card-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }
  .card-icon {
    font-size: 14px;
  }
  .card-title {
    font-size: 15px;
    font-weight: 600;
  }
  .card-summary {
    font-size: 13px;
    color: #bbb;
    line-height: 1.6;
    margin: 0 0 12px 0;
  }
  .card-items {
    border-top: 1px solid #222;
    padding-top: 10px;
  }
  .item {
    margin-bottom: 8px;
  }
  .observation {
    font-size: 13px;
    color: #aaa;
    margin: 0 0 4px 0;
  }
  .cited-post {
    display: block;
    font-size: 12px;
    text-decoration: none;
    margin-bottom: 4px;
  }
  .cited-post:hover {
    text-decoration: underline;
  }
  .empty {
    text-align: center;
    padding: 60px 20px;
    color: #888;
  }
  .generate-btn {
    margin-top: 16px;
    background: #333;
    border: 1px solid #444;
    color: #ddd;
    padding: 10px 24px;
    border-radius: 6px;
    font-size: 14px;
    cursor: pointer;
  }
  .generate-btn:hover {
    background: #3a3a3a;
  }
  .generate-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
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
  @media (max-width: 640px) {
    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>
