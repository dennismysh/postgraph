<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Sigma from 'sigma';
  import forceAtlas2 from 'graphology-layout-forceatlas2';
  import { graphInstance, selectedNode, loading, error, loadGraph, graphData } from '$lib/stores/graph';
  import { filters } from '$lib/stores/filters';
  import { api, type SubjectPost, type IntentInfo } from '$lib/api';
  import type Graph from 'graphology';
  import type { Filters } from '$lib/stores/filters';

  let container: HTMLDivElement = $state(null!);
  let sigma: Sigma | null = $state(null);
  let intents: IntentInfo[] = $state([]);

  // Sidebar state
  let sidebarSubject: string | null = $state(null);
  let sidebarPosts: SubjectPost[] = $state([]);
  let sidebarLoading = $state(false);
  let sidebarError: string | null = $state(null);

  function isMobile(): boolean {
    return typeof window !== 'undefined' && window.innerWidth < 768;
  }

  function nodeMatchesFilters(attrs: any, f: Filters): boolean {
    if (f.minEngagement > 0 && (attrs.avg_engagement ?? 0) < f.minEngagement) return false;
    if (f.searchQuery) {
      const label = (attrs.label || '').toLowerCase();
      if (!label.includes(f.searchQuery.toLowerCase())) return false;
    }
    return true;
  }

  function applyFilters(f: Filters) {
    if (!sigma) return;
    const graph = sigma.getGraph();
    const visibleNodes = new Set<string>();

    graph.forEachNode((node, attrs) => {
      if (nodeMatchesFilters(attrs, f)) {
        visibleNodes.add(node);
      }
    });

    sigma.setSetting('nodeReducer', (node, data) => {
      if (!visibleNodes.has(node)) {
        return { ...data, hidden: true };
      }
      return data;
    });

    sigma.setSetting('edgeReducer', (edge, data) => {
      const source = graph.source(edge);
      const target = graph.target(edge);
      if (!visibleNodes.has(source) || !visibleNodes.has(target)) {
        return { ...data, hidden: true };
      }
      return data;
    });
  }

  let currentFilters: Filters = {
    intent: null,
    timeRange: 'all',
    minEngagement: 0,
    searchQuery: '',
  };

  function initSigma(graph: Graph) {
    try {
      if (sigma) sigma.kill();

      const mobile = isMobile();
      const sizeMultiplier = mobile ? 2.5 : 1;
      graph.forEachNode((node, attrs) => {
        const baseSize = (attrs as any).size || 3;
        graph.setNodeAttribute(node, 'size', baseSize * sizeMultiplier);
      });

      // Run ForceAtlas2 with higher gravity for tighter layout (fewer nodes)
      forceAtlas2.assign(graph, { iterations: 150, settings: { gravity: 3, scalingRatio: 2 } });

      sigma = new Sigma(graph, container, {
        renderEdgeLabels: false,
        defaultEdgeType: 'line',
        labelColor: { color: '#eeeeee' },
        labelSize: mobile ? 12 : 14,
        labelRenderedSizeThreshold: 0,  // Always show labels (few enough nodes)
      });

      sigma.on('clickNode', ({ node }) => {
        selectedNode.set(node);
        fetchSubjectPosts(node);
      });

      sigma.on('clickStage', () => {
        selectedNode.set(null);
        closeSidebar();
      });

      // Apply current filters after init
      applyFilters(currentFilters);
    } catch (e) {
      error.set(e instanceof Error ? e.message : 'Failed to render graph');
    }
  }

  async function fetchSubjectPosts(subjectId: string) {
    sidebarSubject = subjectId;
    sidebarLoading = true;
    sidebarError = null;
    sidebarPosts = [];
    try {
      const resp = await api.getSubjectPosts(subjectId, currentFilters.intent ?? undefined);
      sidebarSubject = resp.subject;
      sidebarPosts = resp.posts.sort((a, b) => b.engagement - a.engagement);
    } catch (e) {
      sidebarError = e instanceof Error ? e.message : 'Failed to load posts';
    } finally {
      sidebarLoading = false;
    }
  }

  function closeSidebar() {
    sidebarSubject = null;
    sidebarPosts = [];
    sidebarError = null;
  }

  function formatDate(ts: string): string {
    return new Date(ts).toLocaleDateString('en-US', {
      year: 'numeric', month: 'short', day: 'numeric',
    });
  }

  function formatNumber(n: number): string {
    if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
    if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K';
    return n.toString();
  }

  function truncateText(text: string | null, maxLen: number = 120): string {
    if (!text) return 'No text content';
    if (text.length <= maxLen) return text;
    return text.slice(0, maxLen) + '...';
  }

  const unsubGraph = graphInstance.subscribe((graph) => {
    if (graph && container) initSigma(graph);
  });

  const unsubData = graphData.subscribe((data) => {
    if (data) {
      intents = data.intents;
    }
  });

  const unsubFilters = filters.subscribe((f) => {
    const needsRefetch = currentFilters.intent !== f.intent || currentFilters.timeRange !== f.timeRange;
    currentFilters = f;
    if (needsRefetch) {
      loadGraph(f.intent ?? undefined, f.timeRange);
    } else {
      applyFilters(f);
    }
  });

  onMount(() => {
    const graph = $graphInstance;
    if (graph) initSigma(graph);
  });

  onDestroy(() => {
    unsubGraph();
    unsubData();
    unsubFilters();
    if (sigma) sigma.kill();
  });
</script>

<div class="graph-wrapper">
  <div class="graph-area" class:has-sidebar={sidebarSubject !== null}>
    <div class="graph-container" bind:this={container}>
      {#if $loading}
        <div class="overlay">Loading graph...</div>
      {/if}
      {#if $error}
        <div class="overlay error">{$error}</div>
      {/if}
    </div>

    {#if intents.length > 0}
      <div class="legend">
        <span class="legend-title">Intents:</span>
        {#each intents as intent}
          <div class="legend-item">
            <span class="legend-dot" style="background: {intent.color}"></span>
            <span class="legend-name">{intent.name} ({intent.post_count})</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  {#if sidebarSubject !== null}
    <aside class="sidebar">
      <div class="sidebar-header">
        <h2>{sidebarSubject}</h2>
        <button class="close-btn" onclick={closeSidebar}>&times;</button>
      </div>

      {#if sidebarLoading}
        <p class="sidebar-status">Loading posts...</p>
      {:else if sidebarError}
        <p class="sidebar-status error">{sidebarError}</p>
      {:else if sidebarPosts.length === 0}
        <p class="sidebar-status">No posts found for this subject.</p>
      {:else}
        <div class="posts-list">
          {#each sidebarPosts as post}
            <div class="post-card">
              <p class="post-text">{truncateText(post.text)}</p>
              <div class="post-meta">
                <span class="intent-badge">{post.intent}</span>
                <span class="post-engagement">{formatNumber(post.engagement)} eng</span>
                <span class="post-date">{formatDate(post.timestamp)}</span>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </aside>
  {/if}
</div>

<style>
  .graph-wrapper {
    display: flex;
    width: 100%;
    height: 100%;
    min-height: 0;
  }
  .graph-area {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }
  .graph-area.has-sidebar {
    flex: 1;
  }
  .graph-container {
    flex: 1;
    min-height: 0;
    position: relative;
  }
  .overlay {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    color: #888;
    z-index: 1;
  }
  .error { color: #e6194b; }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    padding: 0.4rem 0.6rem;
    border-top: 1px solid #333;
    background: #111;
    align-items: center;
  }
  .legend-title {
    font-size: 0.75rem;
    color: #888;
    font-weight: 600;
  }
  .legend-item {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.75rem;
    color: #ccc;
  }
  .legend-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  /* Sidebar */
  .sidebar {
    width: 360px;
    border-left: 1px solid #333;
    background: #1a1a1a;
    overflow-y: auto;
    padding: 1rem;
    flex-shrink: 0;
  }
  .sidebar-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.75rem;
  }
  .sidebar-header h2 {
    margin: 0;
    font-size: 1.1rem;
    color: #fff;
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
  .sidebar-status {
    color: #888;
    font-size: 0.9rem;
  }
  .sidebar-status.error {
    color: #e55;
  }

  .posts-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .post-card {
    background: #252525;
    border-radius: 8px;
    padding: 0.75rem;
  }
  .post-text {
    font-size: 0.85rem;
    line-height: 1.4;
    color: #ddd;
    margin: 0 0 0.5rem 0;
    word-break: break-word;
  }
  .post-meta {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .intent-badge {
    background: #4363d8;
    color: #fff;
    padding: 0.15rem 0.5rem;
    border-radius: 10px;
    font-size: 0.7rem;
    font-weight: 600;
  }
  .post-engagement {
    font-size: 0.75rem;
    color: #aaa;
  }
  .post-date {
    font-size: 0.75rem;
    color: #666;
  }
</style>
