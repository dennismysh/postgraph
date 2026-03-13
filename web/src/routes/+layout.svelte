<script lang="ts">
  import { page } from '$app/stores';

  let { children } = $props();
</script>

<div class="layout">
  <nav>
    <a href="/" class:active={$page.url.pathname === '/'}>Graph</a>
    <a href="/analytics" class:active={$page.url.pathname === '/analytics'}>Analytics</a>
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
    gap: 1rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid #333;
  }
  nav a {
    color: #888;
    text-decoration: none;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
  }
  nav a.active { color: #fff; background: #333; }
  .content { flex: 1; overflow: hidden; }
</style>
