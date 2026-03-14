<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Sigma from 'sigma';
  import forceAtlas2 from 'graphology-layout-forceatlas2';
  import { tagGraphInstance, selectedTagNode, tagLoading, tagError } from '$lib/stores/tagGraph';
  import type Graph from 'graphology';

  let container: HTMLDivElement = $state(null!);
  let sigma: Sigma | null = $state(null);
  let categories: { name: string; color: string }[] = $state([]);

  function isMobile(): boolean {
    return typeof window !== 'undefined' && window.innerWidth < 768;
  }

  function initSigma(graph: Graph) {
    try {
      if (sigma) sigma.kill();

      const mobile = isMobile();
      const sizeMultiplier = mobile ? 3 : 1;
      graph.forEachNode((node, attrs) => {
        graph.setNodeAttribute(node, 'color', attrs.category_color || '#888');
        const baseSize = (attrs as any).size || 2;
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

      forceAtlas2.assign(graph, { iterations: 100, settings: { gravity: 1 } });

      sigma = new Sigma(graph, container, {
        renderEdgeLabels: false,
        defaultEdgeType: 'line',
        labelColor: { color: '#eeeeee' },
        labelSize: mobile ? 12 : 14,
        labelRenderedSizeThreshold: mobile ? 4 : 6,
      });

      sigma.on('clickNode', ({ node }) => {
        selectedTagNode.set(node);
      });

      sigma.on('clickStage', () => {
        selectedTagNode.set(null);
      });
    } catch (e) {
      tagError.set(e instanceof Error ? e.message : 'Failed to render tag graph');
    }
  }

  const unsubGraph = tagGraphInstance.subscribe((graph) => {
    if (graph && container) initSigma(graph);
  });

  onMount(() => {
    const graph = $tagGraphInstance;
    if (graph) initSigma(graph);
  });

  onDestroy(() => {
    unsubGraph();
    if (sigma) sigma.kill();
  });
</script>

<div class="graph-wrapper">
  <div class="graph-container" bind:this={container}>
    {#if $tagLoading}
      <div class="overlay">Loading tag graph...</div>
    {/if}
    {#if $tagError}
      <div class="overlay error">{$tagError}</div>
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
