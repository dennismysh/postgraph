<script lang="ts">
  import { onDestroy } from 'svelte';
  import { filters, resetFilters } from '$lib/stores/filters';
  import { graphData, loadGraph } from '$lib/stores/graph';
  import { api, type SyncStatus, type IntentInfo } from '$lib/api';

  let intentList: IntentInfo[] = $derived.by(() => {
    const data = $graphData;
    if (!data) return [];
    return data.intents;
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
          await loadGraph($filters.intent ?? undefined, $filters.timeRange);
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
          await loadGraph($filters.intent ?? undefined, $filters.timeRange);
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
    placeholder="Search subjects..."
    bind:value={$filters.searchQuery}
    class="search"
  />

  <div class="filter-group">
    <label>Min engagement</label>
    <input type="range" min="0" max="1000" bind:value={$filters.minEngagement} />
    <span>{$filters.minEngagement}</span>
  </div>

  <div class="filter-group">
    <label>Time Range</label>
    <select bind:value={$filters.timeRange} class="time-range-select">
      <option value="24h">Last 24 Hours</option>
      <option value="7d">Last 7 Days</option>
      <option value="14d">Last 2 Weeks</option>
      <option value="30d">Last 30 Days</option>
      <option value="60d">Last 2 Months</option>
      <option value="90d">Last 3 Months</option>
      <option value="180d">Last 6 Months</option>
      <option value="270d">Last 9 Months</option>
      <option value="365d">Last 12 Months</option>
      <option value="all">All Time</option>
    </select>
  </div>

  <div class="filter-group">
    <label>Intent</label>
    <select bind:value={$filters.intent} class="intent-select">
      <option value={null}>All Intents</option>
      {#each intentList as intent}
        <option value={intent.name}>{intent.name} ({intent.post_count})</option>
      {/each}
    </select>
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
  .time-range-select,
  .intent-select {
    background: #1a1a1a;
    border: 1px solid #444;
    color: #eee;
    padding: 0.2rem 0.4rem;
    border-radius: 4px;
    font-size: 0.8rem;
    cursor: pointer;
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
