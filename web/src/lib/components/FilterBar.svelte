<script lang="ts">
  import { filters, resetFilters } from '$lib/stores/filters';
  import { graphData } from '$lib/stores/graph';

  let allTopics: string[] = [];
  $: if ($graphData) {
    const topicSet = new Set<string>();
    for (const node of $graphData.nodes) {
      for (const t of node.topics) topicSet.add(t);
    }
    allTopics = [...topicSet].sort();
  }

  function toggleTopic(topic: string) {
    filters.update(f => {
      const topics = f.topics.includes(topic)
        ? f.topics.filter(t => t !== topic)
        : [...f.topics, topic];
      return { ...f, topics };
    });
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
        on:click={() => toggleTopic(topic)}
      >
        {topic}
      </button>
    {/each}
  </div>

  <button class="reset" on:click={resetFilters}>Reset</button>
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
  .reset {
    background: #333;
    border: 1px solid #555;
    color: #ccc;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
    cursor: pointer;
  }
  label { color: #888; font-size: 0.8rem; }
</style>
