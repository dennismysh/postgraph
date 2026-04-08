# Compose Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Compose tab with a content calendar and scheduled publishing to the postgraph dashboard.

**Architecture:** New `scheduled_posts` table separate from synced `posts`. Backend adds Threads publish API methods, CRUD routes, and a 1-minute scheduler loop. Frontend adds a calendar page with weekly/2-week/monthly views and a compose modal.

**Tech Stack:** Rust (axum, sqlx, tokio, uuid, chrono), Svelte 5, Threads API (two-step publish)

**Spec:** `docs/superpowers/specs/2026-04-07-compose-tab-design.md`

---

## File Structure

### Backend (postgraph-server/src/)
- **Create:** `compose.rs` — Business logic: CRUD for scheduled_posts, publish orchestration
- **Create:** `routes/compose.rs` — HTTP handlers for compose API endpoints
- **Create:** `migrations/015_scheduled_posts.sql` — Database migration
- **Modify:** `threads.rs` — Add `create_container()` and `publish_container()` methods
- **Modify:** `routes/mod.rs` — Register `compose` module
- **Modify:** `main.rs` — Register routes, add scheduler loop, add startup recovery

### Frontend (web/src/)
- **Create:** `routes/compose/+page.svelte` — Route wrapper
- **Create:** `lib/components/Compose.svelte` — Calendar page component
- **Create:** `lib/components/ComposeModal.svelte` — Compose/edit modal
- **Modify:** `lib/api.ts` — Add compose types and API methods
- **Modify:** `routes/+layout.svelte` — Add Compose nav link

---

## Task 1: Database Migration

**Files:**
- Create: `postgraph-server/migrations/015_scheduled_posts.sql`

- [ ] **Step 1: Create migration file**

```sql
CREATE TABLE scheduled_posts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    text TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    scheduled_at TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    threads_post_id TEXT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_scheduled_posts_status_scheduled_at
    ON scheduled_posts (status, scheduled_at)
    WHERE status = 'scheduled';
```

- [ ] **Step 2: Verify migration runs**

Run: `cd postgraph-server && cargo check`

Expected: Compiles successfully (migrations are checked at compile time via `sqlx::migrate!()`)

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/migrations/015_scheduled_posts.sql
git commit -m "feat(compose): add scheduled_posts migration"
```

---

## Task 2: Compose Business Logic Module

**Files:**
- Create: `postgraph-server/src/compose.rs`
- Modify: `postgraph-server/src/main.rs` (add `mod compose;`)

- [ ] **Step 1: Add module declaration**

In `postgraph-server/src/main.rs`, add `mod compose;` to the module list (after `mod auth;`).

- [ ] **Step 2: Create compose.rs with types and CRUD functions**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ScheduledPost {
    pub id: Uuid,
    pub text: String,
    pub status: String,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub published_at: Option<DateTime<Utc>>,
    pub threads_post_id: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn create(pool: &PgPool, text: &str, status: &str, scheduled_at: Option<DateTime<Utc>>) -> Result<ScheduledPost, AppError> {
    let row = sqlx::query_as::<_, ScheduledPost>(
        "INSERT INTO scheduled_posts (text, status, scheduled_at) VALUES ($1, $2, $3) RETURNING *"
    )
    .bind(text)
    .bind(status)
    .bind(scheduled_at)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn get(pool: &PgPool, id: Uuid) -> Result<Option<ScheduledPost>, AppError> {
    let row = sqlx::query_as::<_, ScheduledPost>(
        "SELECT * FROM scheduled_posts WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list(pool: &PgPool, status: Option<&str>, from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> Result<Vec<ScheduledPost>, AppError> {
    let rows = sqlx::query_as::<_, ScheduledPost>(
        "SELECT * FROM scheduled_posts
         WHERE ($1::text IS NULL OR status = $1)
           AND ($2::timestamptz IS NULL OR scheduled_at >= $2)
           AND ($3::timestamptz IS NULL OR scheduled_at < $3)
         ORDER BY COALESCE(scheduled_at, created_at) ASC"
    )
    .bind(status)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn update(pool: &PgPool, id: Uuid, text: Option<&str>, status: Option<&str>, scheduled_at: Option<Option<DateTime<Utc>>>) -> Result<Option<ScheduledPost>, AppError> {
    // Build update dynamically — only touch columns that were provided
    let row = sqlx::query_as::<_, ScheduledPost>(
        "UPDATE scheduled_posts SET
            text = COALESCE($2, text),
            status = COALESCE($3, status),
            scheduled_at = CASE WHEN $4 THEN $5 ELSE scheduled_at END,
            updated_at = now()
         WHERE id = $1
         RETURNING *"
    )
    .bind(id)
    .bind(text)
    .bind(status)
    .bind(scheduled_at.is_some()) // $4: whether to update scheduled_at
    .bind(scheduled_at.flatten())  // $5: the new value (may be NULL for drafts)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM scheduled_posts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Set status to 'publishing' and return posts ready to publish.
/// Only grabs posts where status='scheduled' and scheduled_at <= now.
pub async fn claim_due_posts(pool: &PgPool) -> Result<Vec<ScheduledPost>, AppError> {
    let rows = sqlx::query_as::<_, ScheduledPost>(
        "UPDATE scheduled_posts
         SET status = 'publishing', updated_at = now()
         WHERE status = 'scheduled' AND scheduled_at <= now()
         RETURNING *"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Mark a post as published with its Threads post ID.
pub async fn mark_published(pool: &PgPool, id: Uuid, threads_post_id: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE scheduled_posts SET status = 'published', threads_post_id = $2, published_at = now(), updated_at = now() WHERE id = $1"
    )
    .bind(id)
    .bind(threads_post_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark a post as failed with an error message.
pub async fn mark_failed(pool: &PgPool, id: Uuid, error: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE scheduled_posts SET status = 'failed', error_message = $2, updated_at = now() WHERE id = $1"
    )
    .bind(id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Recover posts stuck in 'publishing' for more than 5 minutes (crash recovery).
pub async fn recover_stuck(pool: &PgPool) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE scheduled_posts SET status = 'scheduled', updated_at = now()
         WHERE status = 'publishing' AND updated_at < now() - interval '5 minutes'"
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd postgraph-server && cargo check`

Expected: Compiles (note: the migration must exist locally for sqlx to see the table — if using offline mode, you may need `cargo sqlx prepare` later)

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/compose.rs postgraph-server/src/main.rs
git commit -m "feat(compose): add compose business logic module with CRUD and scheduler helpers"
```

---

## Task 3: Threads Publish API Methods

**Files:**
- Modify: `postgraph-server/src/threads.rs`

- [ ] **Step 1: Add publish response types**

Add these structs after the existing `InsightsResponse` struct at the top of `threads.rs`:

```rust
#[derive(Debug, Deserialize)]
pub struct CreateContainerResponse {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct PublishResponse {
    pub id: String,
}
```

- [ ] **Step 2: Add create_container method to ThreadsClient**

Add inside the `impl ThreadsClient` block, after the `get_user_insights` method:

```rust
    /// Step 1 of Threads publish: create a media container.
    /// Returns the container ID.
    pub async fn create_container(&self, text: &str) -> Result<String, AppError> {
        let url = format!(
            "{}/me/threads?media_type=TEXT&text={}&access_token={}",
            BASE_URL,
            urlencoding::encode(text),
            self.token().await
        );

        let resp = self.client.post(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::ThreadsApi(format!(
                "Create container failed: {body}"
            )));
        }
        let data: CreateContainerResponse = resp.json().await?;
        Ok(data.id)
    }

    /// Step 2 of Threads publish: publish a container.
    /// Returns the published post ID.
    pub async fn publish_container(&self, container_id: &str) -> Result<String, AppError> {
        let url = format!(
            "{}/me/threads_publish?creation_id={}&access_token={}",
            BASE_URL,
            container_id,
            self.token().await
        );

        let resp = self.client.post(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::ThreadsApi(format!(
                "Publish failed: {body}"
            )));
        }
        let data: PublishResponse = resp.json().await?;
        Ok(data.id)
    }
```

- [ ] **Step 3: Add urlencoding dependency**

The `text` parameter must be URL-encoded for the Threads API. Add to `postgraph-server/Cargo.toml` under `[dependencies]`:

```toml
urlencoding = "2"
```

- [ ] **Step 4: Verify it compiles**

Run: `cd postgraph-server && cargo check`

Expected: Compiles successfully.

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/threads.rs postgraph-server/Cargo.toml
git commit -m "feat(compose): add Threads publish API methods (create_container, publish_container)"
```

---

## Task 4: Compose API Routes

**Files:**
- Create: `postgraph-server/src/routes/compose.rs`
- Modify: `postgraph-server/src/routes/mod.rs`

- [ ] **Step 1: Register module**

Add `pub mod compose;` to `postgraph-server/src/routes/mod.rs`.

- [ ] **Step 2: Create routes/compose.rs**

```rust
use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::compose;
use crate::state::AppState;

#[derive(Serialize)]
pub struct ComposeError {
    pub error: String,
}

type ComposeResult<T> = Result<Json<T>, (axum::http::StatusCode, Json<ComposeError>)>;

fn err(status: axum::http::StatusCode, msg: impl ToString) -> (axum::http::StatusCode, Json<ComposeError>) {
    (status, Json(ComposeError { error: msg.to_string() }))
}

fn internal(e: impl ToString) -> (axum::http::StatusCode, Json<ComposeError>) {
    err(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e)
}

const MAX_TEXT_LENGTH: usize = 500;

// -- Request/Response types --

#[derive(Deserialize)]
pub struct CreateRequest {
    pub text: String,
    pub status: Option<String>, // "draft" or "scheduled", defaults to "draft"
    pub scheduled_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub struct UpdateRequest {
    pub text: Option<String>,
    pub status: Option<String>,
    pub scheduled_at: Option<Option<DateTime<Utc>>>, // Some(None) clears it, None leaves unchanged
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}

#[derive(Serialize)]
pub struct PublishNowResponse {
    pub threads_post_id: String,
}

// -- Handlers --

pub async fn list_posts(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> ComposeResult<Vec<compose::ScheduledPost>> {
    let posts = compose::list(&state.pool, query.status.as_deref(), query.from, query.to)
        .await
        .map_err(internal)?;
    Ok(Json(posts))
}

pub async fn create_post(
    State(state): State<AppState>,
    Json(body): Json<CreateRequest>,
) -> ComposeResult<compose::ScheduledPost> {
    if body.text.is_empty() {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "Text cannot be empty"));
    }
    if body.text.len() > MAX_TEXT_LENGTH {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, format!("Text exceeds {MAX_TEXT_LENGTH} character limit")));
    }

    let status = body.status.as_deref().unwrap_or("draft");
    if status != "draft" && status != "scheduled" {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "Status must be 'draft' or 'scheduled'"));
    }
    if status == "scheduled" && body.scheduled_at.is_none() {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "scheduled_at is required when status is 'scheduled'"));
    }

    let post = compose::create(&state.pool, &body.text, status, body.scheduled_at)
        .await
        .map_err(internal)?;
    Ok(Json(post))
}

pub async fn get_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ComposeResult<compose::ScheduledPost> {
    let post = compose::get(&state.pool, id)
        .await
        .map_err(internal)?;
    match post {
        Some(p) => Ok(Json(p)),
        None => Err(err(axum::http::StatusCode::NOT_FOUND, "Post not found")),
    }
}

pub async fn update_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRequest>,
) -> ComposeResult<compose::ScheduledPost> {
    if let Some(ref text) = body.text {
        if text.is_empty() {
            return Err(err(axum::http::StatusCode::BAD_REQUEST, "Text cannot be empty"));
        }
        if text.len() > MAX_TEXT_LENGTH {
            return Err(err(axum::http::StatusCode::BAD_REQUEST, format!("Text exceeds {MAX_TEXT_LENGTH} character limit")));
        }
    }
    if let Some(ref status) = body.status {
        if !["draft", "scheduled", "cancelled"].contains(&status.as_str()) {
            return Err(err(axum::http::StatusCode::BAD_REQUEST, "Invalid status"));
        }
    }

    let post = compose::update(&state.pool, id, body.text.as_deref(), body.status.as_deref(), body.scheduled_at)
        .await
        .map_err(internal)?;
    match post {
        Some(p) => Ok(Json(p)),
        None => Err(err(axum::http::StatusCode::NOT_FOUND, "Post not found")),
    }
}

pub async fn delete_post(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ComposeResult<DeleteResponse> {
    let deleted = compose::delete(&state.pool, id)
        .await
        .map_err(internal)?;
    Ok(Json(DeleteResponse { deleted }))
}

pub async fn publish_now(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ComposeResult<PublishNowResponse> {
    // Load the post
    let post = compose::get(&state.pool, id)
        .await
        .map_err(internal)?;
    let post = match post {
        Some(p) => p,
        None => return Err(err(axum::http::StatusCode::NOT_FOUND, "Post not found")),
    };
    if post.status == "published" {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "Post is already published"));
    }
    if post.status == "publishing" {
        return Err(err(axum::http::StatusCode::BAD_REQUEST, "Post is currently being published"));
    }

    // Mark as publishing
    compose::update(&state.pool, id, None, Some("publishing"), None)
        .await
        .map_err(internal)?;

    // Two-step Threads publish
    let container_id = match state.threads.create_container(&post.text).await {
        Ok(cid) => cid,
        Err(e) => {
            let _ = compose::mark_failed(&state.pool, id, &e.to_string()).await;
            return Err(err(axum::http::StatusCode::BAD_GATEWAY, format!("Threads API error: {e}")));
        }
    };

    let threads_post_id = match state.threads.publish_container(&container_id).await {
        Ok(pid) => pid,
        Err(e) => {
            let _ = compose::mark_failed(&state.pool, id, &e.to_string()).await;
            return Err(err(axum::http::StatusCode::BAD_GATEWAY, format!("Threads API error: {e}")));
        }
    };

    compose::mark_published(&state.pool, id, &threads_post_id)
        .await
        .map_err(internal)?;

    Ok(Json(PublishNowResponse { threads_post_id }))
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd postgraph-server && cargo check`

Expected: Compiles (routes are defined but not yet registered in the router — that's fine).

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/routes/compose.rs postgraph-server/src/routes/mod.rs
git commit -m "feat(compose): add compose API route handlers"
```

---

## Task 5: Register Routes & Add Scheduler Loop

**Files:**
- Modify: `postgraph-server/src/main.rs`

- [ ] **Step 1: Register compose routes**

In `main.rs`, add these routes inside the `api_routes` Router builder, after the emotions routes and before the `.layer(middleware...)` call:

```rust
        .route("/api/compose", get(routes::compose::list_posts))
        .route("/api/compose", post(routes::compose::create_post))
        .route("/api/compose/{id}", get(routes::compose::get_post))
        .route("/api/compose/{id}", axum::routing::put(routes::compose::update_post))
        .route("/api/compose/{id}", axum::routing::delete(routes::compose::delete_post))
        .route("/api/compose/{id}/publish", post(routes::compose::publish_now))
```

Also add `use axum::routing::put;` and `use axum::routing::delete;` if not already imported, or use inline `axum::routing::put` / `axum::routing::delete` as shown.

Note: Axum 0.8 requires separate `.route()` calls for each method on the same path. If the router complains about duplicate paths, combine with `.route("/api/compose", get(...).post(...))` and `.route("/api/compose/{id}", get(...).put(...).delete(...))`.

- [ ] **Step 2: Add scheduler loop**

Add a new `tokio::spawn` block after the nightly sync spawn (around line 257), before the CORS/router setup:

```rust
    // Spawn publish scheduler (checks every 60s for posts due to publish)
    let sched_state = state.clone();
    tokio::spawn(async move {
        // Startup recovery: reset stuck 'publishing' posts
        match compose::recover_stuck(&sched_state.pool).await {
            Ok(0) => {}
            Ok(n) => info!("Recovered {n} stuck publishing posts"),
            Err(e) => tracing::error!("Failed to recover stuck posts: {e}"),
        }

        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;

            let due = match compose::claim_due_posts(&sched_state.pool).await {
                Ok(posts) => posts,
                Err(e) => {
                    tracing::error!("Scheduler: failed to claim due posts: {e}");
                    continue;
                }
            };

            for post in due {
                info!("Publishing scheduled post {}", post.id);

                // Re-check status (race condition guard)
                let current = compose::get(&sched_state.pool, post.id).await;
                if let Ok(Some(p)) = &current {
                    if p.status != "publishing" {
                        info!("Post {} status changed to '{}', skipping", post.id, p.status);
                        continue;
                    }
                }

                let result = async {
                    let container_id = sched_state.threads.create_container(&post.text).await?;
                    sched_state.threads.publish_container(&container_id).await
                }.await;

                match result {
                    Ok(threads_post_id) => {
                        info!("Published post {} as {threads_post_id}", post.id);
                        if let Err(e) = compose::mark_published(&sched_state.pool, post.id, &threads_post_id).await {
                            tracing::error!("Failed to mark post {} as published: {e}", post.id);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to publish post {}: {e}", post.id);
                        if let Err(e2) = compose::mark_failed(&sched_state.pool, post.id, &e.to_string()).await {
                            tracing::error!("Failed to mark post {} as failed: {e2}", post.id);
                        }
                    }
                }
            }
        }
    });
```

- [ ] **Step 3: Verify it compiles**

Run: `cd postgraph-server && cargo check`

Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/main.rs
git commit -m "feat(compose): register compose routes and add publish scheduler loop"
```

---

## Task 6: Frontend API Layer

**Files:**
- Modify: `web/src/lib/api.ts`

- [ ] **Step 1: Add types**

Add these interfaces after the existing `EmotionNarrativeResponse` interface (around line 271):

```typescript
export interface ScheduledPost {
  id: string;
  text: string;
  status: 'draft' | 'scheduled' | 'publishing' | 'published' | 'failed' | 'cancelled';
  scheduled_at: string | null;
  published_at: string | null;
  threads_post_id: string | null;
  error_message: string | null;
  created_at: string;
  updated_at: string;
}

export interface PublishNowResponse {
  threads_post_id: string;
}
```

- [ ] **Step 2: Add API methods**

Add these methods inside the `api` object, after the `backfillEmotions` method:

```typescript
  // Compose
  getScheduledPosts: (params?: { status?: string; from?: string; to?: string }) => {
    const searchParams = new URLSearchParams();
    if (params?.status) searchParams.set('status', params.status);
    if (params?.from) searchParams.set('from', params.from);
    if (params?.to) searchParams.set('to', params.to);
    const qs = searchParams.toString();
    return fetchApi<ScheduledPost[]>(`/api/compose${qs ? `?${qs}` : ''}`);
  },

  getScheduledPost: (id: string) => fetchApi<ScheduledPost>(`/api/compose/${id}`),

  createScheduledPost: (body: { text: string; status?: string; scheduled_at?: string }) =>
    fetch('/api/compose', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }).then(async r => {
      if (!r.ok) {
        const data = await r.json().catch(() => ({ error: `Create failed (${r.status})` }));
        throw new Error(data.error ?? `Create failed (${r.status})`);
      }
      return r.json() as Promise<ScheduledPost>;
    }),

  updateScheduledPost: (id: string, body: { text?: string; status?: string; scheduled_at?: string | null }) =>
    fetch(`/api/compose/${id}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }).then(async r => {
      if (!r.ok) {
        const data = await r.json().catch(() => ({ error: `Update failed (${r.status})` }));
        throw new Error(data.error ?? `Update failed (${r.status})`);
      }
      return r.json() as Promise<ScheduledPost>;
    }),

  deleteScheduledPost: (id: string) =>
    fetch(`/api/compose/${id}`, { method: 'DELETE' }).then(async r => {
      if (!r.ok) {
        const data = await r.json().catch(() => ({ error: `Delete failed (${r.status})` }));
        throw new Error(data.error ?? `Delete failed (${r.status})`);
      }
      return r.json() as Promise<{ deleted: boolean }>;
    }),

  publishNow: (id: string) =>
    fetch(`/api/compose/${id}/publish`, { method: 'POST' }).then(async r => {
      if (!r.ok) {
        const data = await r.json().catch(() => ({ error: `Publish failed (${r.status})` }));
        throw new Error(data.error ?? `Publish failed (${r.status})`);
      }
      return r.json() as Promise<PublishNowResponse>;
    }),
```

- [ ] **Step 3: Verify frontend builds**

Run: `cd web && npx svelte-check`

Expected: No type errors.

- [ ] **Step 4: Commit**

```bash
git add web/src/lib/api.ts
git commit -m "feat(compose): add compose types and API methods to frontend"
```

---

## Task 7: Frontend Route & Nav Link

**Files:**
- Create: `web/src/routes/compose/+page.svelte`
- Modify: `web/src/routes/+layout.svelte`

- [ ] **Step 1: Create route file**

```svelte
<script lang="ts">
  import Compose from '$lib/components/Compose.svelte';
</script>

<Compose />
```

- [ ] **Step 2: Add nav link**

In `web/src/routes/+layout.svelte`, add the Compose link in the `nav-links` div, after the Insights link and before the Fourier link:

```svelte
      <a href="/compose" class:active={$page.url.pathname === '/compose'}>Compose</a>
```

So the nav-links section becomes:
```svelte
    <div class="nav-links">
      <a href="/" class:active={$page.url.pathname === '/'}>Graph</a>
      <a href="/analytics" class:active={$page.url.pathname === '/analytics'}>Analytics</a>
      <a href="/analytics-v2" class:active={$page.url.pathname === '/analytics-v2'}>V2</a>
      <a href="/insights" class:active={$page.url.pathname === '/insights'}>Insights</a>
      <a href="/compose" class:active={$page.url.pathname === '/compose'}>Compose</a>
      <a href="/fourier" class:active={$page.url.pathname === '/fourier'}>ƒ(t)</a>
      <a href="/debug" class:active={$page.url.pathname === '/debug'}>Debug</a>
      <a href="/health" class:active={$page.url.pathname === '/health'}>Health</a>
    </div>
```

- [ ] **Step 3: Commit**

```bash
git add web/src/routes/compose/+page.svelte web/src/routes/+layout.svelte
git commit -m "feat(compose): add /compose route and nav link"
```

---

## Task 8: Compose Modal Component

**Files:**
- Create: `web/src/lib/components/ComposeModal.svelte`

Build the modal first since the calendar depends on it.

- [ ] **Step 1: Create ComposeModal.svelte**

```svelte
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
    return d.toISOString().slice(0, 10);
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
        // Save text first, then publish
        await api.updateScheduledPost(post!.id, { text });
        await api.publishNow(post!.id);
      } else if (action === 'publish' && !isEditing) {
        // Create then publish
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
    border: 1px solid #333;
    border-radius: 8px;
    padding: 1.5rem;
    width: 90%;
    max-width: 520px;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .header h3 { margin: 0; }
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
    font-size: 0.85rem;
  }
  textarea {
    background: #111;
    border: 1px solid #333;
    color: #eee;
    padding: 0.75rem;
    border-radius: 4px;
    resize: vertical;
    font-family: inherit;
    font-size: 0.95rem;
  }
  textarea:focus { outline: none; border-color: #555; }
  .char-count { text-align: right; font-size: 0.8rem; color: #666; }
  .char-count.over { color: #e6194b; }
  .schedule-row {
    display: flex;
    gap: 1rem;
  }
  .schedule-row label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.85rem;
    color: #888;
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
    gap: 0.5rem;
    margin-top: 0.5rem;
  }
  .primary-actions {
    display: flex;
    gap: 0.5rem;
    margin-left: auto;
  }
  .btn {
    padding: 0.5rem 1rem;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.85rem;
  }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn.draft { background: #333; color: #ccc; }
  .btn.schedule { background: #1a3a5c; color: #6cb4ee; }
  .btn.publish { background: #1a4a2e; color: #6be67a; }
  .btn.cancel { background: #3a2a15; color: #e6a64b; }
  .btn.delete { background: #3a1515; color: #e6194b; }
  .btn.retry { background: #1a3a5c; color: #6cb4ee; }
</style>
```

- [ ] **Step 2: Verify it compiles**

Run: `cd web && npx svelte-check`

Expected: No errors (component exists but isn't imported anywhere yet — that's fine).

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/components/ComposeModal.svelte
git commit -m "feat(compose): add ComposeModal component"
```

---

## Task 9: Calendar Component (Compose Page)

**Files:**
- Create: `web/src/lib/components/Compose.svelte`

- [ ] **Step 1: Create Compose.svelte**

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type ScheduledPost } from '$lib/api';
  import ComposeModal from './ComposeModal.svelte';

  type ViewMode = 'week' | '2week' | 'month';

  let posts: ScheduledPost[] = $state([]);
  let loading = $state(true);
  let viewMode: ViewMode = $state('2week');
  let currentDate = $state(new Date()); // anchor date for navigation

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
      // Start from Monday of the current week
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
      // Month view: start from the first day of the month, include leading days to fill the week
      start.setDate(1);
      const firstDay = start.getDay();
      const leadingDays = firstDay === 0 ? 6 : firstDay - 1;
      start.setDate(start.getDate() - leadingDays);
      // Always show 5 weeks (35 days) to fill the grid
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
      // Fetch all non-cancelled posts for the visible date range
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

  // Reload posts when calendar range changes
  $effect(() => {
    // Touch calendarDays to create dependency
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
          <button
            class="day-cell"
            class:today={isToday(day)}
            class:other-month={viewMode === 'month' && !isCurrentMonth(day)}
            onclick={() => openNewPost(day)}
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
          </button>
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
```

- [ ] **Step 2: Verify frontend builds**

Run: `cd web && npx svelte-check`

Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/components/Compose.svelte
git commit -m "feat(compose): add calendar component with week/2-week/month views"
```

---

## Task 10: Backend Compile Check & End-to-End Verification

**Files:** None (verification only)

- [ ] **Step 1: Full backend check**

Run: `cd postgraph-server && cargo clippy --workspace --all-targets`

Fix any warnings or errors.

- [ ] **Step 2: Full frontend check**

Run: `cd web && npx svelte-check`

Fix any type errors.

- [ ] **Step 3: Build frontend**

Run: `cd web && npm run build`

Expected: Builds successfully.

- [ ] **Step 4: Commit any fixes**

If any fixes were needed:
```bash
git add -A
git commit -m "fix(compose): address clippy and svelte-check issues"
```
