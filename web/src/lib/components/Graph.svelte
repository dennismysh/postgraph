<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Sigma from 'sigma';
  import forceAtlas2 from 'graphology-layout-forceatlas2';
  import louvain from 'graphology-communities-louvain';
  import { graphInstance, selectedNode, loading } from '$lib/stores/graph';
  import type Graph from 'graphology';

  let container: HTMLDivElement;
  let sigma: Sigma | null = null;

  const COLORS = [
    '#e6194b', '#3cb44b', '#4363d8', '#f58231', '#911eb4',
    '#42d4f4', '#f032e6', '#bfef45', '#fabed4', '#469990',
  ];

  function initSigma(graph: Graph) {
    if (sigma) sigma.kill();

    // Run community detection for coloring
    louvain.assign(graph);

    // Assign colors by community
    graph.forEachNode((node, attrs) => {
      const community = (attrs as any).community || 0;
      graph.setNodeAttribute(node, 'color', COLORS[community % COLORS.length]);
    });

    // Run ForceAtlas2 layout
    forceAtlas2.assign(graph, { iterations: 100, settings: { gravity: 1 } });

    sigma = new Sigma(graph, container, {
      renderEdgeLabels: false,
      defaultEdgeType: 'line',
    });

    sigma.on('clickNode', ({ node }) => {
      selectedNode.set(node);
    });

    sigma.on('clickStage', () => {
      selectedNode.set(null);
    });
  }

  const unsubscribe = graphInstance.subscribe((graph) => {
    if (graph && container) initSigma(graph);
  });

  onMount(() => {
    const graph = $graphInstance;
    if (graph) initSigma(graph);
  });

  onDestroy(() => {
    unsubscribe();
    if (sigma) sigma.kill();
  });
</script>

<div class="graph-container" bind:this={container}>
  {#if $loading}
    <div class="loading">Loading graph...</div>
  {/if}
</div>

<style>
  .graph-container {
    width: 100%;
    height: 100%;
    position: relative;
  }
  .loading {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    color: #888;
  }
</style>
