<script lang="ts">
  import { page } from '$app/stores';
  import { onMount } from 'svelte';
  import { api } from '$lib/api';

  let { children } = $props();
  let unrepliedCount = $state(0);

  onMount(async () => {
    try {
      const data = await api.getReplyCount();
      unrepliedCount = data.count;
    } catch {
      // silently fail — badge just won't show
    }
  });
</script>

<div class="layout">
  <nav>
    <div class="nav-primary">
      <a href="/" class:active={$page.url.pathname === '/'}>Graph</a>
      <a href="/analytics-v2" class:active={$page.url.pathname === '/analytics-v2'}>Analytics</a>
      <a href="/insights" class:active={$page.url.pathname === '/insights'}>Insights</a>
      <a href="/compose" class:active={$page.url.pathname === '/compose'}>Compose</a>
      <a href="/replies" class:active={$page.url.pathname === '/replies'}>
        Replies{#if unrepliedCount > 0} ({unrepliedCount}){/if}
      </a>
    </div>
    <div class="nav-secondary">
      <a href="/analytics" class:active={$page.url.pathname === '/analytics'}>Legacy</a>
      <a href="/fourier" class:active={$page.url.pathname === '/fourier'}>f(t)</a>
      <a href="/debug" class:active={$page.url.pathname === '/debug'}>Debug</a>
      <a href="/health" class:active={$page.url.pathname === '/health'}>Health</a>
      <span class="nav-divider"></span>
      <a href="/logout" class="logout">Logout</a>
    </div>
  </nav>
  <div class="content">
    {@render children()}
  </div>
</div>

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
    background: #0a0a0a;
    color: #eee;
    font-size: 1rem;
    line-height: 1.55;
    font-kerning: normal;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;

    /* Type scale — major third (1.25) */
    --text-xs: 0.75rem;
    --text-sm: 0.875rem;
    --text-base: 1rem;
    --text-lg: 1.25rem;
    --text-xl: 1.5rem;

    /* Weights */
    --weight-normal: 400;
    --weight-medium: 500;
    --weight-semibold: 600;

    /* Spacing scale — 4pt base */
    --space-xs: 0.25rem;
    --space-sm: 0.5rem;
    --space-md: 0.75rem;
    --space-lg: 1rem;
    --space-xl: 1.5rem;
    --space-2xl: 2rem;
    --space-3xl: 3rem;
  }
  .layout { display: flex; flex-direction: column; height: 100vh; }
  nav {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--space-sm) var(--space-lg);
    border-bottom: 1px solid #1e1e1e;
    gap: var(--space-lg);
  }
  .nav-primary {
    display: flex;
    gap: var(--space-xs);
  }
  .nav-primary a {
    color: #888;
    text-decoration: none;
    padding: var(--space-xs) var(--space-sm);
    border-radius: 4px;
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    letter-spacing: 0.01em;
    transition: color 0.15s;
  }
  .nav-primary a:hover { color: #ccc; }
  .nav-primary a.active { color: #fff; background: #222; }
  .nav-secondary {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
  }
  .nav-secondary a {
    color: #555;
    text-decoration: none;
    padding: var(--space-xs) var(--space-sm);
    border-radius: 4px;
    font-size: var(--text-xs);
    letter-spacing: 0.01em;
    transition: color 0.15s;
  }
  .nav-secondary a:hover { color: #999; }
  .nav-secondary a.active { color: #aaa; background: #1a1a1a; }
  .nav-divider { width: 1px; height: 1rem; background: #2a2a2a; }
  .logout { color: #555; font-size: var(--text-xs); }
  .logout:hover { color: #e6194b; }
  .content { flex: 1; overflow-y: auto; min-height: 0; }

  @media (prefers-reduced-motion: reduce) {
    :global(*, *::before, *::after) {
      animation-duration: 0.01ms !important;
      animation-iteration-count: 1 !important;
      transition-duration: 0.01ms !important;
    }
  }
</style>
