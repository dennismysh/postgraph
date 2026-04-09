# Replies Tab — Inbox for Reply Management

**Date:** 2026-04-09
**Status:** Approved

## Overview

A new Replies tab in the postgraph dashboard for managing replies to your Threads posts. Inbox-style view prioritizing unreplied items (oldest first), with the ability to reply directly or dismiss. Background sync fetches replies for recent posts (last 7 days) every 15 minutes; older posts are on-demand.

## Data Model

### `replies` table

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT (PK) | Threads reply ID |
| `parent_post_id` | TEXT NOT NULL | FK to `posts.id` — the post this is a reply to |
| `username` | TEXT | Reply author's username |
| `text` | TEXT | Reply content |
| `timestamp` | TIMESTAMPTZ | When the reply was posted on Threads |
| `status` | TEXT NOT NULL | `unreplied`, `replied`, `dismissed` |
| `replied_at` | TIMESTAMPTZ | When you responded (NULL if unreplied/dismissed) |
| `our_reply_id` | TEXT | Threads ID of your response (NULL if not replied) |
| `synced_at` | TIMESTAMPTZ NOT NULL | When this reply was last fetched |

**Status lifecycle:**
```
unreplied --> replied    (user sends a reply)
unreplied --> dismissed  (user dismisses)
```

**Sync deduplication:** Upsert on `replies.id`. New replies insert with `status = 'unreplied'`. Existing replies update only `synced_at` — never overwrite `status`, `replied_at`, or `our_reply_id`.

## Backend

### Threads API methods (`threads.rs`)

**`get_post_replies(post_id)`** — Fetches replies for a single post.
- `GET /{post_id}/replies?fields=id,text,username,timestamp`
- Handles pagination (follows `paging.next` cursor)
- Returns `Vec<ThreadsReply>` with id, text, username, timestamp

**`create_reply(parent_id, text)`** — Two-step publish to reply to a post.
1. `POST /me/threads` with `media_type=TEXT`, `text={encoded}`, `reply_to_id={parent_id}` — returns container ID
2. `POST /me/threads_publish` with `creation_id={container_id}` — returns published reply ID

### Reply sync (`sync.rs`)

New function `sync_replies(pool, threads)`:
- Queries `posts` table for posts from the last 7 days
- For each: calls `get_post_replies()`, upserts into `replies` table
- New replies get `status = 'unreplied'`; existing replies only update `synced_at`
- Skips posts where the replies API returns an error (e.g. deleted posts)
- Rate limit aware: if 429, log and abort the cycle (catch up next time)

Added to the 15-min sync loop in `main.rs`, after `sync_post_metrics` and before `sync_daily_views`.

### API routes (`routes/replies.rs`)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/replies` | GET | List replies, filterable by `status` (default: `unreplied`), ordered oldest first. Returns replies with parent post text snippet. |
| `/api/replies/count` | GET | Count of unreplied replies (for nav badge) |
| `/api/replies/{id}/reply` | POST | Send a reply (text in JSON body). Sets status to `replied`, stores `our_reply_id` and `replied_at`. |
| `/api/replies/{id}/dismiss` | POST | Mark reply as dismissed |

The list endpoint joins with `posts` to include `parent_post_text` (first 80 chars of the parent post's text) so the frontend has context without a second call.

## Frontend

### New route: `/replies`

Added to nav between Compose and Fourier. Nav link shows unreplied count badge: "Replies (3)" or just "Replies" when count is 0.

The unreplied count is fetched on layout mount via `/api/replies/count` and refreshed when the Replies page is visited.

### Page layout

Single-column inbox list. Top bar has a filter toggle: **Unreplied** (default) | **All**.

Each reply card shows:
- **Parent post context** — first ~80 chars of parent post text, muted color, as a header
- **Username** — reply author
- **Reply text** — full content
- **Timestamp** — relative (e.g. "2h ago")
- **Actions** — "Reply" and "Dismiss" buttons

### Reply action

Clicking "Reply" expands an inline text area below the reply card:
- Text area with live character count (500 limit)
- Send button + Cancel button
- On send: calls `/api/replies/{id}/reply`, card transitions out of inbox on success
- On error: shows error message inline below text area, card stays

### Dismiss action

Clicking "Dismiss" immediately calls `/api/replies/{id}/dismiss` and removes the card from the inbox. No confirmation dialog.

### Empty state

When no unreplied replies exist, show "All caught up" centered message.

## Error Handling & Edge Cases

### Reply send failures
Show error inline below the text area. Reply stays in inbox (status unchanged) for retry.

### Rate limiting
- Background sync: if 429, log and skip cycle. No user-visible error.
- Sending a reply: show "Rate limited — try again in a minute" inline.

### Deleted parent posts
If the replies API returns an error for a post (e.g. deleted), skip that post during sync. Don't crash the cycle.

### Sync deduplication
Upsert on `replies.id`. Only update `synced_at`. Never overwrite user-set fields (`status`, `replied_at`, `our_reply_id`).

### Empty inbox
Show "All caught up" message when filter is set to "Unreplied" and there are no unreplied replies.

## Out of Scope (V1)

- Reply analytics (sentiment, response times, top engagers)
- Nested reply threads (replies to replies)
- Bulk actions (dismiss all, mark all as read)
- Media in replies
- On-demand fetch for posts older than 7 days (sync only covers recent posts)
