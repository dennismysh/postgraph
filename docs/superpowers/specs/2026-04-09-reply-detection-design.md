# Reply Detection: Auto-detect externally replied threads

**Date:** 2026-04-09
**Status:** Approved

## Problem

The Replies page shows 427 unreplied entries, but many have already been replied to directly on Threads (outside postgraph). There's no way to detect this, forcing manual Skip on each one.

## Solution

Use the Threads `/conversation` endpoint to detect replies from the authenticated user, and automatically mark those as `replied`.

## Design

### 1. Username Resolution

- At server startup, call `GET /me?fields=id,username` once using the existing access token
- Store the returned username in `ThreadsClient` (e.g. `owner_username: String`)
- This is a single cheap call that runs once per server lifetime

### 2. Conversation Fetching

New method on `ThreadsClient`:

```
get_conversation(post_id: &str) -> Result<Vec<ThreadsReply>, AppError>
```

- Calls `GET /{post_id}/conversation?fields=id,text,username,timestamp`
- Supports pagination (same cursor pattern as `get_post_replies`)
- 100ms delay between pagination requests (same rate-limit pattern)

### 3. Detection Logic

New function in `sync.rs`:

```
detect_external_replies(pool, client) -> Result<u64, AppError>
```

1. Query DB: all replies with `status = 'unreplied'`, grouped by `parent_post_id`
2. For each unique parent post:
   a. Call `get_conversation(parent_post_id)` — returns full thread tree
   b. Build a set of reply IDs where `username == client.owner_username`
   c. For each unreplied reply under this parent: if any of our replies has a timestamp *after* the unreplied reply's timestamp, mark it as `replied` with `replied_at = now()`
3. Rate-limit: 100ms delay between conversation API calls
4. Return count of replies marked as replied

### 4. Triggers

**Manual (debug page):**
- New endpoint: `POST /api/replies/detect`
- Runs detection against *all* unreplied replies
- Returns `{ detected: u64 }` — count of replies auto-marked
- "Detect Replies" button on the debug/sync page

**Incremental (automatic):**
- After `sync_replies` inserts new replies, run detection but only for parent posts that received new unreplied replies in this sync cycle
- Runs on both the 15-min interval sync and nightly sync

### 5. Status Handling

- Detected replies get `status = 'replied'` — same as replies sent through postgraph
- No new status values; existing UI filters work unchanged
- `our_reply_id` is left NULL (we don't track the external reply's ID)
- `replied_at` is set to `now()` (detection time, not actual reply time)

### 6. Frontend

- **Replies page:** No changes. Unreplied filter already hides `replied` entries, All view shows "Replied" badge.
- **Debug page:** Add "Detect Replies" button that calls `POST /api/replies/detect` and shows the count of detected replies.

## Scope

### In scope
- `GET /me` username resolution at startup
- `get_conversation` method on `ThreadsClient`
- `detect_external_replies` function in sync
- `POST /api/replies/detect` endpoint
- Debug page button
- Incremental detection in regular sync flow

### Out of scope
- Separate `replied_external` status
- Detecting replies older than the current 7-day sync window
- Pagination of the Replies page UI
