<script lang="ts">
  import { page } from '$app/stores';

  let { children } = $props();
</script>

<div class="layout">
  <nav>
    <div class="nav-links">
      <a href="/" class:active={$page.url.pathname === '/'}>Graph</a>
      <a href="/analytics" class:active={$page.url.pathname === '/analytics'}>Analytics</a>
      <a href="/analytics-v2" class:active={$page.url.pathname === '/analytics-v2'}>V2</a>
      <a href="/insights" class:active={$page.url.pathname === '/insights'}>Insights</a>
      <a href="/fourier" class:active={$page.url.pathname === '/fourier'}>ƒ(t)</a>
      <a href="/debug" class:active={$page.url.pathname === '/debug'}>Debug</a>
      <a href="/health" class:active={$page.url.pathname === '/health'}>Health</a>
    </div>
    <a href="/logout" class="logout">Logout</a>
  </nav>
  <div class="content">
    {@render children()}
  </div>
</div>

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    background: #0a0a0a;
    color: #eee;
  }
  .layout { display: flex; flex-direction: column; height: 100vh; }
  nav {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid #333;
  }
  .nav-links {
    display: flex;
    gap: 1rem;
  }
  nav a {
    color: #888;
    text-decoration: none;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
  }
  nav a.active { color: #fff; background: #333; }
  .logout { color: #888; font-size: 0.85rem; }
  .logout:hover { color: #e6194b; }
  .content { flex: 1; overflow-y: auto; min-height: 0; }
</style>
