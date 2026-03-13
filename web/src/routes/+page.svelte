<script lang="ts">
  import { onMount } from 'svelte';
  import Graph from '$lib/components/Graph.svelte';
  import { loadGraph, selectedNode, graphData } from '$lib/stores/graph';

  onMount(() => {
    loadGraph();
  });
</script>

<div class="app">
  <header>
    <h1>postgraph</h1>
    <span class="stats">
      {#if $graphData}
        {$graphData.nodes.length} posts | {$graphData.edges.length} connections
      {/if}
    </span>
  </header>

  <main>
    <div class="graph-panel">
      <Graph />
    </div>

    {#if $selectedNode}
      <aside class="detail-panel">
        <h2>Post Detail</h2>
        <p>ID: {$selectedNode}</p>
        <!-- Post detail sidebar will be expanded later -->
      </aside>
    {/if}
  </main>
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: #0a0a0a;
    color: #eee;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid #333;
  }
  h1 { margin: 0; font-size: 1.2rem; }
  .stats { color: #888; font-size: 0.85rem; }
  main {
    display: flex;
    flex: 1;
    overflow: hidden;
  }
  .graph-panel {
    flex: 1;
  }
  .detail-panel {
    width: 320px;
    padding: 1rem;
    border-left: 1px solid #333;
    overflow-y: auto;
  }
</style>
