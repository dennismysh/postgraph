<script lang="ts">
  type ServiceStatus = {
    status: string;
    error?: string;
  };

  type HealthData = {
    status: string;
    database: ServiceStatus;
    threads_api: ServiceStatus;
    mercury_api: ServiceStatus;
  };

  let health: HealthData | null = $state(null);
  let loading = $state(true);
  let fetchError: string | null = $state(null);

  async function fetchHealth() {
    loading = true;
    fetchError = null;
    try {
      const res = await fetch('/api/health');
      if (!res.ok) {
        fetchError = `HTTP ${res.status}`;
        return;
      }
      health = await res.json();
    } catch (e) {
      fetchError = e instanceof Error ? e.message : 'Unknown error';
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    fetchHealth();
  });
</script>

<div class="health-page">
  <h1>System Health</h1>

  {#if loading}
    <p class="loading">Checking services...</p>
  {:else if fetchError}
    <div class="card error">
      <p>Failed to fetch health status: {fetchError}</p>
      <button onclick={fetchHealth}>Retry</button>
    </div>
  {:else if health}
    <div class="overall" class:ok={health.status === 'ok'} class:degraded={health.status === 'degraded'}>
      Overall: {health.status.toUpperCase()}
    </div>

    <div class="services">
      <div class="card" class:ok={health.database.status === 'ok'} class:error={health.database.status !== 'ok'}>
        <h2>Database</h2>
        <span class="badge">{health.database.status}</span>
        {#if health.database.error}
          <p class="error-msg">{health.database.error}</p>
        {/if}
      </div>

      <div class="card" class:ok={health.threads_api.status === 'ok'} class:error={health.threads_api.status !== 'ok'}>
        <h2>Threads API</h2>
        <span class="badge">{health.threads_api.status}</span>
        {#if health.threads_api.error}
          <p class="error-msg">{health.threads_api.error}</p>
        {/if}
      </div>

      <div class="card" class:ok={health.mercury_api.status === 'ok'} class:error={health.mercury_api.status !== 'ok'}>
        <h2>Mercury LLM</h2>
        <span class="badge">{health.mercury_api.status}</span>
        {#if health.mercury_api.error}
          <p class="error-msg">{health.mercury_api.error}</p>
        {/if}
      </div>
    </div>

    <button class="refresh" onclick={fetchHealth}>Refresh</button>
  {/if}
</div>

<style>
  .health-page {
    max-width: 640px;
    margin: 2rem auto;
    padding: 0 1rem;
  }
  h1 {
    font-size: 1.5rem;
    margin-bottom: 1.5rem;
  }
  .loading {
    color: #888;
  }
  .overall {
    font-size: 1.1rem;
    font-weight: 600;
    padding: 0.75rem 1rem;
    border-radius: 6px;
    margin-bottom: 1.5rem;
    text-align: center;
  }
  .overall.ok {
    background: #0d3320;
    color: #4ade80;
    border: 1px solid #166534;
  }
  .overall.degraded {
    background: #3b1a0a;
    color: #fb923c;
    border: 1px solid #9a3412;
  }
  .services {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .card {
    padding: 1rem 1.25rem;
    border-radius: 6px;
    border: 1px solid #333;
    background: #141414;
    display: flex;
    align-items: center;
    gap: 1rem;
    flex-wrap: wrap;
  }
  .card h2 {
    font-size: 1rem;
    margin: 0;
    min-width: 120px;
  }
  .badge {
    font-size: 0.8rem;
    font-weight: 600;
    text-transform: uppercase;
    padding: 0.2rem 0.5rem;
    border-radius: 4px;
  }
  .card.ok .badge {
    background: #0d3320;
    color: #4ade80;
  }
  .card.error .badge {
    background: #3b0a0a;
    color: #f87171;
  }
  .error-msg {
    width: 100%;
    margin: 0.5rem 0 0;
    font-size: 0.85rem;
    color: #f87171;
    word-break: break-word;
  }
  .refresh {
    margin-top: 1.5rem;
    padding: 0.5rem 1.25rem;
    background: #222;
    color: #eee;
    border: 1px solid #444;
    border-radius: 4px;
    cursor: pointer;
  }
  .refresh:hover {
    background: #333;
  }
  button {
    padding: 0.5rem 1rem;
    background: #222;
    color: #eee;
    border: 1px solid #444;
    border-radius: 4px;
    cursor: pointer;
  }
  button:hover {
    background: #333;
  }
</style>
