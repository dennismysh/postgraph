<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type ReplyWithContext } from '$lib/api';

  type Filter = 'unreplied' | 'all';

  let replies: ReplyWithContext[] = $state([]);
  let loading = $state(true);
  let filter: Filter = $state('unreplied');

  // Inline reply state — keyed by reply ID
  let replyingTo: string | null = $state(null);
  let replyText = $state('');
  let sending = $state(false);
  let error = $state('');

  const MAX_LENGTH = 500;
  let charCount = $derived(replyText.length);
  let overLimit = $derived(charCount > MAX_LENGTH);

  async function loadReplies() {
    loading = true;
    try {
      const status = filter === 'all' ? undefined : 'unreplied';
      replies = await api.getReplies(status);
    } catch {
      replies = [];
    }
    loading = false;
  }

  function startReply(id: string) {
    replyingTo = id;
    replyText = '';
    error = '';
  }

  function cancelReply() {
    replyingTo = null;
    replyText = '';
    error = '';
  }

  async function sendReply(id: string) {
    if (replyText.trim().length === 0 || overLimit) return;
    sending = true;
    error = '';
    try {
      await api.sendReply(id, replyText);
      replyingTo = null;
      replyText = '';
      // Remove from list
      replies = replies.filter(r => r.id !== id);
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to send reply';
    } finally {
      sending = false;
    }
  }

  async function dismiss(id: string) {
    try {
      await api.dismissReply(id);
      replies = replies.filter(r => r.id !== id);
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to dismiss';
    }
  }

  function timeAgo(ts: string | null): string {
    if (!ts) return '';
    const diff = Date.now() - new Date(ts).getTime();
    const mins = Math.floor(diff / 60000);
    if (mins < 1) return 'just now';
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  }

  $effect(() => {
    filter;
    loadReplies();
  });

  onMount(loadReplies);
</script>

<div class="replies-page">
  <div class="toolbar">
    <h2>Replies</h2>
    <div class="filter-toggle">
      <button class:active={filter === 'unreplied'} onclick={() => filter = 'unreplied'}>Unreplied</button>
      <button class:active={filter === 'all'} onclick={() => filter = 'all'}>All</button>
    </div>
  </div>

  {#if loading}
    <div class="empty">Loading...</div>
  {:else if replies.length === 0}
    <div class="empty">
      {filter === 'unreplied' ? 'All caught up' : 'No replies yet'}
    </div>
  {:else}
    <div class="reply-list">
      {#each replies as reply (reply.id)}
        <div class="reply-card">
          <div class="parent-context">
            {reply.parent_post_text ?? 'Original post'}
          </div>
          <div class="reply-header">
            <span class="username">@{reply.username ?? 'unknown'}</span>
            <span class="time">{timeAgo(reply.timestamp)}</span>
            {#if reply.status !== 'unreplied'}
              <span class="status-badge" class:replied={reply.status === 'replied'} class:dismissed={reply.status === 'dismissed'}>
                {reply.status}
              </span>
            {/if}
          </div>
          <div class="reply-text">{reply.text ?? ''}</div>

          {#if reply.status === 'unreplied'}
            <div class="reply-actions">
              <button class="btn reply-btn" onclick={() => startReply(reply.id)}>Reply</button>
              <button class="btn dismiss-btn" onclick={() => dismiss(reply.id)}>Dismiss</button>
            </div>
          {/if}

          {#if replyingTo === reply.id}
            <div class="reply-compose">
              {#if error}
                <div class="error">{error}</div>
              {/if}
              <textarea
                bind:value={replyText}
                placeholder="Write a reply..."
                rows="3"
                disabled={sending}
              ></textarea>
              <div class="compose-footer">
                <span class="char-count" class:over={overLimit}>{charCount}/{MAX_LENGTH}</span>
                <div class="compose-actions">
                  <button class="btn cancel-btn" onclick={cancelReply} disabled={sending}>Cancel</button>
                  <button class="btn send-btn" onclick={() => sendReply(reply.id)} disabled={sending || overLimit || replyText.trim().length === 0}>
                    {sending ? 'Sending...' : 'Send'}
                  </button>
                </div>
              </div>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .replies-page {
    max-width: 700px;
    margin: 0 auto;
    padding: 1rem;
  }
  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
  }
  .toolbar h2 { margin: 0; }
  .filter-toggle {
    display: flex;
    gap: 0.25rem;
  }
  .filter-toggle button {
    background: #222;
    border: 1px solid #333;
    color: #888;
    padding: 0.3rem 0.75rem;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.85rem;
  }
  .filter-toggle button.active { color: #fff; background: #333; }
  .empty {
    color: #666;
    text-align: center;
    padding: 3rem;
    font-size: 1.1rem;
  }
  .reply-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .reply-card {
    background: #141414;
    border: 1px solid #222;
    border-radius: 6px;
    padding: 0.75rem 1rem;
  }
  .parent-context {
    font-size: 0.8rem;
    color: #555;
    border-left: 2px solid #333;
    padding-left: 0.5rem;
    margin-bottom: 0.5rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .reply-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.25rem;
  }
  .username { color: #6cb4ee; font-size: 0.85rem; font-weight: 500; }
  .time { color: #555; font-size: 0.8rem; }
  .status-badge {
    font-size: 0.7rem;
    padding: 0.1rem 0.4rem;
    border-radius: 3px;
    text-transform: uppercase;
  }
  .status-badge.replied { background: #1a4a2e; color: #6be67a; }
  .status-badge.dismissed { background: #333; color: #888; }
  .reply-text {
    color: #ccc;
    font-size: 0.95rem;
    line-height: 1.4;
    margin-bottom: 0.5rem;
  }
  .reply-actions {
    display: flex;
    gap: 0.5rem;
  }
  .btn {
    padding: 0.35rem 0.75rem;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.8rem;
  }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .reply-btn { background: #1a3a5c; color: #6cb4ee; }
  .dismiss-btn { background: #333; color: #888; }
  .cancel-btn { background: #333; color: #ccc; }
  .send-btn { background: #1a4a2e; color: #6be67a; }
  .reply-compose {
    margin-top: 0.5rem;
    border-top: 1px solid #222;
    padding-top: 0.5rem;
  }
  .error {
    background: #3a1515;
    border: 1px solid #e6194b;
    color: #ff6b6b;
    padding: 0.4rem 0.6rem;
    border-radius: 4px;
    font-size: 0.8rem;
    margin-bottom: 0.5rem;
  }
  textarea {
    width: 100%;
    background: #111;
    border: 1px solid #333;
    color: #eee;
    padding: 0.5rem;
    border-radius: 4px;
    resize: vertical;
    font-family: inherit;
    font-size: 0.9rem;
    box-sizing: border-box;
  }
  textarea:focus { outline: none; border-color: #555; }
  .compose-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: 0.35rem;
  }
  .char-count { font-size: 0.75rem; color: #666; }
  .char-count.over { color: #e6194b; }
  .compose-actions {
    display: flex;
    gap: 0.5rem;
  }
</style>
