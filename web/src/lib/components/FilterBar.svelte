<script lang="ts">
  import { onDestroy } from 'svelte';
  import { filters, resetFilters } from '$lib/stores/filters';
  import { graphData, loadGraph } from '$lib/stores/graph';
  import { api, type SyncStatus } from '$lib/api';

  let allTopics = $derived.by(() => {
    const data = $graphData;
    if (!data) return [];
    const topicSet = new Set<string>();
    for (const node of data.nodes) {
      for (const t of node.topics) topicSet.add(t);
    }
    return [...topicSet].sort();
  });

  let syncing = $state(false);
  let syncStatus = $state('');
  let syncStatusData: SyncStatus | null = $state(null);
  let syncInterval: ReturnType<typeof setInterval> | null = null;
  let reanalyzing = $state(false);
  let reanalyzeStatus = $state('');
  let reanalyzeInterval: ReturnType<typeof setInterval> | null = null;

  onDestroy(() => {
    if (syncInterval) clearInterval(syncInterval);
    if (reanalyzeInterval) clearInterval(reanalyzeInterval);
  });

  function toggleTopic(topic: string) {
    filters.update(f => {
      const topics = f.topics.includes(topic)
        ? f.topics.filter(t => t !== topic)
        : [...f.topics, topic];
      return { ...f, topics };
    });
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

  function getSyncProgressPercent(): number {
    if (syncStatusData && syncStatusData.total > 0) {
      return Math.round((syncStatusData.synced / syncStatusData.total) * 100);
    }
    return 0;
  }

  function pollSyncStatus() {
    if (syncInterval) clearInterval(syncInterval);
    syncInterval = setInterval(async () => {
      try {
        const status = await api.getSyncStatus();
        syncStatus = status.message;
        syncStatusData = status;
        if (!status.running) {
          syncing = false;
          syncStatusData = null;
          if (syncInterval) clearInterval(syncInterval);
          syncInterval = null;
          await loadGraph();
          setTimeout(() => { syncStatus = ''; }, 5000);
        }
      } catch {
        // Keep polling on transient errors
      }
    }, 2000);
  }

  async function handleReanalyze() {
    if (!confirm('This will re-run Mercury analysis on all posts. This may take a while. Continue?')) return;
    reanalyzing = true;
    reanalyzeStatus = 'Reanalyzing...';
    try {
      const result = await api.triggerReanalyze();
      if (!result.started) {
        reanalyzeStatus = result.message;
        reanalyzing = false;
        return;
      }
      reanalyzeStatus = result.message;
      pollReanalyzeStatus();
    } catch (e) {
      reanalyzeStatus = `Failed: ${e instanceof Error ? e.message : 'Unknown error'}`;
      reanalyzing = false;
      setTimeout(() => { reanalyzeStatus = ''; }, 5000);
    }
  }

  function pollReanalyzeStatus() {
    if (reanalyzeInterval) clearInterval(reanalyzeInterval);
    reanalyzeInterval = setInterval(async () => {
      try {
        const status = await api.getAnalyzeStatus();
        if (status.total > 0) {
          const pct = Math.round((status.analyzed / status.total) * 100);
          reanalyzeStatus = `Reanalyzing... ${status.analyzed}/${status.total} (${pct}%)`;
        }
        if (!status.running) {
          reanalyzing = false;
          reanalyzeStatus = 'Reanalysis complete!';
          if (reanalyzeInterval) clearInterval(reanalyzeInterval);
          reanalyzeInterval = null;
          await loadGraph();
          setTimeout(() => { reanalyzeStatus = ''; }, 5000);
        }
      } catch {
        // Keep polling on transient errors
      }
    }, 3000);
  }
</script>

<div class="filter-bar">
  <input
    type="text"
    placeholder="Search posts..."
    bind:value={$filters.searchQuery}
    class="search"
  />

  <div class="filter-group">
    <label>Min engagement</label>
    <input type="range" min="0" max="1000" bind:value={$filters.minEngagement} />
    <span>{$filters.minEngagement}</span>
  </div>

  <div class="filter-group">
    <label>From</label>
    <input type="date" bind:value={$filters.dateFrom} />
    <label>To</label>
    <input type="date" bind:value={$filters.dateTo} />
  </div>

  <div class="topics">
    {#each allTopics as topic}
      <button
        class="topic-chip"
        class:active={$filters.topics.includes(topic)}
        onclick={() => toggleTopic(topic)}
      >
        {topic}
      </button>
    {/each}
  </div>

  <div class="actions">
    <button class="reset" onclick={resetFilters}>Reset</button>
    <button class="sync" onclick={handleSync} disabled={syncing}>
      {syncing ? 'Syncing...' : 'Sync'}
    </button>
    <button class="reanalyze" onclick={handleReanalyze} disabled={reanalyzing}>
      {reanalyzing ? 'Reanalyzing...' : 'Reanalyze'}
    </button>
  </div>
  {#if syncing && syncStatusData}
    <div class="sync-progress-section">
      <div class="sync-progress-bar-container">
        <div class="sync-progress-bar" style="width: {getSyncProgressPercent()}%"></div>
      </div>
      <span class="status">
        {syncStatus}{#if syncStatusData.total > 0} &mdash; {syncStatusData.synced} / {syncStatusData.total} ({getSyncProgressPercent()}%){/if}
      </span>
    </div>
  {:else if syncStatus}
    <span class="status">{syncStatus}</span>
  {/if}
  {#if reanalyzeStatus}
    <span class="status">{reanalyzeStatus}</span>
  {/if}
</div>

<style>
  .filter-bar {
    display: flex;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid #333;
    flex-wrap: wrap;
    align-items: center;
  }
  .search {
    background: #1a1a1a;
    border: 1px solid #444;
    color: #eee;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
  }
  .filter-group {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.8rem;
  }
  .filter-group input[type="date"] {
    background: #1a1a1a;
    border: 1px solid #444;
    color: #eee;
    padding: 0.2rem;
    border-radius: 4px;
  }
  .topics {
    display: flex;
    gap: 0.3rem;
    flex-wrap: wrap;
    max-height: 8rem;
    overflow-y: auto;
  }
  .topic-chip {
    background: #222;
    border: 1px solid #555;
    color: #ccc;
    padding: 0.2rem 0.5rem;
    border-radius: 12px;
    cursor: pointer;
    font-size: 0.75rem;
  }
  .topic-chip.active {
    background: #4363d8;
    border-color: #4363d8;
    color: white;
  }
  .actions {
    display: flex;
    gap: 0.3rem;
  }
  .reset {
    background: #333;
    border: 1px solid #555;
    color: #ccc;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
    cursor: pointer;
  }
  .sync {
    background: #2563eb;
    border: 1px solid #1d4ed8;
    color: white;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.75rem;
  }
  .sync:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .reanalyze {
    background: #8b5cf6;
    border: 1px solid #7c3aed;
    color: white;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.75rem;
  }
  .reanalyze:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .status {
    font-size: 0.75rem;
    color: #aaa;
  }
  .sync-progress-section {
    flex: 1 1 100%;
  }
  .sync-progress-bar-container {
    background: #222;
    border-radius: 4px;
    height: 6px;
    overflow: hidden;
  }
  .sync-progress-bar {
    background: #2563eb;
    height: 100%;
    transition: width 0.3s ease;
    border-radius: 4px;
  }
  label { color: #888; font-size: 0.8rem; }
</style>
