<script lang="ts">
  import { api, type ScheduledPost } from '$lib/api';

  interface Props {
    post?: ScheduledPost | null;
    initialDate?: Date | null;
    onclose: () => void;
    onsaved: () => void;
  }

  let { post = null, initialDate = null, onclose, onsaved }: Props = $props();

  const MAX_LENGTH = 500;

  let text = $state(post?.text ?? '');
  let scheduledDate = $state(formatDateForInput(
    post?.scheduled_at ? new Date(post.scheduled_at) : initialDate
  ));
  let scheduledTime = $state(formatTimeForInput(
    post?.scheduled_at ? new Date(post.scheduled_at) : initialDate
  ));
  let saving = $state(false);
  let error = $state('');

  function formatDateForInput(d: Date | null | undefined): string {
    if (!d) return '';
    const year = d.getFullYear();
    const month = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    return `${year}-${month}-${day}`;
  }

  function formatTimeForInput(d: Date | null | undefined): string {
    if (!d) return '09:00';
    return d.toTimeString().slice(0, 5);
  }

  function getScheduledAt(): string | undefined {
    if (!scheduledDate || !scheduledTime) return undefined;
    return new Date(`${scheduledDate}T${scheduledTime}`).toISOString();
  }

  let isEditing = $derived(post !== null && post !== undefined);
  let charCount = $derived(text.length);
  let overLimit = $derived(charCount > MAX_LENGTH);

  async function save(action: 'draft' | 'schedule' | 'publish') {
    if (overLimit || text.trim().length === 0) return;
    saving = true;
    error = '';

    try {
      if (action === 'publish' && isEditing) {
        await api.updateScheduledPost(post!.id, { text });
        await api.publishNow(post!.id);
      } else if (action === 'publish' && !isEditing) {
        const created = await api.createScheduledPost({ text, status: 'draft' });
        await api.publishNow(created.id);
      } else if (isEditing) {
        const scheduled_at = action === 'schedule' ? getScheduledAt() : undefined;
        await api.updateScheduledPost(post!.id, {
          text,
          status: action === 'schedule' ? 'scheduled' : 'draft',
          scheduled_at: action === 'schedule' ? scheduled_at ?? null : undefined,
        });
      } else {
        const scheduled_at = action === 'schedule' ? getScheduledAt() : undefined;
        await api.createScheduledPost({
          text,
          status: action === 'schedule' ? 'scheduled' : 'draft',
          scheduled_at,
        });
      }
      onsaved();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Something went wrong';
    } finally {
      saving = false;
    }
  }

  async function cancelPost() {
    if (!post) return;
    saving = true;
    error = '';
    try {
      await api.updateScheduledPost(post.id, { status: 'cancelled' });
      onsaved();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to cancel';
    } finally {
      saving = false;
    }
  }

  async function deletePost() {
    if (!post) return;
    saving = true;
    error = '';
    try {
      await api.deleteScheduledPost(post.id);
      onsaved();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to delete';
    } finally {
      saving = false;
    }
  }

  async function retryPost() {
    if (!post) return;
    saving = true;
    error = '';
    try {
      await api.updateScheduledPost(post.id, {
        status: 'scheduled',
        scheduled_at: new Date().toISOString(),
      });
      onsaved();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to retry';
    } finally {
      saving = false;
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onclose}>
  <div class="modal" onclick={(e) => e.stopPropagation()}>
    <div class="header">
      <h3>{isEditing ? 'Edit Post' : 'New Post'}</h3>
      <button class="close-btn" onclick={onclose}>&times;</button>
    </div>

    {#if error}
      <div class="error">{error}</div>
    {/if}

    {#if post?.status === 'failed' && post.error_message}
      <div class="error">Last error: {post.error_message}</div>
    {/if}

    <textarea
      bind:value={text}
      placeholder="What's on your mind?"
      rows="6"
      disabled={saving || post?.status === 'published'}
    ></textarea>

    <div class="char-count" class:over={overLimit}>
      {charCount}/{MAX_LENGTH}
    </div>

    <div class="schedule-row">
      <label>
        Date
        <input type="date" bind:value={scheduledDate} disabled={saving} />
      </label>
      <label>
        Time
        <input type="time" bind:value={scheduledTime} disabled={saving} />
      </label>
    </div>

    <div class="actions">
      {#if post?.status === 'failed'}
        <button class="btn retry" onclick={retryPost} disabled={saving}>Retry</button>
      {/if}
      {#if post?.status === 'scheduled'}
        <button class="btn cancel" onclick={cancelPost} disabled={saving}>Cancel Post</button>
      {/if}
      {#if post?.status === 'draft'}
        <button class="btn delete" onclick={deletePost} disabled={saving}>Delete Draft</button>
      {/if}

      {#if post?.status !== 'published' && post?.status !== 'cancelled'}
        <div class="primary-actions">
          <button class="btn draft" onclick={() => save('draft')} disabled={saving || overLimit}>
            Save as Draft
          </button>
          <button class="btn schedule" onclick={() => save('schedule')} disabled={saving || overLimit || !scheduledDate}>
            Schedule
          </button>
          <button class="btn publish" onclick={() => save('publish')} disabled={saving || overLimit}>
            Post Now
          </button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0,0,0,0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .modal {
    background: #1a1a1a;
    border: 1px solid #2a2a2a;
    border-radius: 8px;
    padding: var(--space-xl);
    width: 90%;
    max-width: 520px;
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
  }
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .header h3 { margin: 0; font-size: var(--text-lg); font-weight: var(--weight-semibold); letter-spacing: -0.01em; }
  .close-btn {
    background: none;
    border: none;
    color: #888;
    font-size: 1.5rem;
    cursor: pointer;
  }
  .error {
    background: #3a1515;
    border: 1px solid #e6194b;
    color: #ff6b6b;
    padding: 0.5rem 0.75rem;
    border-radius: 4px;
    font-size: var(--text-sm);
  }
  textarea {
    background: #111;
    border: 1px solid #333;
    color: #eee;
    padding: 0.75rem;
    border-radius: 4px;
    resize: vertical;
    font-family: inherit;
    font-size: var(--text-base);
    line-height: 1.6;
  }
  textarea:focus { outline: none; border-color: #555; }
  .char-count { text-align: right; font-size: var(--text-xs); color: #555; font-variant-numeric: tabular-nums; }
  .char-count.over { color: #e6194b; }
  .schedule-row {
    display: flex;
    gap: var(--space-lg);
  }
  .schedule-row label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: var(--text-xs);
    color: #666;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: var(--weight-medium);
  }
  .schedule-row input {
    background: #111;
    border: 1px solid #333;
    color: #eee;
    padding: 0.4rem;
    border-radius: 4px;
  }
  .actions {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-sm);
    margin-top: var(--space-md);
    padding-top: var(--space-md);
    border-top: 1px solid #222;
  }
  .primary-actions {
    display: flex;
    gap: var(--space-sm);
    margin-left: auto;
  }
  .btn {
    padding: 0.5rem 1rem;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn.draft { background: #333; color: #ccc; }
  .btn.schedule { background: #1a3a5c; color: #6cb4ee; }
  .btn.publish { background: #1a4a2e; color: #6be67a; }
  .btn.cancel { background: #3a2a15; color: #e6a64b; }
  .btn.delete { background: #3a1515; color: #e6194b; }
  .btn.retry { background: #1a3a5c; color: #6cb4ee; }
</style>
