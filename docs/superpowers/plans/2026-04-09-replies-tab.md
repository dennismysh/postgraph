# Replies Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Replies tab with an inbox for managing replies to your Threads posts — sync, read, respond, and dismiss.

**Architecture:** New `replies` table stores synced replies with status tracking. Background sync fetches replies for posts from the last 7 days every 15 minutes. Backend provides list/count/reply/dismiss endpoints. Frontend shows a prioritized inbox (oldest unreplied first) with inline reply composition.

**Tech Stack:** Rust (axum, sqlx, tokio, chrono), Svelte 5, Threads API (replies + two-step publish with reply_to_id)

**Spec:** `docs/superpowers/specs/2026-04-09-replies-tab-design.md`

**Lesson from Compose Tab:** Every frontend `/api/*` call goes through SvelteKit server-side proxy routes. Always create `+server.ts` files in `web/src/routes/api/` that proxy to the Rust backend via `proxyToBackend()`.

---

## File Structure

### Backend (postgraph-server/src/)
- **Create:** `migrations/016_replies.sql` — Database migration
- **Create:** `replies.rs` — Business logic: CRUD for replies, sync upsert
- **Create:** `routes/replies.rs` — HTTP handlers for replies API
- **Modify:** `threads.rs` — Add `get_post_replies()` and `create_reply()` methods
- **Modify:** `sync.rs` — Add `sync_replies()` function
- **Modify:** `routes/mod.rs` — Register `replies` module
- **Modify:** `main.rs` — Register routes, add `sync_replies` to 15-min sync loop

### Frontend (web/src/)
- **Create:** `routes/api/replies/+server.ts` — SvelteKit proxy (GET list)
- **Create:** `routes/api/replies/count/+server.ts` — SvelteKit proxy (GET count)
- **Create:** `routes/api/replies/[id]/reply/+server.ts` — SvelteKit proxy (POST reply)
- **Create:** `routes/api/replies/[id]/dismiss/+server.ts` — SvelteKit proxy (POST dismiss)
- **Create:** `routes/replies/+page.svelte` — Route wrapper
- **Create:** `lib/components/Replies.svelte` — Inbox component
- **Modify:** `lib/api.ts` — Add reply types and API methods
- **Modify:** `routes/+layout.svelte` — Add Replies nav link with count badge

---

## Task 1: Database Migration

**Files:**
- Create: `postgraph-server/migrations/016_replies.sql`

- [ ] **Step 1: Create migration file**

```sql
CREATE TABLE replies (
    id TEXT PRIMARY KEY,
    parent_post_id TEXT NOT NULL,
    username TEXT,
    text TEXT,
    timestamp TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'unreplied',
    replied_at TIMESTAMPTZ,
    our_reply_id TEXT,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_replies_status ON replies (status) WHERE status = 'unreplied';
CREATE INDEX idx_replies_parent ON replies (parent_post_id);
```

- [ ] **Step 2: Verify migration compiles**

Run: `cd postgraph-server && cargo check`

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/migrations/016_replies.sql
git commit -m "feat(replies): add replies table migration"
```

---

## Task 2: Threads API Methods for Replies

**Files:**
- Modify: `postgraph-server/src/threads.rs`

- [ ] **Step 1: Add reply response types**

Add after the existing `PublishResponse` struct:

```rust
#[derive(Debug, Deserialize)]
pub struct ThreadsReply {
    pub id: String,
    pub text: Option<String>,
    pub username: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RepliesResponse {
    pub data: Vec<ThreadsReply>,
    pub paging: Option<ThreadsPaging>,
}
```

- [ ] **Step 2: Add get_post_replies method**

Add inside the `impl ThreadsClient` block, after the `publish_container` method:

```rust
    /// Fetch replies to a specific post.
    pub async fn get_post_replies(&self, post_id: &str) -> Result<Vec<ThreadsReply>, AppError> {
        let mut all_replies = Vec::new();
        let mut url = format!(
            "{}/{}/replies?fields=id,text,username,timestamp&access_token={}",
            BASE_URL,
            post_id,
            self.token().await
        );

        loop {
            let resp = self.client.get(&url).send().await?;
            if resp.status() == 429 {
                return Err(AppError::RateLimited(60));
            }
            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(AppError::ThreadsApi(format!(
                    "Get replies failed for {post_id}: {body}"
                )));
            }

            let data: RepliesResponse = resp.json().await?;
            all_replies.extend(data.data);

            let has_next = data
                .paging
                .as_ref()
                .and_then(|p| p.next.as_ref())
                .is_some();
            if !has_next {
                break;
            }

            let next_cursor = data
                .paging
                .as_ref()
                .and_then(|p| p.cursors.as_ref())
                .and_then(|c| c.after.clone());
            match next_cursor {
                Some(cursor) => {
                    url = format!(
                        "{}/{}/replies?fields=id,text,username,timestamp&after={}&access_token={}",
                        BASE_URL,
                        post_id,
                        cursor,
                        self.token().await
                    );
                }
                None => break,
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        Ok(all_replies)
    }

    /// Create a reply to a post. Two-step publish with reply_to_id.
    pub async fn create_reply(&self, reply_to_id: &str, text: &str) -> Result<String, AppError> {
        let url = format!(
            "{}/me/threads?media_type=TEXT&text={}&reply_to_id={}&access_token={}",
            BASE_URL,
            urlencoding::encode(text),
            reply_to_id,
            self.token().await
        );

        let resp = self.client.post(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::ThreadsApi(format!(
                "Create reply container failed: {body}"
            )));
        }
        let data: CreateContainerResponse = resp.json().await?;
        let container_id = data.id;

        // Step 2: publish the container
        self.publish_container(&container_id).await
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd postgraph-server && cargo check`

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/threads.rs
git commit -m "feat(replies): add get_post_replies and create_reply to ThreadsClient"
```

---

## Task 3: Replies Business Logic Module

**Files:**
- Create: `postgraph-server/src/replies.rs`
- Modify: `postgraph-server/src/main.rs` (add `mod replies;`)

- [ ] **Step 1: Add module declaration**

In `postgraph-server/src/main.rs`, add `mod replies;` to the module list (alphabetically, after `mod mercury;`).

- [ ] **Step 2: Create replies.rs**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Reply {
    pub id: String,
    pub parent_post_id: String,
    pub username: Option<String>,
    pub text: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    pub status: String,
    pub replied_at: Option<DateTime<Utc>>,
    pub our_reply_id: Option<String>,
    pub synced_at: DateTime<Utc>,
}

/// Reply with parent post context for the inbox view.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ReplyWithContext {
    pub id: String,
    pub parent_post_id: String,
    pub username: Option<String>,
    pub text: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    pub status: String,
    pub replied_at: Option<DateTime<Utc>>,
    pub our_reply_id: Option<String>,
    pub synced_at: DateTime<Utc>,
    pub parent_post_text: Option<String>,
}

/// Upsert a reply from the Threads API. New replies get status 'unreplied'.
/// Existing replies only update synced_at — never overwrite status.
pub async fn upsert_reply(
    pool: &PgPool,
    id: &str,
    parent_post_id: &str,
    username: Option<&str>,
    text: Option<&str>,
    timestamp: Option<DateTime<Utc>>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO replies (id, parent_post_id, username, text, timestamp)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (id) DO UPDATE SET synced_at = now()"
    )
    .bind(id)
    .bind(parent_post_id)
    .bind(username)
    .bind(text)
    .bind(timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

/// List replies with parent post context.
pub async fn list(pool: &PgPool, status: Option<&str>) -> Result<Vec<ReplyWithContext>, AppError> {
    let rows = sqlx::query_as::<_, ReplyWithContext>(
        "SELECT r.*, LEFT(p.text, 80) AS parent_post_text
         FROM replies r
         LEFT JOIN posts p ON r.parent_post_id = p.id
         WHERE ($1::text IS NULL OR r.status = $1)
         ORDER BY CASE WHEN r.status = 'unreplied' THEN 0 ELSE 1 END, r.timestamp ASC"
    )
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Count unreplied replies.
pub async fn count_unreplied(pool: &PgPool) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM replies WHERE status = 'unreplied'")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Mark a reply as replied, storing our reply ID.
pub async fn mark_replied(pool: &PgPool, id: &str, our_reply_id: &str) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE replies SET status = 'replied', replied_at = now(), our_reply_id = $2 WHERE id = $1"
    )
    .bind(id)
    .bind(our_reply_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Mark a reply as dismissed.
pub async fn mark_dismissed(pool: &PgPool, id: &str) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE replies SET status = 'dismissed' WHERE id = $1"
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Get a single reply by ID.
pub async fn get(pool: &PgPool, id: &str) -> Result<Option<Reply>, AppError> {
    let row = sqlx::query_as::<_, Reply>("SELECT * FROM replies WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Get post IDs from the last N days (for reply sync scope).
pub async fn recent_post_ids(pool: &PgPool, days: i32) -> Result<Vec<String>, AppError> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM posts WHERE timestamp >= now() - make_interval(days => $1) ORDER BY timestamp DESC"
    )
    .bind(days)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd postgraph-server && cargo check`

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/replies.rs postgraph-server/src/main.rs
git commit -m "feat(replies): add replies business logic module"
```

---

## Task 4: Reply Sync Function

**Files:**
- Modify: `postgraph-server/src/sync.rs`
- Modify: `postgraph-server/src/main.rs` (add to sync loop)

- [ ] **Step 1: Add sync_replies function to sync.rs**

Add at the end of `sync.rs`:

```rust
// ── Task 4: Reply Sync ─────────────────────────────────────────

/// Sync replies for posts from the last 7 days.
/// New replies are inserted with status 'unreplied'. Existing replies only update synced_at.
pub async fn sync_replies(
    pool: &PgPool,
    client: &ThreadsClient,
) -> Result<u32, AppError> {
    let post_ids = crate::replies::recent_post_ids(pool, 7).await?;
    info!("Syncing replies for {} recent posts", post_ids.len());

    let mut total_synced: u32 = 0;

    for post_id in &post_ids {
        let replies = match client.get_post_replies(post_id).await {
            Ok(r) => r,
            Err(AppError::RateLimited(_)) => {
                warn!("Rate limited during reply sync, aborting cycle");
                return Ok(total_synced);
            }
            Err(e) => {
                warn!("Failed to fetch replies for {post_id}: {e}");
                continue;
            }
        };

        for reply in &replies {
            let ts = reply.timestamp.as_deref().and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(s)
                    .ok()
                    .or_else(|| chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z").ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))
            });

            crate::replies::upsert_reply(
                pool,
                &reply.id,
                post_id,
                reply.username.as_deref(),
                reply.text.as_deref(),
                ts,
            ).await?;
            total_synced += 1;
        }

        // Brief pause between posts to avoid rate limits
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    info!("Synced {total_synced} replies");
    Ok(total_synced)
}
```

- [ ] **Step 2: Add sync_replies to the 15-min sync loop in main.rs**

In `main.rs`, in the background sync loop, add after the `sync_post_metrics` call (after line ~142) and before `sync_daily_views`:

```rust
            // Task 4: Sync replies for recent posts
            if let Err(e) = sync::sync_replies(&bg_state.pool, &bg_state.threads).await {
                tracing::error!("Background reply sync failed: {e}");
            }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd postgraph-server && cargo check`

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/sync.rs postgraph-server/src/main.rs
git commit -m "feat(replies): add reply sync to 15-min background loop"
```

---

## Task 5: Replies API Routes

**Files:**
- Create: `postgraph-server/src/routes/replies.rs`
- Modify: `postgraph-server/src/routes/mod.rs`
- Modify: `postgraph-server/src/main.rs` (register routes)

- [ ] **Step 1: Register module**

Add `pub mod replies;` to `postgraph-server/src/routes/mod.rs`.

- [ ] **Step 2: Create routes/replies.rs**

```rust
use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::replies;
use crate::state::AppState;

#[derive(Serialize)]
pub struct RepliesError {
    pub error: String,
}

type RepliesResult<T> = Result<Json<T>, (axum::http::StatusCode, Json<RepliesError>)>;

fn err(status: axum::http::StatusCode, msg: impl ToString) -> (axum::http::StatusCode, Json<RepliesError>) {
    (status, Json(RepliesError { error: msg.to_string() }))
}

fn internal(e: impl ToString) -> (axum::http::StatusCode, Json<RepliesError>) {
    err(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e)
}

const MAX_REPLY_LENGTH: usize = 500;

#[derive(Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
}

#[derive(Serialize)]
pub struct CountResponse {
    pub count: i64,
}

#[derive(Deserialize)]
pub struct ReplyRequest {
    pub text: String,
}

#[derive(Serialize)]
pub struct ReplyResponse {
    pub our_reply_id: String,
}

#[derive(Serialize)]
pub struct DismissResponse {
    pub dismissed: bool,
}

pub async fn list_replies(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> RepliesResult<Vec<replies::ReplyWithContext>> {
    let status = query.status.as_deref().or(Some("unreplied"));
    let list = replies::list(&state.pool, status)
        .await
        .map_err(internal)?;
    Ok(Json(list))
}

pub async fn count_unreplied(
    State(state): State<AppState>,
) -> RepliesResult<CountResponse> {
    let count = replies::count_unreplied(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(CountResponse { count }))
}

pub async fn send_reply(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReplyRequest>,
) -> RepliesResult<ReplyResponse> {
    if body.text.trim().is_empty() {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "Reply text cannot be empty"));
    }
    if body.text.chars().count() > MAX_REPLY_LENGTH {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, format!("Reply exceeds {MAX_REPLY_LENGTH} character limit")));
    }

    // Verify the reply exists
    let target = replies::get(&state.pool, &id)
        .await
        .map_err(internal)?;
    if target.is_none() {
        return Err(err(axum::http::StatusCode::NOT_FOUND, "Reply not found"));
    }

    // Send the reply via Threads API (reply_to_id is the reply we're responding to)
    let our_reply_id = state.threads.create_reply(&id, &body.text)
        .await
        .map_err(|e| err(axum::http::StatusCode::BAD_GATEWAY, format!("Threads API error: {e}")))?;

    // Mark as replied
    replies::mark_replied(&state.pool, &id, &our_reply_id)
        .await
        .map_err(internal)?;

    Ok(Json(ReplyResponse { our_reply_id }))
}

pub async fn dismiss_reply(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> RepliesResult<DismissResponse> {
    let dismissed = replies::mark_dismissed(&state.pool, &id)
        .await
        .map_err(internal)?;
    if !dismissed {
        return Err(err(axum::http::StatusCode::NOT_FOUND, "Reply not found"));
    }
    Ok(Json(DismissResponse { dismissed }))
}
```

- [ ] **Step 3: Register routes in main.rs**

Add after the compose routes (after `.route("/api/compose/{id}/publish", ...)`) and before `.layer(middleware...)`:

```rust
        .route("/api/replies", get(routes::replies::list_replies))
        .route("/api/replies/count", get(routes::replies::count_unreplied))
        .route("/api/replies/{id}/reply", post(routes::replies::send_reply))
        .route("/api/replies/{id}/dismiss", post(routes::replies::dismiss_reply))
```

- [ ] **Step 4: Verify it compiles**

Run: `cd postgraph-server && cargo check`

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/routes/replies.rs postgraph-server/src/routes/mod.rs postgraph-server/src/main.rs
git commit -m "feat(replies): add replies API route handlers"
```

---

## Task 6: Frontend API Layer

**Files:**
- Modify: `web/src/lib/api.ts`

- [ ] **Step 1: Add types**

Add after the `PublishNowResponse` interface:

```typescript
export interface ReplyWithContext {
  id: string;
  parent_post_id: string;
  username: string | null;
  text: string | null;
  timestamp: string | null;
  status: 'unreplied' | 'replied' | 'dismissed';
  replied_at: string | null;
  our_reply_id: string | null;
  synced_at: string;
  parent_post_text: string | null;
}

export interface ReplyCountResponse {
  count: number;
}

export interface SendReplyResponse {
  our_reply_id: string;
}
```

- [ ] **Step 2: Add API methods**

Add inside the `api` object, after the `publishNow` method:

```typescript
  // Replies
  getReplies: (status?: string) => {
    const params = new URLSearchParams();
    if (status) params.set('status', status);
    const qs = params.toString();
    return fetchApi<ReplyWithContext[]>(`/api/replies${qs ? `?${qs}` : ''}`);
  },

  getReplyCount: () => fetchApi<ReplyCountResponse>('/api/replies/count'),

  sendReply: (id: string, text: string) =>
    fetch(`/api/replies/${id}/reply`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text }),
    }).then(async r => {
      if (!r.ok) {
        const data = await r.json().catch(() => ({ error: `Reply failed (${r.status})` }));
        throw new Error(data.error ?? `Reply failed (${r.status})`);
      }
      return r.json() as Promise<SendReplyResponse>;
    }),

  dismissReply: (id: string) =>
    fetch(`/api/replies/${id}/dismiss`, { method: 'POST' }).then(async r => {
      if (!r.ok) {
        const data = await r.json().catch(() => ({ error: `Dismiss failed (${r.status})` }));
        throw new Error(data.error ?? `Dismiss failed (${r.status})`);
      }
      return r.json() as Promise<{ dismissed: boolean }>;
    }),
```

- [ ] **Step 3: Verify frontend**

Run: `cd web && npx svelte-check`

- [ ] **Step 4: Commit**

```bash
git add web/src/lib/api.ts
git commit -m "feat(replies): add reply types and API methods to frontend"
```

---

## Task 7: SvelteKit Proxy Routes

**Files:**
- Create: `web/src/routes/api/replies/+server.ts`
- Create: `web/src/routes/api/replies/count/+server.ts`
- Create: `web/src/routes/api/replies/[id]/reply/+server.ts`
- Create: `web/src/routes/api/replies/[id]/dismiss/+server.ts`

- [ ] **Step 1: Create proxy route for GET /api/replies**

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ url }) => {
  const searchParams = url.searchParams;
  return proxyToBackend('/api/replies', { searchParams });
};
```

- [ ] **Step 2: Create proxy route for GET /api/replies/count**

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  return proxyToBackend('/api/replies/count');
};
```

- [ ] **Step 3: Create proxy route for POST /api/replies/[id]/reply**

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async ({ params, request }) => {
  const body = await request.text();
  return proxyToBackend(`/api/replies/${params.id}/reply`, { method: 'POST', body });
};
```

- [ ] **Step 4: Create proxy route for POST /api/replies/[id]/dismiss**

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async ({ params }) => {
  return proxyToBackend(`/api/replies/${params.id}/dismiss`, { method: 'POST' });
};
```

- [ ] **Step 5: Commit**

```bash
git add web/src/routes/api/replies/
git commit -m "feat(replies): add SvelteKit proxy routes for replies API"
```

---

## Task 8: Frontend Route, Nav Link & Badge

**Files:**
- Create: `web/src/routes/replies/+page.svelte`
- Modify: `web/src/routes/+layout.svelte`

- [ ] **Step 1: Create route file**

```svelte
<script lang="ts">
  import Replies from '$lib/components/Replies.svelte';
</script>

<Replies />
```

- [ ] **Step 2: Add nav link with count badge**

In `web/src/routes/+layout.svelte`, replace the script block and add the Replies link:

```svelte
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
```

Add the Replies link in the nav-links div, after Compose and before Fourier:

```svelte
      <a href="/replies" class:active={$page.url.pathname === '/replies'}>
        Replies{#if unrepliedCount > 0} ({unrepliedCount}){/if}
      </a>
```

So the full nav-links section becomes:
```svelte
    <div class="nav-links">
      <a href="/" class:active={$page.url.pathname === '/'}>Graph</a>
      <a href="/analytics" class:active={$page.url.pathname === '/analytics'}>Analytics</a>
      <a href="/analytics-v2" class:active={$page.url.pathname === '/analytics-v2'}>V2</a>
      <a href="/insights" class:active={$page.url.pathname === '/insights'}>Insights</a>
      <a href="/compose" class:active={$page.url.pathname === '/compose'}>Compose</a>
      <a href="/replies" class:active={$page.url.pathname === '/replies'}>
        Replies{#if unrepliedCount > 0} ({unrepliedCount}){/if}
      </a>
      <a href="/fourier" class:active={$page.url.pathname === '/fourier'}>ƒ(t)</a>
      <a href="/debug" class:active={$page.url.pathname === '/debug'}>Debug</a>
      <a href="/health" class:active={$page.url.pathname === '/health'}>Health</a>
    </div>
```

- [ ] **Step 3: Commit**

```bash
git add web/src/routes/replies/+page.svelte web/src/routes/+layout.svelte
git commit -m "feat(replies): add /replies route and nav link with count badge"
```

---

## Task 9: Replies Inbox Component

**Files:**
- Create: `web/src/lib/components/Replies.svelte`

- [ ] **Step 1: Create Replies.svelte**

```svelte
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
```

- [ ] **Step 2: Verify frontend builds**

Run: `cd web && npx svelte-check && npm run build`

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/components/Replies.svelte
git commit -m "feat(replies): add Replies inbox component"
```

---

## Task 10: Backend & Frontend Verification

**Files:** None (verification only)

- [ ] **Step 1: Full backend check**

Run: `cd postgraph-server && cargo clippy --workspace --all-targets`

Fix any warnings or errors.

- [ ] **Step 2: Full frontend check**

Run: `cd web && npx svelte-check`

- [ ] **Step 3: Build frontend**

Run: `cd web && npm run build`

Expected: Builds successfully.

- [ ] **Step 4: Commit any fixes**

If any fixes were needed:
```bash
git add -A
git commit -m "fix(replies): address clippy and svelte-check issues"
```
