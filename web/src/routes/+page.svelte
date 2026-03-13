<script lang="ts">
  import { onMount } from 'svelte';
  import Graph from '$lib/components/Graph.svelte';
  import FilterBar from '$lib/components/FilterBar.svelte';
  import { loadGraph, selectedNode, graphData } from '$lib/stores/graph';
  import { api, type PostDetail } from '$lib/api';

  let postDetail: PostDetail | null = $state(null);
  let detailLoading = $state(false);
  let detailError: string | null = $state(null);

  onMount(() => {
    loadGraph();
  });

  $effect(() => {
    const node = $selectedNode;
    if (node) {
      fetchPostDetail(node);
    } else {
      postDetail = null;
    }
  });

  async function fetchPostDetail(id: string) {
    detailLoading = true;
    detailError = null;
    postDetail = null;
    try {
      postDetail = await api.getPost(id);
    } catch (e) {
      detailError = e instanceof Error ? e.message : 'Failed to load post';
    } finally {
      detailLoading = false;
    }
  }

  function formatDate(ts: string): string {
    return new Date(ts).toLocaleDateString('en-US', {
      year: 'numeric', month: 'short', day: 'numeric',
      hour: '2-digit', minute: '2-digit',
    });
  }

  function formatNumber(n: number): string {
    if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
    if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K';
    return n.toString();
  }

  function closeDetail() {
    selectedNode.set(null);
  }
</script>

<div class="app">
  <FilterBar />
  <main>
    <div class="graph-panel">
      <Graph />
    </div>
    {#if $selectedNode}
      <aside class="detail-panel">
        <div class="detail-header">
          <h2>Post Detail</h2>
          <button class="close-btn" onclick={closeDetail}>&times;</button>
        </div>

        {#if detailLoading}
          <p class="loading">Loading...</p>
        {:else if detailError}
          <p class="error">{detailError}</p>
        {:else if postDetail}
          <div class="post-content">
            {#if postDetail.text}
              <p class="post-text">{postDetail.text}</p>
            {:else}
              <p class="post-text empty">No text content</p>
            {/if}

            {#if postDetail.media_url}
              <div class="media">
                {#if postDetail.media_type === 'IMAGE'}
                  <img src={postDetail.media_url} alt="Post media" />
                {:else if postDetail.media_type === 'VIDEO'}
                  <video src={postDetail.media_url} controls>
                    <track kind="captions" />
                  </video>
                {/if}
              </div>
            {/if}
          </div>

          <div class="views-stat">
            <span class="views-value">{formatNumber(postDetail.views)}</span>
            <span class="views-label">Views</span>
          </div>

          <div class="stats-grid">
            <div class="stat">
              <span class="stat-value">{formatNumber(postDetail.likes)}</span>
              <span class="stat-label">Likes</span>
            </div>
            <div class="stat">
              <span class="stat-value">{formatNumber(postDetail.replies_count)}</span>
              <span class="stat-label">Comments</span>
            </div>
            <div class="stat">
              <span class="stat-value">{formatNumber(postDetail.reposts)}</span>
              <span class="stat-label">Reposts</span>
            </div>
            <div class="stat">
              <span class="stat-value">{formatNumber(postDetail.quotes)}</span>
              <span class="stat-label">Quotes</span>
            </div>
            <div class="stat">
              <span class="stat-value">{formatNumber(postDetail.shares)}</span>
              <span class="stat-label">Shares</span>
            </div>
          </div>

          <div class="detail-section">
            <h3>Engagement</h3>
            <div class="engagement-total">
              <span class="eng-value">{formatNumber(postDetail.engagement_rate)}</span>
              <span class="eng-label">Total interactions</span>
            </div>
          </div>

          {#if postDetail.topics.length > 0}
            <div class="detail-section">
              <h3>Topics</h3>
              <div class="topics">
                {#each postDetail.topics as topic}
                  <span class="topic-tag">{topic}</span>
                {/each}
              </div>
            </div>
          {/if}

          {#if postDetail.sentiment !== null}
            <div class="detail-section">
              <h3>Sentiment</h3>
              <div class="sentiment-bar">
                <div class="sentiment-fill" style="width: {((postDetail.sentiment + 1) / 2) * 100}%"></div>
              </div>
              <span class="sentiment-value">{postDetail.sentiment.toFixed(2)}</span>
            </div>
          {/if}

          <div class="detail-section meta">
            <p class="timestamp">{formatDate(postDetail.timestamp)}</p>
            {#if postDetail.permalink}
              <a href={postDetail.permalink} target="_blank" rel="noopener noreferrer" class="permalink">
                View on Threads
              </a>
            {/if}
          </div>
        {/if}
      </aside>
    {/if}
  </main>
</div>

<style>
  .app { display: flex; flex-direction: column; height: 100%; }
  main { display: flex; flex: 1; overflow: hidden; min-height: 0; }
  .graph-panel { flex: 1; min-height: 0; position: relative; }
  .detail-panel {
    width: 360px;
    padding: 1rem;
    border-left: 1px solid #333;
    overflow-y: auto;
    background: #1a1a1a;
  }
  .detail-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.75rem;
  }
  .detail-header h2 {
    margin: 0;
    font-size: 1.1rem;
  }
  .close-btn {
    background: none;
    border: none;
    color: #888;
    font-size: 1.4rem;
    cursor: pointer;
    padding: 0 0.25rem;
    line-height: 1;
  }
  .close-btn:hover { color: #fff; }
  .loading, .error { color: #888; font-size: 0.9rem; }
  .error { color: #e55; }

  .post-content { margin-bottom: 1rem; }
  .post-text {
    font-size: 0.95rem;
    line-height: 1.5;
    color: #e0e0e0;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .post-text.empty { color: #666; font-style: italic; }

  .media {
    margin-top: 0.75rem;
    border-radius: 8px;
    overflow: hidden;
  }
  .media img, .media video {
    width: 100%;
    display: block;
    border-radius: 8px;
  }

  .views-stat {
    background: #252525;
    border-radius: 8px;
    padding: 0.75rem;
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }
  .views-value {
    font-size: 1.5rem;
    font-weight: 700;
    color: #fff;
  }
  .views-label {
    font-size: 0.8rem;
    color: #888;
  }

  .stats-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }
  .stat {
    background: #252525;
    border-radius: 8px;
    padding: 0.6rem 0.75rem;
    display: flex;
    flex-direction: column;
  }
  .stat-value {
    font-size: 1.25rem;
    font-weight: 600;
    color: #fff;
  }
  .stat-label {
    font-size: 0.75rem;
    color: #888;
    margin-top: 0.15rem;
  }

  .detail-section {
    margin-bottom: 1rem;
  }
  .detail-section h3 {
    font-size: 0.8rem;
    text-transform: uppercase;
    color: #666;
    letter-spacing: 0.05em;
    margin: 0 0 0.5rem 0;
  }

  .engagement-total {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
  }
  .eng-value {
    font-size: 1.5rem;
    font-weight: 700;
    color: #fff;
  }
  .eng-label {
    font-size: 0.8rem;
    color: #888;
  }

  .topics {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }
  .topic-tag {
    background: #333;
    color: #ccc;
    padding: 0.2rem 0.6rem;
    border-radius: 12px;
    font-size: 0.8rem;
  }

  .sentiment-bar {
    height: 6px;
    background: #333;
    border-radius: 3px;
    overflow: hidden;
    margin-bottom: 0.25rem;
  }
  .sentiment-fill {
    height: 100%;
    background: linear-gradient(90deg, #e55, #5e5);
    border-radius: 3px;
  }
  .sentiment-value {
    font-size: 0.85rem;
    color: #aaa;
  }

  .meta {
    border-top: 1px solid #333;
    padding-top: 0.75rem;
  }
  .timestamp {
    font-size: 0.8rem;
    color: #888;
    margin: 0 0 0.5rem 0;
  }
  .permalink {
    font-size: 0.85rem;
    color: #7b8cff;
    text-decoration: none;
  }
  .permalink:hover { text-decoration: underline; }
</style>
