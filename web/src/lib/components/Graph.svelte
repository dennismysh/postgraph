<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Sigma from 'sigma';
  import forceAtlas2 from 'graphology-layout-forceatlas2';
  import louvain from 'graphology-communities-louvain';
  import { graphInstance, selectedNode, loading, error } from '$lib/stores/graph';
  import { filters } from '$lib/stores/filters';
  import type Graph from 'graphology';
  import type { Filters } from '$lib/stores/filters';

  let container: HTMLDivElement = $state(null!);
  let sigma: Sigma | null = $state(null);

  function isMobile(): boolean {
    return typeof window !== 'undefined' && window.innerWidth < 768;
  }

  const COLORS = [
    '#e6194b', '#3cb44b', '#4363d8', '#f58231', '#911eb4',
    '#42d4f4', '#f032e6', '#bfef45', '#fabed4', '#469990',
  ];

  function nodeMatchesFilters(attrs: any, f: Filters): boolean {
    if (f.dateFrom && attrs.timestamp && attrs.timestamp < f.dateFrom) return false;
    if (f.dateTo && attrs.timestamp && attrs.timestamp > f.dateTo) return false;
    if (f.minEngagement > 0 && (attrs.engagement ?? 0) < f.minEngagement) return false;
    if (f.topics.length > 0) {
      const nodeTopics: string[] = attrs.topics || [];
      if (!f.topics.some((t: string) => nodeTopics.includes(t))) return false;
    }
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

      // Run community detection for coloring
      if (graph.size > 0) {
        louvain.assign(graph);
      }

      // Assign colors by community and scale node sizes for mobile
      const mobile = isMobile();
      const sizeMultiplier = mobile ? 3 : 1;
      graph.forEachNode((node, attrs) => {
        const community = (attrs as any).community || 0;
        graph.setNodeAttribute(node, 'color', COLORS[community % COLORS.length]);
        const baseSize = (attrs as any).size || 1;
        graph.setNodeAttribute(node, 'size', baseSize * sizeMultiplier);
      });

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

<div class="graph-container" bind:this={container}>
  {#if $loading}
    <div class="overlay">Loading graph...</div>
  {/if}
  {#if $error}
    <div class="overlay error">{$error}</div>
  {/if}
</div>

<style>
  .graph-container {
    width: 100%;
    height: 100%;
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
</style>
