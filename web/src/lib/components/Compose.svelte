<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type ScheduledPost } from '$lib/api';
  import ComposeModal from './ComposeModal.svelte';

  type ViewMode = 'week' | '2week' | 'month';

  let posts: ScheduledPost[] = $state([]);
  let loading = $state(true);
  let viewMode: ViewMode = $state('2week');
  let currentDate = $state(new Date());

  // Modal state
  let showModal = $state(false);
  let editingPost: ScheduledPost | null = $state(null);
  let modalInitialDate: Date | null = $state(null);

  // Calendar grid computation
  let calendarDays = $derived(computeCalendarDays(viewMode, currentDate));

  function computeCalendarDays(mode: ViewMode, anchor: Date): Date[] {
    const days: Date[] = [];
    const start = new Date(anchor);

    if (mode === 'week') {
      const day = start.getDay();
      start.setDate(start.getDate() - day);
      for (let i = 0; i < 7; i++) {
        const d = new Date(start);
        d.setDate(start.getDate() + i);
        days.push(d);
      }
    } else if (mode === '2week') {
      const day = start.getDay();
      start.setDate(start.getDate() - day);
      for (let i = 0; i < 14; i++) {
        const d = new Date(start);
        d.setDate(start.getDate() + i);
        days.push(d);
      }
    } else {
      start.setDate(1);
      const firstDay = start.getDay();
      start.setDate(start.getDate() - firstDay);
      for (let i = 0; i < 35; i++) {
        const d = new Date(start);
        d.setDate(start.getDate() + i);
        days.push(d);
      }
    }
    return days;
  }

  function navigate(direction: number) {
    const d = new Date(currentDate);
    if (viewMode === 'month') {
      d.setMonth(d.getMonth() + direction);
    } else if (viewMode === '2week') {
      d.setDate(d.getDate() + direction * 14);
    } else {
      d.setDate(d.getDate() + direction * 7);
    }
    currentDate = d;
  }

  function goToday() {
    currentDate = new Date();
  }

  function dateKey(d: Date): string {
    return d.toISOString().slice(0, 10);
  }

  function postsForDay(day: Date): ScheduledPost[] {
    const key = dateKey(day);
    return posts.filter(p => {
      if (p.status === 'published' || p.status === 'cancelled') return false;
      const postDate = p.scheduled_at ?? p.created_at;
      return postDate.slice(0, 10) === key;
    });
  }

  function isToday(d: Date): boolean {
    return dateKey(d) === dateKey(new Date());
  }

  function isCurrentMonth(d: Date): boolean {
    return d.getMonth() === currentDate.getMonth();
  }

  const dayNames = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

  const statusColors: Record<string, string> = {
    draft: '#666',
    scheduled: '#1a3a5c',
    publishing: '#3a3a15',
    published: '#1a4a2e',
    failed: '#5c1a1a',
    cancelled: '#333',
  };

  const statusDots: Record<string, string> = {
    draft: '#888',
    scheduled: '#6cb4ee',
    publishing: '#e6e64b',
    published: '#6be67a',
    failed: '#e6194b',
    cancelled: '#555',
  };

  function headerLabel(): string {
    if (viewMode === 'month') {
      return currentDate.toLocaleDateString('en-US', { month: 'long', year: 'numeric' });
    }
    const days = calendarDays;
    if (days.length === 0) return '';
    const first = days[0];
    const last = days[days.length - 1];
    const opts: Intl.DateTimeFormatOptions = { month: 'short', day: 'numeric' };
    return `${first.toLocaleDateString('en-US', opts)} – ${last.toLocaleDateString('en-US', opts)}, ${last.getFullYear()}`;
  }

  function openNewPost(day?: Date) {
    editingPost = null;
    modalInitialDate = day ?? null;
    showModal = true;
  }

  function openEditPost(post: ScheduledPost) {
    editingPost = post;
    modalInitialDate = null;
    showModal = true;
  }

  function closeModal() {
    showModal = false;
    editingPost = null;
    modalInitialDate = null;
  }

  async function onSaved() {
    closeModal();
    await loadPosts();
  }

  async function loadPosts() {
    try {
      const days = calendarDays;
      if (days.length === 0) return;
      const from = days[0].toISOString();
      const to = new Date(days[days.length - 1].getTime() + 86400000).toISOString();
      posts = await api.getScheduledPosts({ from, to });
    } catch {
      posts = [];
    }
    loading = false;
  }

  $effect(() => {
    calendarDays;
    loadPosts();
  });

  onMount(loadPosts);
</script>

<div class="compose-page">
  <div class="toolbar">
    <div class="nav-controls">
      <button onclick={() => navigate(-1)}>&larr;</button>
      <button class="today-btn" onclick={goToday}>Today</button>
      <button onclick={() => navigate(1)}>&rarr;</button>
      <span class="header-label">{headerLabel()}</span>
    </div>
    <div class="view-controls">
      <button class:active={viewMode === 'week'} onclick={() => viewMode = 'week'}>Week</button>
      <button class:active={viewMode === '2week'} onclick={() => viewMode = '2week'}>2 Weeks</button>
      <button class:active={viewMode === 'month'} onclick={() => viewMode = 'month'}>Month</button>
      <button class="new-post-btn" onclick={() => openNewPost()}>+ New Post</button>
    </div>
  </div>

  {#if loading}
    <div class="loading">Loading...</div>
  {:else}
    <div class="calendar" class:month-view={viewMode === 'month'}>
      <div class="day-headers">
        {#each dayNames as name}
          <div class="day-header">{name}</div>
        {/each}
      </div>
      <div class="day-grid" style="grid-template-columns: repeat(7, 1fr); grid-template-rows: repeat({Math.ceil(calendarDays.length / 7)}, 1fr);">
        {#each calendarDays as day}
          <!-- svelte-ignore a11y_interactive_supports_focus -->
          <div
            class="day-cell"
            class:today={isToday(day)}
            class:has-posts={postsForDay(day).length > 0}
            class:other-month={viewMode === 'month' && !isCurrentMonth(day)}
            role="button"
            onclick={() => openNewPost(day)}
            onkeydown={(e) => e.key === 'Enter' && openNewPost(day)}
          >
            <div class="day-number">{day.getDate()}</div>
            <div class="day-posts">
              {#each postsForDay(day) as p}
                <button
                  class="post-chip"
                  style="background: {statusColors[p.status]}; border-left: 3px solid {statusDots[p.status]};"
                  onclick={(e) => { e.stopPropagation(); openEditPost(p); }}
                >
                  <span class="chip-text">{p.text.slice(0, viewMode === 'month' ? 30 : 50)}{p.text.length > (viewMode === 'month' ? 30 : 50) ? '...' : ''}</span>
                  {#if p.scheduled_at}
                    <span class="chip-time">{new Date(p.scheduled_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>
                  {/if}
                </button>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

{#if showModal}
  <ComposeModal
    post={editingPost}
    initialDate={modalInitialDate}
    onclose={closeModal}
    onsaved={onSaved}
  />
{/if}

<style>
  .compose-page {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: var(--space-lg) var(--space-xl);
    gap: var(--space-xl);
  }
  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
  .nav-controls {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }
  .nav-controls button {
    background: #1a1a1a;
    border: 1px solid #2a2a2a;
    color: #999;
    padding: var(--space-xs) var(--space-sm);
    border-radius: 4px;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .nav-controls button:hover { border-color: #444; color: #ccc; }
  .today-btn { font-size: var(--text-sm); }
  .header-label { color: #eee; font-size: var(--text-xl); font-weight: var(--weight-semibold); margin-left: var(--space-md); letter-spacing: -0.02em; }
  .view-controls {
    display: flex;
    gap: var(--space-xs);
  }
  .view-controls button {
    background: #1a1a1a;
    border: 1px solid #2a2a2a;
    color: #888;
    padding: var(--space-xs) var(--space-md);
    border-radius: 4px;
    cursor: pointer;
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }
  .view-controls button:hover { border-color: #444; color: #ccc; }
  .view-controls button.active { color: #fff; background: #2a2a2a; border-color: #444; }
  .new-post-btn {
    background: #1a3a5c;
    color: #6cb4ee;
    border-color: #2a5a8c;
    margin-left: var(--space-sm);
  }
  .new-post-btn:hover { background: #1f4570; }
  .loading { color: #555; text-align: center; padding: var(--space-3xl); font-size: var(--text-sm); }
  .calendar {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .day-headers {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 2px;
    margin-bottom: var(--space-xs);
  }
  .day-header {
    text-align: center;
    font-size: var(--text-xs);
    color: #555;
    padding: var(--space-sm) 0;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-weight: var(--weight-medium);
  }
  .day-grid {
    display: grid;
    flex: 1;
    column-gap: 2px;
    row-gap: 2px;
    min-height: 0;
  }
  .day-cell {
    background: #0f0f0f;
    border: 1px solid transparent;
    border-radius: 4px;
    padding: var(--space-sm);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    cursor: pointer;
    text-align: left;
    color: inherit;
    font: inherit;
    min-height: 80px;
    transition: background 0.15s, border-color 0.15s;
  }
  .day-cell:hover { background: #161616; border-color: #2a2a2a; }
  .day-cell.has-posts { background: #141414; }
  .day-cell.today { background: #111318; border-color: #1a3a5c; }
  .day-cell.other-month { opacity: 0.3; }
  .day-number {
    font-size: var(--text-sm);
    color: #555;
    margin-bottom: var(--space-xs);
    font-variant-numeric: tabular-nums;
  }
  .today .day-number { color: #6cb4ee; font-weight: var(--weight-semibold); }
  .day-posts {
    display: flex;
    flex-direction: column;
    gap: 3px;
    overflow-y: auto;
    flex: 1;
  }
  .post-chip {
    padding: var(--space-xs) var(--space-sm);
    border-radius: 3px;
    font-size: var(--text-xs);
    cursor: pointer;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-xs);
    border: none;
    color: #ccc;
    text-align: left;
    font-family: inherit;
    line-height: 1.3;
    transition: filter 0.1s;
  }
  .post-chip:hover { filter: brightness(1.3); }
  .chip-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }
  .chip-time { color: #888; font-size: var(--text-xs); flex-shrink: 0; font-variant-numeric: tabular-nums; }
</style>
