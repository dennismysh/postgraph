<script lang="ts">
  import { onMount } from 'svelte';
  import Graph from '$lib/components/Graph.svelte';
  import FilterBar from '$lib/components/FilterBar.svelte';
  import { loadGraph, selectedNode, graphData } from '$lib/stores/graph';

  onMount(() => {
    loadGraph();
  });
</script>

<div class="app">
  <FilterBar />
  <main>
    <div class="graph-panel">
      <Graph />
    </div>
    {#if $selectedNode}
      <aside class="detail-panel">
        <h2>Post Detail</h2>
        <p>ID: {$selectedNode}</p>
      </aside>
    {/if}
  </main>
</div>

<style>
  .app { display: flex; flex-direction: column; height: 100%; }
  main { display: flex; flex: 1; overflow: hidden; }
  .graph-panel { flex: 1; }
  .detail-panel {
    width: 320px;
    padding: 1rem;
    border-left: 1px solid #333;
    overflow-y: auto;
  }
</style>
