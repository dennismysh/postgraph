<script lang="ts">
  import { onMount } from 'svelte';
  import Graph from '$lib/components/Graph.svelte';
  import FilterBar from '$lib/components/FilterBar.svelte';
  import { loadGraph } from '$lib/stores/graph';

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
  </main>
</div>

<style>
  .app { display: flex; flex-direction: column; height: 100%; }
  main { display: flex; flex: 1; overflow: hidden; min-height: 0; }
  .graph-panel { flex: 1; min-height: 0; position: relative; }
</style>
