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
      const diff = day === 0 ? -6 : 1 - day;
      start.setDate(start.getDate() + diff);
      for (let i = 0; i < 7; i++) {
        const d = new Date(start);
        d.setDate(start.getDate() + i);
        days.push(d);
      }
    } else if (mode === '2week') {
      const day = start.getDay();
      const diff = day === 0 ? -6 : 1 - day;
      start.setDate(start.getDate() + diff);
      for (let i = 0; i < 14; i++) {
        const d = new Date(start);
        d.setDate(start.getDate() + i);
        days.push(d);
      }
    } else {
      start.setDate(1);
      const firstDay = start.getDay();
      const leadingDays = firstDay === 0 ? 6 : firstDay - 1;
      start.setDate(start.getDate() - leadingDays);
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

  const dayNames = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

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
    padding: 1rem;
    gap: 0.75rem;
  }
  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .nav-controls {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .nav-controls button {
    background: #222;
    border: 1px solid #333;
    color: #ccc;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
    cursor: pointer;
  }
  .today-btn { font-size: 0.85rem; }
  .header-label { color: #ccc; font-size: 1rem; font-weight: 500; margin-left: 0.5rem; }
  .view-controls {
    display: flex;
    gap: 0.5rem;
  }
  .view-controls button {
    background: #222;
    border: 1px solid #333;
    color: #888;
    padding: 0.3rem 0.75rem;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.85rem;
  }
  .view-controls button.active { color: #fff; background: #333; }
  .new-post-btn {
    background: #1a3a5c !important;
    color: #6cb4ee !important;
    border-color: #2a5a8c !important;
  }
  .loading { color: #888; text-align: center; padding: 3rem; }
  .calendar {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .day-headers {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 1px;
  }
  .day-header {
    text-align: center;
    font-size: 0.8rem;
    color: #666;
    padding: 0.3rem 0;
  }
  .day-grid {
    display: grid;
    flex: 1;
    gap: 1px;
    min-height: 0;
  }
  .day-cell {
    background: #141414;
    border: 1px solid #222;
    border-radius: 4px;
    padding: 0.3rem;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    cursor: pointer;
    text-align: left;
    color: inherit;
    font: inherit;
    min-height: 80px;
  }
  .day-cell:hover { border-color: #444; }
  .day-cell.today { border-color: #6cb4ee; }
  .day-cell.other-month { opacity: 0.4; }
  .day-number {
    font-size: 0.75rem;
    color: #888;
    margin-bottom: 0.25rem;
  }
  .today .day-number { color: #6cb4ee; font-weight: 600; }
  .day-posts {
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
    flex: 1;
  }
  .post-chip {
    padding: 0.2rem 0.4rem;
    border-radius: 3px;
    font-size: 0.7rem;
    cursor: pointer;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.25rem;
    border: none;
    color: #ccc;
    text-align: left;
    font: inherit;
    font-size: 0.7rem;
  }
  .post-chip:hover { filter: brightness(1.3); }
  .chip-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }
  .chip-time { color: #888; font-size: 0.65rem; flex-shrink: 0; }
</style>
