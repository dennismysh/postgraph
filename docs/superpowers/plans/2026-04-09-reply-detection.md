# Reply Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto-detect replies made outside postgraph by checking conversation threads for the owner's username, and mark them as `replied`.

**Architecture:** Add `get_me()` and `get_conversation()` to `ThreadsClient`, store `owner_username` in `AppState`, add `detect_external_replies()` to sync logic, expose via new endpoint, and add a debug page button.

**Tech Stack:** Rust (axum, sqlx, reqwest), Svelte 5, Threads API `/me` and `/{id}/conversation` endpoints.

---

### File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `postgraph-server/src/threads.rs` | Modify | Add `get_me()` and `get_conversation()` methods |
| `postgraph-server/src/state.rs` | Modify | Add `owner_username: String` to `AppState` |
| `postgraph-server/src/main.rs` | Modify | Fetch username at startup, pass to state, add route, wire into sync |
| `postgraph-server/src/sync.rs` | Modify | Add `detect_external_replies()` function, call after `sync_replies` |
| `postgraph-server/src/replies.rs` | Modify | Add `unreplied_grouped_by_parent()` and `mark_replied_external()` queries |
| `postgraph-server/src/routes/replies.rs` | Modify | Add `detect_replies` handler |
| `postgraph-server/src/routes/mod.rs` | No change | `replies` module already exported |
| `web/src/lib/api.ts` | Modify | Add `detectReplies()` method |
| `web/src/routes/api/replies/detect/+server.ts` | Create | SvelteKit proxy for POST /api/replies/detect |
| `web/src/lib/components/Debug.svelte` | Modify | Add "Detect Replies" button |

---

### Task 1: Add `get_me()` to ThreadsClient

**Files:**
- Modify: `postgraph-server/src/threads.rs:22-25` (struct), `:156-170` (health_check area)

- [ ] **Step 1: Add the `get_me` method and response struct**

In `threads.rs`, add a new response struct after `PublishResponse` (after line 87):

```rust
#[derive(Debug, Deserialize)]
pub struct MeResponse {
    pub id: String,
    pub username: Option<String>,
}
```

Add the `get_me` method to the `impl ThreadsClient` block, after `health_check` (after line 170):

```rust
/// Fetch the authenticated user's profile (id + username).
pub async fn get_me(&self) -> Result<MeResponse, AppError> {
    let url = format!(
        "{}/me?fields=id,username&access_token={}",
        BASE_URL,
        self.token().await
    );
    let resp = self.client.get(&url).send().await?;
    if resp.status() == 429 {
        return Err(AppError::RateLimited(60));
    }
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::ThreadsApi(format!("Get /me failed: {body}")));
    }
    let data: MeResponse = resp.json().await?;
    Ok(data)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/threads.rs
git commit -m "feat: add get_me() to ThreadsClient for username resolution"
```

---

### Task 2: Add `get_conversation()` to ThreadsClient

**Files:**
- Modify: `postgraph-server/src/threads.rs`

- [ ] **Step 1: Add the `get_conversation` method**

Add after the `get_me` method. This follows the same pagination pattern as `get_post_replies`:

```rust
/// Fetch the full conversation thread for a post.
/// Returns all replies in the conversation tree (including nested replies).
pub async fn get_conversation(&self, post_id: &str) -> Result<Vec<ThreadsReply>, AppError> {
    let mut all_replies = Vec::new();
    let mut url = format!(
        "{}/{}/conversation?fields=id,text,username,timestamp&access_token={}",
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
                "Get conversation failed for {post_id}: {body}"
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
                    "{}/{}/conversation?fields=id,text,username,timestamp&after={}&access_token={}",
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
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/threads.rs
git commit -m "feat: add get_conversation() to ThreadsClient"
```

---

### Task 3: Add `owner_username` to AppState and resolve at startup

**Files:**
- Modify: `postgraph-server/src/state.rs:9-21`
- Modify: `postgraph-server/src/main.rs:79-91`

- [ ] **Step 1: Add `owner_username` to AppState**

In `state.rs`, add the field to the `AppState` struct:

```rust
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub threads: Arc<ThreadsClient>,
    pub mercury: Arc<MercuryClient>,
    pub api_key: String,
    pub owner_username: String,
    pub analysis_running: Arc<AtomicBool>,
    pub analysis_progress: Arc<AtomicU32>,
    pub analysis_total: Arc<AtomicU32>,
    pub sync_running: Arc<AtomicBool>,
    pub sync_message: Arc<RwLock<String>>,
    pub sync_progress: Arc<AtomicU32>,
    pub sync_total: Arc<AtomicU32>,
}
```

- [ ] **Step 2: Fetch username at startup in main.rs**

In `main.rs`, after creating the `ThreadsClient` (line 81) but before building `AppState` (line 79), add the username resolution. Insert after line 81 (`Arc::new(ThreadsClient::new(effective_token))`):

```rust
let threads = Arc::new(ThreadsClient::new(effective_token));

// Resolve owner username from Threads API
let owner_username = match threads.get_me().await {
    Ok(me) => {
        let username = me.username.unwrap_or_else(|| {
            tracing::warn!("Threads /me returned no username, using empty string");
            String::new()
        });
        info!("Threads owner: @{username}");
        username
    }
    Err(e) => {
        tracing::error!("Failed to fetch Threads username: {e}");
        String::new()
    }
};
```

Then update the `AppState` construction to use the new `threads` variable and add `owner_username`:

```rust
let state = AppState {
    pool: pool.clone(),
    threads,
    mercury: Arc::new(MercuryClient::new(mercury_key, mercury_url)),
    api_key,
    owner_username,
    analysis_running: Arc::new(AtomicBool::new(false)),
    // ... rest unchanged
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/state.rs postgraph-server/src/main.rs
git commit -m "feat: resolve Threads owner username at startup"
```

---

### Task 4: Add DB queries for detection

**Files:**
- Modify: `postgraph-server/src/replies.rs`

- [ ] **Step 1: Add `unreplied_grouped_by_parent()` query**

Add at the end of `replies.rs`:

```rust
/// Get all unreplied reply IDs grouped by parent_post_id.
/// Returns (parent_post_id, Vec<(reply_id, reply_timestamp)>).
pub async fn unreplied_grouped_by_parent(
    pool: &PgPool,
) -> Result<std::collections::HashMap<String, Vec<(String, Option<DateTime<Utc>>)>>, AppError> {
    let rows: Vec<(String, String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT parent_post_id, id, timestamp FROM replies WHERE status = 'unreplied' ORDER BY parent_post_id"
    )
    .fetch_all(pool)
    .await?;

    let mut map: std::collections::HashMap<String, Vec<(String, Option<DateTime<Utc>>)>> =
        std::collections::HashMap::new();
    for (parent, id, ts) in rows {
        map.entry(parent).or_default().push((id, ts));
    }
    Ok(map)
}
```

- [ ] **Step 2: Add `mark_replied_external()` function**

Add after the previous function:

```rust
/// Mark a reply as replied (detected externally — no our_reply_id).
pub async fn mark_replied_external(pool: &PgPool, id: &str) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE replies SET status = 'replied', replied_at = now() WHERE id = $1 AND status = 'unreplied'"
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/replies.rs
git commit -m "feat: add DB queries for external reply detection"
```

---

### Task 5: Add `detect_external_replies()` to sync.rs

**Files:**
- Modify: `postgraph-server/src/sync.rs`

- [ ] **Step 1: Add the detection function**

Add at the end of `sync.rs`, before the `// ── Helpers` section (before line 249):

```rust
// ── Task 5: External Reply Detection ──────────────────────────────

/// Detect replies made outside postgraph by checking conversation threads
/// for the owner's username. Marks detected replies as 'replied'.
pub async fn detect_external_replies(
    pool: &PgPool,
    client: &ThreadsClient,
    owner_username: &str,
) -> Result<u64, AppError> {
    if owner_username.is_empty() {
        warn!("Owner username not set, skipping reply detection");
        return Ok(0);
    }

    let grouped = crate::replies::unreplied_grouped_by_parent(pool).await?;
    let parent_count = grouped.len();
    info!("Detecting external replies across {parent_count} parent posts");

    let mut detected: u64 = 0;

    for (parent_post_id, unreplied_replies) in &grouped {
        let conversation = match client.get_conversation(parent_post_id).await {
            Ok(c) => c,
            Err(AppError::RateLimited(_)) => {
                warn!("Rate limited during reply detection, stopping early");
                return Ok(detected);
            }
            Err(e) => {
                warn!("Failed to fetch conversation for {parent_post_id}: {e}");
                continue;
            }
        };

        // Find all our replies in this conversation (by username match)
        let our_replies: Vec<_> = conversation
            .iter()
            .filter(|r| r.username.as_deref() == Some(owner_username))
            .collect();

        if our_replies.is_empty() {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }

        // For each unreplied reply, check if we have a reply after it
        for (reply_id, reply_ts) in unreplied_replies {
            let we_replied = our_replies.iter().any(|our| {
                // If we have timestamp info, check our reply is after theirs
                match (reply_ts, &our.timestamp) {
                    (Some(their_ts), Some(our_ts_str)) => {
                        // Parse our timestamp
                        let our_ts = chrono::DateTime::parse_from_rfc3339(our_ts_str)
                            .ok()
                            .or_else(|| {
                                chrono::DateTime::parse_from_str(our_ts_str, "%Y-%m-%dT%H:%M:%S%z")
                                    .ok()
                            })
                            .map(|dt| dt.with_timezone(&chrono::Utc));
                        our_ts.is_some_and(|ot| ot > *their_ts)
                    }
                    // If timestamps are missing, presence of our reply is enough
                    _ => true,
                }
            });

            if we_replied {
                if crate::replies::mark_replied_external(pool, reply_id).await? {
                    detected += 1;
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    info!("Detected {detected} externally-replied replies across {parent_count} posts");
    Ok(detected)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/sync.rs
git commit -m "feat: add detect_external_replies() to sync"
```

---

### Task 6: Add the backend endpoint and wire into sync

**Files:**
- Modify: `postgraph-server/src/routes/replies.rs`
- Modify: `postgraph-server/src/main.rs` (route registration + sync wiring)

- [ ] **Step 1: Add the detect handler**

In `routes/replies.rs`, add a response struct and handler after the existing `dismiss_reply` function:

```rust
#[derive(Serialize)]
pub struct DetectResponse {
    pub detected: u64,
}

pub async fn detect_replies(
    State(state): State<AppState>,
) -> RepliesResult<DetectResponse> {
    let detected = crate::sync::detect_external_replies(
        &state.pool,
        &state.threads,
        &state.owner_username,
    )
    .await
    .map_err(internal)?;
    Ok(Json(DetectResponse { detected }))
}
```

- [ ] **Step 2: Register the route in main.rs**

In `main.rs`, add the route after the existing replies routes (after line 412):

```rust
.route("/api/replies/detect", post(routes::replies::detect_replies))
```

- [ ] **Step 3: Wire detection into the background sync**

In `main.rs`, after the reply sync in the background loop (after line 148, where `sync::sync_replies` is called), add:

```rust
// Task 3b: Detect externally-replied replies
if let Err(e) = sync::detect_external_replies(&bg_state.pool, &bg_state.threads, &bg_state.owner_username).await {
    tracing::error!("Background reply detection failed: {e}");
}
```

Note: The `owner_username` field is accessible because `bg_state` is a clone of `AppState` which owns the `String`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/routes/replies.rs postgraph-server/src/main.rs
git commit -m "feat: add /api/replies/detect endpoint and wire into background sync"
```

---

### Task 7: Add frontend API method and proxy

**Files:**
- Create: `web/src/routes/api/replies/detect/+server.ts`
- Modify: `web/src/lib/api.ts:520-551`

- [ ] **Step 1: Create the SvelteKit proxy endpoint**

Create `web/src/routes/api/replies/detect/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async () => {
  return proxyToBackend('/api/replies/detect', { method: 'POST' });
};
```

- [ ] **Step 2: Add the API method**

In `web/src/lib/api.ts`, add after `dismissReply` (before the closing `};` on line 551):

```typescript
  detectReplies: () =>
    fetch('/api/replies/detect', { method: 'POST' }).then(async r => {
      if (!r.ok) {
        const data = await r.json().catch(() => ({ error: `Detection failed (${r.status})` }));
        throw new Error(data.error ?? `Detection failed (${r.status})`);
      }
      return r.json() as Promise<{ detected: number }>;
    }),
```

- [ ] **Step 3: Verify the frontend builds**

Run: `cd web && npx svelte-check`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add web/src/routes/api/replies/detect/+server.ts web/src/lib/api.ts
git commit -m "feat: add detectReplies frontend API method and proxy"
```

---

### Task 8: Add "Detect Replies" button to Debug page

**Files:**
- Modify: `web/src/lib/components/Debug.svelte`

- [ ] **Step 1: Add state and handler**

In the `<script>` block, after the `backfillResult` state variable (line 10), add:

```typescript
let detecting = $state(false);
let detectResult: string | null = $state(null);
```

After the `runBackfill` function (after line 79), add:

```typescript
async function runDetectReplies() {
  detecting = true;
  detectResult = null;
  try {
    const result = await api.detectReplies();
    detectResult = `Detected ${result.detected} externally-replied replies`;
  } catch (e) {
    detectResult = e instanceof Error ? e.message : 'Detection failed';
  } finally {
    detecting = false;
  }
}
```

- [ ] **Step 2: Add the button to the toolbar**

In the template, after the "Backfill Emotions" button (after line 98), add:

```svelte
<button class="backfill-btn" onclick={runDetectReplies} disabled={detecting}>
  {detecting ? 'Detecting...' : 'Detect Replies'}
</button>
```

- [ ] **Step 3: Add the result display**

After the existing `backfillResult` display block (after line 104), add:

```svelte
{#if detectResult}
  <div class="backfill-result">{detectResult}</div>
{/if}
```

- [ ] **Step 4: Verify the frontend builds**

Run: `cd web && npx svelte-check`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add web/src/lib/components/Debug.svelte
git commit -m "feat: add Detect Replies button to debug page"
```

---

### Task 9: Manual end-to-end test

- [ ] **Step 1: Start the backend**

Run: `cargo run --package postgraph-server`
Expected: Startup log shows `Threads owner: @<your_username>`

- [ ] **Step 2: Start the frontend**

Run: `cd web && npm run dev`

- [ ] **Step 3: Test the detect endpoint directly**

Run: `curl -X POST http://localhost:8000/api/replies/detect -H "Authorization: Bearer $POSTGRAPH_API_KEY"`
Expected: `{"detected": N}` where N >= 0

- [ ] **Step 4: Test via the debug page**

Navigate to `/debug` in the browser, click "Detect Replies", verify the result message shows.

- [ ] **Step 5: Verify the replies page**

Navigate to `/replies`, confirm the Unreplied count decreased by the detected amount.

- [ ] **Step 6: Run pre-commit checks**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo check --workspace
cd web && npx svelte-check
```

- [ ] **Step 7: Final commit (if fmt/clippy made changes)**

```bash
git add -A
git commit -m "style: fmt and clippy fixes for reply detection"
```
