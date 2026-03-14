<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Sigma from 'sigma';
  import forceAtlas2 from 'graphology-layout-forceatlas2';
  import { graphInstance, selectedNode, loading, error } from '$lib/stores/graph';
  import { filters } from '$lib/stores/filters';
  import type Graph from 'graphology';
  import type { Filters } from '$lib/stores/filters';

  let container: HTMLDivElement = $state(null!);
  let sigma: Sigma | null = $state(null);
  let categories: { name: string; color: string }[] = $state([]);

  function isMobile(): boolean {
    return typeof window !== 'undefined' && window.innerWidth < 768;
  }

  function nodeMatchesFilters(attrs: any, f: Filters): boolean {
    if (f.dateFrom && attrs.timestamp && attrs.timestamp < f.dateFrom) return false;
    if (f.dateTo && attrs.timestamp && attrs.timestamp > f.dateTo) return false;
    if (f.minEngagement > 0 && (attrs.engagement ?? 0) < f.minEngagement) return false;
    if (f.topics.length > 0) {
      const nodeTopics: string[] = attrs.topics || [];
      if (!f.topics.some((t: string) => nodeTopics.includes(t))) return false;
    }
    if (f.category && attrs.category_name !== f.category) return false;
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
    topics: [],
    category: null,
    dateFrom: null,
    dateTo: null,
    minEngagement: 0,
    edgeTypes: [],
    searchQuery: '',
  };

  function initSigma(graph: Graph) {
    try {
      if (sigma) sigma.kill();

      // Assign colors by category and scale node sizes for mobile
      const mobile = isMobile();
      const sizeMultiplier = mobile ? 3 : 1;
      graph.forEachNode((node, attrs) => {
        graph.setNodeAttribute(node, 'color', attrs.category_color || '#888');
        const baseSize = (attrs as any).size || 1;
        graph.setNodeAttribute(node, 'size', baseSize * sizeMultiplier);
      });

      // Extract unique categories for the legend
      const catMap = new Map<string, string>();
      graph.forEachNode((_node, attrs) => {
        if (attrs.category_name && attrs.category_color) {
          catMap.set(attrs.category_name, attrs.category_color);
        }
      });
      categories = [...catMap.entries()].map(([name, color]) => ({ name, color }));

      // Run ForceAtlas2 layout
      forceAtlas2.assign(graph, { iterations: 100, settings: { gravity: 1 } });

      sigma = new Sigma(graph, container, {
        renderEdgeLabels: false,
        defaultEdgeType: 'line',
        labelColor: { color: '#eeeeee' },
        labelSize: mobile ? 12 : 14,
        labelRenderedSizeThreshold: mobile ? 4 : 8,
      });

      sigma.on('clickNode', ({ node }) => {
        selectedNode.set(node);
      });

      sigma.on('clickStage', () => {
        selectedNode.set(null);
      });

      // Apply current filters after init
      applyFilters(currentFilters);
    } catch (e) {
      error.set(e instanceof Error ? e.message : 'Failed to render graph');
    }
  }

  const unsubGraph = graphInstance.subscribe((graph) => {
    if (graph && container) initSigma(graph);
  });

  const unsubFilters = filters.subscribe((f) => {
    currentFilters = f;
    applyFilters(f);
  });

  onMount(() => {
    const graph = $graphInstance;
    if (graph) initSigma(graph);
  });

  onDestroy(() => {
    unsubGraph();
    unsubFilters();
    if (sigma) sigma.kill();
  });
</script>

<div class="graph-wrapper">
  <div class="graph-container" bind:this={container}>
    {#if $loading}
      <div class="overlay">Loading graph...</div>
    {/if}
    {#if $error}
      <div class="overlay error">{$error}</div>
    {/if}
  </div>

  {#if categories.length > 0}
    <div class="legend">
      {#each categories as cat}
        <div class="legend-item">
          <span class="legend-dot" style="background: {cat.color}"></span>
          <span class="legend-name">{cat.name}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .graph-wrapper {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-height: 0;
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
</style>
