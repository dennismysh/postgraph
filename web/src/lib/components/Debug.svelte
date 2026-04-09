<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type DebugPost } from '$lib/api';

  let loading = $state(true);
  let error: string | null = $state(null);
  let posts: DebugPost[] = $state([]);
  let range = $state('24h');
  let backfilling = $state(false);
  let backfillResult: string | null = $state(null);
  let detecting = $state(false);
  let detectResult: string | null = $state(null);

  const ranges = [
    { label: '24h', value: '24h' },
    { label: '7d', value: '7d' },
    { label: '30d', value: '30d' },
    { label: 'All', value: 'all' },
  ];

  function sinceFromRange(r: string): string | undefined {
    if (r === 'all') return undefined;
    const hours = r === '24h' ? 24 : r === '7d' ? 168 : 720;
    const d = new Date();
    d.setHours(d.getHours() - hours);
    return d.toISOString();
  }

  function formatET(iso: string): string {
    return new Date(iso).toLocaleString('en-US', {
      timeZone: 'America/New_York',
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
      hour12: true,
    });
  }

  function getApiBucket(utcTimestamp: string): string {
    const dt = new Date(utcTimestamp);
    if (dt.getUTCHours() < 8) {
      dt.setUTCDate(dt.getUTCDate() - 1);
    }
    return dt.toISOString().slice(0, 10);
  }

  function getLocalDate(utcTimestamp: string): string {
    return new Date(utcTimestamp).toLocaleDateString('en-CA', {
      timeZone: 'America/New_York',
    });
  }

  function bucketDiffers(utcTimestamp: string): boolean {
    return getApiBucket(utcTimestamp) !== getLocalDate(utcTimestamp);
  }

  async function fetchPosts() {
    loading = true;
    error = null;
    try {
      posts = await api.getDebugPosts(sinceFromRange(range));
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load';
    } finally {
      loading = false;
    }
  }

  async function runBackfill() {
    backfilling = true;
    backfillResult = null;
    try {
      await api.backfillEmotions();
      backfillResult = 'Backfill started — check server logs for progress';
    } catch (e) {
      backfillResult = e instanceof Error ? e.message : 'Backfill failed';
    } finally {
      backfilling = false;
    }
  }

  async function runDetectReplies() {
    detecting = true;
    detectResult = null;
    try {
      const result = await api.detectReplies();
      detectResult = `Detected ${result.detected} externally-replied replies`;
    } catch (e) {
      detectResult = e instanceof Error ? e.message : 'Detection failed';
    } finally {
      detecting = false;
    }
  }

  onMount(fetchPosts);
</script>

<div class="debug-page">
  <div class="toolbar">
    <h2>Pipeline Debug</h2>
    <div class="toolbar-actions">
      <div class="range-buttons">
        {#each ranges as r}
          <button
            class:active={range === r.value}
            onclick={() => { range = r.value; fetchPosts(); }}
          >{r.label}</button>
        {/each}
      </div>
      <button class="backfill-btn" onclick={runBackfill} disabled={backfilling}>
        {backfilling ? 'Backfilling...' : 'Backfill Emotions'}
      </button>
      <button class="backfill-btn" onclick={runDetectReplies} disabled={detecting}>
        {detecting ? 'Detecting...' : 'Detect Replies'}
      </button>
    </div>
  </div>

  {#if backfillResult}
    <div class="backfill-result">{backfillResult}</div>
  {/if}

  {#if detectResult}
    <div class="backfill-result">{detectResult}</div>
  {/if}

  {#if loading}
    <div class="status">Loading...</div>
  {:else if error}
    <div class="status error">{error}</div>
  {:else if posts.length === 0}
    <div class="status">No posts in this range.</div>
  {:else}
    <div class="summary">
      <span>{posts.length} posts</span>
      <span>Latest sync: {formatET(posts[0].synced_at)}</span>
      {#if posts[0].last_captured_at}
        <span>Latest capture: {formatET(posts[0].last_captured_at)}</span>
      {/if}
    </div>

    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Text</th>
            <th>Posted (ET)</th>
            <th>API Bucket</th>
            <th>Views</th>
            <th>Likes</th>
            <th>Replies</th>
            <th>Reposts</th>
            <th>Intent</th>
            <th>Subject</th>
            <th>Synced (ET)</th>
            <th>Captured (ET)</th>
          </tr>
        </thead>
        <tbody>
          {#each posts as post}
            <tr>
              <td class="text-cell" title={post.text_preview ?? ''}>
                {post.text_preview?.slice(0, 80) ?? '—'}
              </td>
              <td class="mono">{formatET(post.timestamp)}</td>
              <td class="mono" class:bucket-diff={bucketDiffers(post.timestamp)}>
                {getApiBucket(post.timestamp)}
              </td>
              <td class="num">{post.views.toLocaleString()}</td>
              <td class="num">{post.likes}</td>
              <td class="num">{post.replies_count}</td>
              <td class="num">{post.reposts}</td>
              <td>{post.intent ?? '—'}</td>
              <td>{post.subject ?? '—'}</td>
              <td class="mono">{formatET(post.synced_at)}</td>
              <td class="mono">{post.last_captured_at ? formatET(post.last_captured_at) : '—'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .debug-page {
    padding: 1.5rem;
    max-width: 1400px;
    margin: 0 auto;
  }
  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
  }
  .toolbar h2 {
    margin: 0;
    font-size: 1rem;
    color: #aaa;
    font-weight: 500;
  }
  .toolbar-actions {
    display: flex;
    gap: 12px;
    align-items: center;
  }
  .range-buttons {
    display: flex;
    gap: 4px;
  }
  .backfill-btn {
    background: #1a2a1a;
    color: #4ade80;
    border: 1px solid #2a3a2a;
    border-radius: 4px;
    padding: 0.25rem 0.75rem;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .backfill-btn:hover { border-color: #4ade80; }
  .backfill-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .backfill-result {
    font-size: 0.8rem;
    color: #4ade80;
    padding: 0.5rem 0.75rem;
    background: #111;
    border: 1px solid #2a3a2a;
    border-radius: 4px;
    margin-bottom: 1rem;
  }
  .range-buttons button {
    background: #222;
    color: #ccc;
    border: 1px solid #333;
    border-radius: 4px;
    padding: 0.25rem 0.75rem;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .range-buttons button:hover { border-color: #555; }
  .range-buttons button.active { background: #333; color: #fff; border-color: #555; }
  .status {
    text-align: center;
    color: #888;
    padding: 3rem 1rem;
  }
  .status.error { color: #f87171; }
  .summary {
    display: flex;
    gap: 2rem;
    font-size: 0.8rem;
    color: #888;
    margin-bottom: 1rem;
    padding: 0.5rem 0;
    border-bottom: 1px solid #222;
  }
  .table-wrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8rem;
  }
  thead th {
    text-align: left;
    color: #666;
    font-weight: 500;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #333;
    white-space: nowrap;
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.05em;
  }
  tbody td {
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #1a1a1a;
    color: #ccc;
    white-space: nowrap;
  }
  tbody tr:hover { background: #111; }
  .text-cell {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono { font-family: 'SF Mono', 'Menlo', monospace; font-size: 0.75rem; }
  .num { text-align: right; }
  .bucket-diff {
    color: #f59e0b;
    font-weight: 600;
  }

  @media (max-width: 768px) {
    .debug-page { padding: 0.75rem; }
    table { font-size: 0.7rem; }
  }
</style>
