# postgraph Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Threads analytics platform with a Rust backend that syncs posts, analyzes them with Mercury LLM, computes a topic-based node graph, and serves data to a Svelte dashboard with Sigma.js visualization and Chart.js analytics.

**Architecture:** Rust API server (Shuttle + axum) connects to Shuttle-provisioned Postgres via sqlx. Background tasks sync from Threads API, run Mercury LLM analysis, and compute graph edges. Svelte frontend (SvelteKit on Netlify) polls the REST API and renders an interactive force-directed graph plus analytics charts.

**Tech Stack:** Rust (edition 2024), axum, sqlx, reqwest, tokio, Shuttle | SvelteKit, Sigma.js, Graphology, Chart.js | Postgres

**Spec:** `docs/superpowers/specs/2026-03-11-postgraph-design.md`

**Deferred to v1.1:** Token refresh lifecycle, graph endpoint pagination/LOD, sentiment trend charts, best/worst posts table, URL state persistence, keyboard shortcuts, export (JSON/CSV), re-analysis endpoint, additional edge types (theme, reply_chain, temporal_proximity), concurrent insights throttling. These are documented in the spec but not implemented in this plan to keep the MVP focused.

---

## Chunk 1: Rust Project Scaffold + Database

### Task 1: Cargo Workspace + Shuttle Boilerplate

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `postgraph-server/Cargo.toml`
- Create: `postgraph-server/src/main.rs`
- Create: `.gitignore`
- Create: `.env.example`

- [ ] **Step 1: Create workspace root Cargo.toml**

```toml
[workspace]
resolver = "2"
members = ["postgraph-server"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"
```

- [ ] **Step 2: Create postgraph-server/Cargo.toml**

```toml
[package]
name = "postgraph-server"
version.workspace = true
edition.workspace = true

[dependencies]
axum = "0.8"
shuttle-axum = "0.51"
shuttle-runtime = "0.51"
shuttle-shared-db = { version = "0.51", features = ["postgres", "sqlx"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "postgres", "chrono", "uuid"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
thiserror = "2"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
tower-http = { version = "0.6", features = ["cors"] }
tracing = "0.1"
```

- [ ] **Step 3: Create minimal postgraph-server/src/main.rs**

```rust
use axum::{Router, routing::get};
use shuttle_runtime::ShuttleAxum;
use sqlx::PgPool;

async fn health() -> &'static str {
    "ok"
}

#[shuttle_runtime::main]
async fn main(
    #[shuttle_shared_db::Postgres] pool: PgPool,
) -> ShuttleAxum {
    sqlx::migrate!().run(&pool).await.expect("migrations failed");

    let router = Router::new()
        .route("/health", get(health))
        .with_state(pool);

    Ok(router.into())
}
```

- [ ] **Step 4: Create .gitignore**

```
/target
.env
*.swp
web/node_modules
web/.svelte-kit
web/build
```

- [ ] **Step 5: Create .env.example**

```
THREADS_ACCESS_TOKEN=your_threads_token_here
MERCURY_API_KEY=your_mercury_api_key_here
MERCURY_API_URL=https://api.inceptionlabs.ai/v1
POSTGRAPH_API_KEY=your_dashboard_api_key_here
```

- [ ] **Step 6: Create migrations directory**

Run: `mkdir -p postgraph-server/migrations`

- [ ] **Step 7: Verify it compiles**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: scaffold Cargo workspace with Shuttle + axum boilerplate"
```

---

### Task 2: Database Migrations

**Files:**
- Create: `postgraph-server/migrations/001_initial_schema.sql`

- [ ] **Step 1: Write initial migration**

```sql
-- Posts cached from Threads API
CREATE TABLE posts (
    id TEXT PRIMARY KEY,
    text TEXT,
    media_type TEXT,
    media_url TEXT,
    timestamp TIMESTAMPTZ NOT NULL,
    permalink TEXT,
    likes INTEGER NOT NULL DEFAULT 0,
    replies_count INTEGER NOT NULL DEFAULT 0,
    reposts INTEGER NOT NULL DEFAULT 0,
    quotes INTEGER NOT NULL DEFAULT 0,
    sentiment REAL,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    analyzed_at TIMESTAMPTZ
);

-- LLM-extracted topics
CREATE TABLE topics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT
);

-- Post-to-topic many-to-many with relevance weight
CREATE TABLE post_topics (
    post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    topic_id UUID NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
    weight REAL NOT NULL DEFAULT 1.0,
    PRIMARY KEY (post_id, topic_id)
);
CREATE INDEX idx_post_topics_topic_id ON post_topics(topic_id);

-- Pre-computed graph edges between posts
CREATE TABLE post_edges (
    source_post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    target_post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    edge_type TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 0.0,
    PRIMARY KEY (source_post_id, target_post_id, edge_type)
);
CREATE INDEX idx_post_edges_target ON post_edges(target_post_id);

-- Engagement time-series
CREATE TABLE engagement_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    captured_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    likes INTEGER NOT NULL DEFAULT 0,
    replies_count INTEGER NOT NULL DEFAULT 0,
    reposts INTEGER NOT NULL DEFAULT 0,
    quotes INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_engagement_post_id ON engagement_snapshots(post_id);
CREATE INDEX idx_engagement_captured_at ON engagement_snapshots(captured_at);

-- Sync bookkeeping
CREATE TABLE sync_state (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    last_sync_cursor TEXT,
    last_sync_at TIMESTAMPTZ
);
INSERT INTO sync_state (id) VALUES (1);

-- Threads API token storage
CREATE TABLE api_tokens (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    access_token TEXT NOT NULL,
    expires_at TIMESTAMPTZ,
    refreshed_at TIMESTAMPTZ DEFAULT NOW()
);
```

- [ ] **Step 2: Verify migration compiles with sqlx**

Run: `cargo check --workspace`
Expected: Compiles (migration file is just SQL, checked at runtime)

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/migrations/
git commit -m "feat: add initial database schema migration"
```

---

### Task 3: Database Module + Types

**Files:**
- Create: `postgraph-server/src/db.rs`
- Create: `postgraph-server/src/types.rs`
- Modify: `postgraph-server/src/main.rs` (add module declarations)

- [ ] **Step 1: Create types.rs with all domain structs**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Post {
    pub id: String,
    pub text: Option<String>,
    pub media_type: Option<String>,
    pub media_url: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub permalink: Option<String>,
    pub likes: i32,
    pub replies_count: i32,
    pub reposts: i32,
    pub quotes: i32,
    pub sentiment: Option<f32>,
    pub synced_at: DateTime<Utc>,
    pub analyzed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Topic {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PostTopic {
    pub post_id: String,
    pub topic_id: Uuid,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PostEdge {
    pub source_post_id: String,
    pub target_post_id: String,
    pub edge_type: String,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EngagementSnapshot {
    pub id: Uuid,
    pub post_id: String,
    pub captured_at: DateTime<Utc>,
    pub likes: i32,
    pub replies_count: i32,
    pub reposts: i32,
    pub quotes: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SyncState {
    pub id: i32,
    pub last_sync_cursor: Option<String>,
    pub last_sync_at: Option<DateTime<Utc>>,
}
```

- [ ] **Step 2: Create db.rs with core queries**

```rust
use sqlx::PgPool;
use crate::types::*;

// -- Posts --

pub async fn upsert_post(pool: &PgPool, post: &Post) -> sqlx::Result<()> {
    sqlx::query(
        r#"INSERT INTO posts (id, text, media_type, media_url, timestamp, permalink, likes, replies_count, reposts, quotes, synced_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW())
           ON CONFLICT (id) DO UPDATE SET
             likes = EXCLUDED.likes,
             replies_count = EXCLUDED.replies_count,
             reposts = EXCLUDED.reposts,
             quotes = EXCLUDED.quotes,
             synced_at = NOW()"#,
    )
    .bind(&post.id)
    .bind(&post.text)
    .bind(&post.media_type)
    .bind(&post.media_url)
    .bind(post.timestamp)
    .bind(&post.permalink)
    .bind(post.likes)
    .bind(post.replies_count)
    .bind(post.reposts)
    .bind(post.quotes)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_unanalyzed_posts(pool: &PgPool, limit: i64) -> sqlx::Result<Vec<Post>> {
    sqlx::query_as::<_, Post>(
        "SELECT * FROM posts WHERE analyzed_at IS NULL ORDER BY timestamp DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn get_all_posts(pool: &PgPool) -> sqlx::Result<Vec<Post>> {
    sqlx::query_as::<_, Post>("SELECT * FROM posts ORDER BY timestamp DESC")
        .fetch_all(pool)
        .await
}

pub async fn mark_post_analyzed(
    pool: &PgPool,
    post_id: &str,
    sentiment: f32,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE posts SET analyzed_at = NOW(), sentiment = $1 WHERE id = $2")
        .bind(sentiment)
        .bind(post_id)
        .execute(pool)
        .await?;
    Ok(())
}

// -- Topics --

pub async fn upsert_topic(pool: &PgPool, name: &str, description: &str) -> sqlx::Result<Topic> {
    sqlx::query_as::<_, Topic>(
        r#"INSERT INTO topics (name, description)
           VALUES ($1, $2)
           ON CONFLICT (name) DO UPDATE SET description = EXCLUDED.description
           RETURNING *"#,
    )
    .bind(name)
    .bind(description)
    .fetch_one(pool)
    .await
}

pub async fn get_all_topics(pool: &PgPool) -> sqlx::Result<Vec<Topic>> {
    sqlx::query_as::<_, Topic>("SELECT * FROM topics ORDER BY name")
        .fetch_all(pool)
        .await
}

pub async fn upsert_post_topic(
    pool: &PgPool,
    post_id: &str,
    topic_id: uuid::Uuid,
    weight: f32,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"INSERT INTO post_topics (post_id, topic_id, weight)
           VALUES ($1, $2, $3)
           ON CONFLICT (post_id, topic_id) DO UPDATE SET weight = EXCLUDED.weight"#,
    )
    .bind(post_id)
    .bind(topic_id)
    .bind(weight)
    .execute(pool)
    .await?;
    Ok(())
}

// -- Edges --

pub async fn upsert_edge(pool: &PgPool, edge: &PostEdge) -> sqlx::Result<()> {
    sqlx::query(
        r#"INSERT INTO post_edges (source_post_id, target_post_id, edge_type, weight)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (source_post_id, target_post_id, edge_type)
           DO UPDATE SET weight = EXCLUDED.weight"#,
    )
    .bind(&edge.source_post_id)
    .bind(&edge.target_post_id)
    .bind(&edge.edge_type)
    .bind(edge.weight)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_all_edges(pool: &PgPool) -> sqlx::Result<Vec<PostEdge>> {
    sqlx::query_as::<_, PostEdge>("SELECT * FROM post_edges WHERE weight >= 0.1")
        .fetch_all(pool)
        .await
}

// -- Engagement Snapshots --

pub async fn insert_engagement_snapshot(
    pool: &PgPool,
    post_id: &str,
    likes: i32,
    replies_count: i32,
    reposts: i32,
    quotes: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"INSERT INTO engagement_snapshots (post_id, likes, replies_count, reposts, quotes)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(post_id)
    .bind(likes)
    .bind(replies_count)
    .bind(reposts)
    .bind(quotes)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_engagement_history(
    pool: &PgPool,
    post_id: &str,
) -> sqlx::Result<Vec<EngagementSnapshot>> {
    sqlx::query_as::<_, EngagementSnapshot>(
        "SELECT * FROM engagement_snapshots WHERE post_id = $1 ORDER BY captured_at",
    )
    .bind(post_id)
    .fetch_all(pool)
    .await
}

// -- Sync State --

pub async fn get_sync_state(pool: &PgPool) -> sqlx::Result<SyncState> {
    sqlx::query_as::<_, SyncState>("SELECT * FROM sync_state WHERE id = 1")
        .fetch_one(pool)
        .await
}

pub async fn update_sync_state(
    pool: &PgPool,
    cursor: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE sync_state SET last_sync_cursor = $1, last_sync_at = NOW() WHERE id = 1")
        .bind(cursor)
        .execute(pool)
        .await?;
    Ok(())
}
```

- [ ] **Step 3: Add module declarations to main.rs**

Add to the top of `postgraph-server/src/main.rs`:

```rust
mod db;
mod types;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/db.rs postgraph-server/src/types.rs postgraph-server/src/main.rs
git commit -m "feat: add database module with types and core queries"
```

---

## Chunk 2: Threads API Client + Sync Pipeline

### Task 4: Threads API Client

**Files:**
- Create: `postgraph-server/src/threads.rs`
- Create: `postgraph-server/src/error.rs`
- Modify: `postgraph-server/src/main.rs` (add module declarations)

- [ ] **Step 1: Create error.rs**

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Threads API error: {0}")]
    ThreadsApi(String),

    #[error("Mercury API error: {0}")]
    MercuryApi(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Rate limited, retry after {0}s")]
    RateLimited(u64),
}
```

- [ ] **Step 2: Create threads.rs**

```rust
use reqwest::Client;
use serde::Deserialize;
use crate::error::AppError;

const BASE_URL: &str = "https://graph.threads.net/v1.0";

pub struct ThreadsClient {
    client: Client,
    access_token: String,
}

#[derive(Debug, Deserialize)]
pub struct ThreadsPost {
    pub id: String,
    pub text: Option<String>,
    pub media_type: Option<String>,
    pub media_url: Option<String>,
    pub timestamp: Option<String>,
    pub permalink: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ThreadsPaging {
    pub cursors: Option<ThreadsCursors>,
    pub next: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ThreadsCursors {
    pub after: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ThreadsListResponse {
    pub data: Vec<ThreadsPost>,
    pub paging: Option<ThreadsPaging>,
}

#[derive(Debug, Deserialize)]
pub struct InsightValue {
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct InsightData {
    pub name: String,
    pub values: Option<Vec<InsightValue>>,
    // Some metrics return total_value instead of values
    pub total_value: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct InsightsResponse {
    pub data: Vec<InsightData>,
}

#[derive(Debug, Default)]
pub struct PostInsights {
    pub likes: i32,
    pub replies: i32,
    pub reposts: i32,
    pub quotes: i32,
}

impl ThreadsClient {
    pub fn new(access_token: String) -> Self {
        Self {
            client: Client::new(),
            access_token,
        }
    }

    pub async fn get_user_threads(
        &self,
        cursor: Option<&str>,
    ) -> Result<ThreadsListResponse, AppError> {
        let mut url = format!(
            "{}/me/threads?fields=id,text,media_type,media_url,timestamp,permalink&access_token={}",
            BASE_URL, self.access_token
        );
        if let Some(c) = cursor {
            url.push_str(&format!("&after={}", c));
        }

        let resp = self.client.get(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::ThreadsApi(body));
        }
        let data: ThreadsListResponse = resp.json().await?;
        Ok(data)
    }

    pub async fn get_post_insights(&self, post_id: &str) -> Result<PostInsights, AppError> {
        let url = format!(
            "{}/{}/insights?metric=likes,replies,reposts,quotes&access_token={}",
            BASE_URL, post_id, self.access_token
        );

        let resp = self.client.get(&url).send().await?;
        if resp.status() == 429 {
            return Err(AppError::RateLimited(60));
        }
        if !resp.status().is_success() {
            // Some posts may not support insights; return zeros
            return Ok(PostInsights::default());
        }

        let data: InsightsResponse = resp.json().await?;
        let mut insights = PostInsights::default();

        for item in &data.data {
            let value = item
                .total_value
                .as_ref()
                .and_then(|v| v.as_i64())
                .or_else(|| {
                    item.values
                        .as_ref()
                        .and_then(|vals| vals.first())
                        .and_then(|v| v.value.as_ref())
                        .and_then(|v| v.as_i64())
                })
                .unwrap_or(0) as i32;

            match item.name.as_str() {
                "likes" => insights.likes = value,
                "replies" => insights.replies = value,
                "reposts" => insights.reposts = value,
                "quotes" => insights.quotes = value,
                _ => {}
            }
        }

        Ok(insights)
    }
}
```

- [ ] **Step 3: Add module declarations to main.rs**

Add to `postgraph-server/src/main.rs`:

```rust
mod error;
mod threads;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/error.rs postgraph-server/src/threads.rs postgraph-server/src/main.rs
git commit -m "feat: add Threads API client with post listing and insights"
```

---

### Task 5: Sync Pipeline

**Files:**
- Create: `postgraph-server/src/sync.rs`
- Modify: `postgraph-server/src/main.rs` (add module)

- [ ] **Step 1: Create sync.rs**

```rust
use chrono::{DateTime, Utc};
use sqlx::{self, PgPool};
use std::time::Duration;
use tracing::{info, warn};

use crate::db;
use crate::error::AppError;
use crate::threads::{ThreadsClient, ThreadsPost};
use crate::types::Post;

fn parse_threads_timestamp(ts: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn threads_post_to_post(tp: &ThreadsPost) -> Post {
    Post {
        id: tp.id.clone(),
        text: tp.text.clone(),
        media_type: tp.media_type.clone(),
        media_url: tp.media_url.clone(),
        timestamp: tp
            .timestamp
            .as_deref()
            .map(parse_threads_timestamp)
            .unwrap_or_else(Utc::now),
        permalink: tp.permalink.clone(),
        likes: 0,
        replies_count: 0,
        reposts: 0,
        quotes: 0,
        sentiment: None,
        synced_at: Utc::now(),
        analyzed_at: None,
    }
}

pub async fn run_sync(pool: &PgPool, client: &ThreadsClient) -> Result<u32, AppError> {
    let sync_state = db::get_sync_state(pool).await?;
    let mut cursor = sync_state.last_sync_cursor;
    let mut total_synced: u32 = 0;

    loop {
        let response = client.get_user_threads(cursor.as_deref()).await?;
        let post_count = response.data.len();

        for tp in &response.data {
            let post = threads_post_to_post(tp);
            db::upsert_post(pool, &post).await?;

            // Fetch insights with throttling
            match client.get_post_insights(&tp.id).await {
                Ok(insights) => {
                    sqlx::query(
                        "UPDATE posts SET likes = $1, replies_count = $2, reposts = $3, quotes = $4 WHERE id = $5",
                    )
                    .bind(insights.likes)
                    .bind(insights.replies)
                    .bind(insights.reposts)
                    .bind(insights.quotes)
                    .bind(&tp.id)
                    .execute(pool)
                    .await?;

                    db::insert_engagement_snapshot(
                        pool,
                        &tp.id,
                        insights.likes,
                        insights.replies,
                        insights.reposts,
                        insights.quotes,
                    )
                    .await?;
                }
                Err(AppError::RateLimited(secs)) => {
                    warn!("Rate limited fetching insights for {}, waiting {}s", tp.id, secs);
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                }
                Err(e) => {
                    warn!("Failed to fetch insights for {}: {}", tp.id, e);
                }
            }

            // Throttle between insight calls
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        total_synced += post_count as u32;
        info!("Synced {} posts (batch of {})", total_synced, post_count);

        // Update cursor
        let next_cursor = response
            .paging
            .as_ref()
            .and_then(|p| p.cursors.as_ref())
            .and_then(|c| c.after.clone());

        db::update_sync_state(pool, next_cursor.as_deref()).await?;

        if response.paging.and_then(|p| p.next).is_none() {
            break;
        }

        cursor = next_cursor;
    }

    Ok(total_synced)
}
```

- [ ] **Step 2: Add module to main.rs**

Add `mod sync;` to `postgraph-server/src/main.rs`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/sync.rs postgraph-server/src/main.rs
git commit -m "feat: add sync pipeline for Threads posts and insights"
```

---

### Task 6: Mercury LLM Client

**Files:**
- Create: `postgraph-server/src/mercury.rs`
- Modify: `postgraph-server/src/main.rs` (add module)

- [ ] **Step 1: Create mercury.rs**

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};
use crate::error::AppError;

pub struct MercuryClient {
    client: Client,
    api_key: String,
    api_url: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedPost {
    pub post_id: String,
    pub topics: Vec<TopicAssignment>,
    pub sentiment: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicAssignment {
    pub name: String,
    pub description: String,
    pub weight: f32,
}

#[derive(Debug, Deserialize)]
pub struct AnalysisResponse {
    pub posts: Vec<AnalyzedPost>,
}

impl MercuryClient {
    pub fn new(api_key: String, api_url: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            api_url,
        }
    }

    pub async fn analyze_posts(
        &self,
        posts: &[(String, String)], // (id, text)
        existing_topics: &[String],
    ) -> Result<AnalysisResponse, AppError> {
        let topics_list = if existing_topics.is_empty() {
            "No existing topics yet.".to_string()
        } else {
            existing_topics.join(", ")
        };

        let posts_json: Vec<serde_json::Value> = posts
            .iter()
            .map(|(id, text)| serde_json::json!({"id": id, "text": text}))
            .collect();
        let posts_json_str = serde_json::to_string_pretty(&posts_json).unwrap_or_default();

        let prompt = format!(
            r#"Analyze these social media posts. For each post, extract:
1. Topics (map to existing topics when possible, create new ones only when needed)
2. Sentiment (-1.0 to 1.0)

Existing topics: [{topics_list}]

Posts:
{posts_json_str}

Respond with ONLY valid JSON in this exact format:
{{
  "posts": [
    {{
      "post_id": "the id",
      "topics": [
        {{"name": "Topic Name", "description": "Brief description", "weight": 0.8}}
      ],
      "sentiment": 0.5
    }}
  ]
}}"#
        );

        let request = ChatRequest {
            model: "mercury-coder-small".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
            }],
            temperature: 0.3,
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::MercuryApi(body));
        }

        let chat_resp: ChatResponse = resp.json().await?;
        let content = chat_resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        // Parse JSON from response, stripping markdown code fences if present
        let json_str = content
            .trim()
            .strip_prefix("```json")
            .or_else(|| content.trim().strip_prefix("```"))
            .unwrap_or(content.trim())
            .strip_suffix("```")
            .unwrap_or(content.trim())
            .trim();

        let analysis: AnalysisResponse =
            serde_json::from_str(json_str).map_err(|e| AppError::MercuryApi(format!("Failed to parse response: {e}. Raw: {json_str}")))?;

        Ok(analysis)
    }
}
```

- [ ] **Step 2: Add module to main.rs**

Add `mod mercury;` to `postgraph-server/src/main.rs`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/mercury.rs postgraph-server/src/main.rs
git commit -m "feat: add Mercury LLM client with post analysis"
```

---

### Task 7: Analysis Orchestration + Edge Computation

**Files:**
- Create: `postgraph-server/src/analysis.rs`
- Create: `postgraph-server/src/graph.rs`
- Modify: `postgraph-server/src/main.rs` (add modules)

- [ ] **Step 1: Create analysis.rs**

```rust
use sqlx::PgPool;
use tracing::{info, warn};

use crate::db;
use crate::error::AppError;
use crate::mercury::MercuryClient;

const ANALYSIS_BATCH_SIZE: i64 = 15;

pub async fn run_analysis(pool: &PgPool, mercury: &MercuryClient) -> Result<u32, AppError> {
    let unanalyzed = db::get_unanalyzed_posts(pool, ANALYSIS_BATCH_SIZE).await?;
    if unanalyzed.is_empty() {
        return Ok(0);
    }

    let existing_topics = db::get_all_topics(pool).await?;
    let topic_names: Vec<String> = existing_topics.iter().map(|t| t.name.clone()).collect();

    let posts_for_llm: Vec<(String, String)> = unanalyzed
        .iter()
        .filter_map(|p| {
            p.text
                .as_ref()
                .map(|text| (p.id.clone(), text.clone()))
        })
        .collect();

    if posts_for_llm.is_empty() {
        // All posts are media-only with no text; mark them analyzed with neutral sentiment
        for post in &unanalyzed {
            db::mark_post_analyzed(pool, &post.id, 0.0).await?;
        }
        return Ok(unanalyzed.len() as u32);
    }

    let result = match mercury.analyze_posts(&posts_for_llm, &topic_names).await {
        Ok(r) => r,
        Err(e) => {
            warn!("Mercury analysis failed: {e}");
            return Err(e);
        }
    };

    let mut analyzed_count: u32 = 0;

    for analyzed in &result.posts {
        // Upsert topics and create post_topics links
        for topic_assignment in &analyzed.topics {
            let topic =
                db::upsert_topic(pool, &topic_assignment.name, &topic_assignment.description)
                    .await?;
            db::upsert_post_topic(pool, &analyzed.post_id, topic.id, topic_assignment.weight)
                .await?;
        }

        db::mark_post_analyzed(pool, &analyzed.post_id, analyzed.sentiment).await?;
        analyzed_count += 1;
    }

    info!("Analyzed {} posts", analyzed_count);
    Ok(analyzed_count)
}
```

- [ ] **Step 2: Create graph.rs**

```rust
use sqlx::PgPool;
use tracing::info;

use crate::db;
use crate::error::AppError;
use crate::types::PostEdge;

const EDGE_WEIGHT_THRESHOLD: f32 = 0.1;

pub async fn compute_edges_for_post(pool: &PgPool, post_id: &str) -> Result<u32, AppError> {
    // Find all posts that share topics with this post via SQL
    let shared_topic_edges: Vec<(String, f32)> = sqlx::query_as::<_, (String, f32)>(
        r#"SELECT pt2.post_id, SUM(pt1.weight * pt2.weight) as edge_weight
           FROM post_topics pt1
           JOIN post_topics pt2 ON pt1.topic_id = pt2.topic_id AND pt1.post_id != pt2.post_id
           WHERE pt1.post_id = $1
           GROUP BY pt2.post_id
           HAVING SUM(pt1.weight * pt2.weight) >= $2"#,
    )
    .bind(post_id)
    .bind(EDGE_WEIGHT_THRESHOLD)
    .fetch_all(pool)
    .await?;

    let mut edge_count: u32 = 0;

    for (target_id, weight) in &shared_topic_edges {
        let edge = PostEdge {
            source_post_id: post_id.to_string(),
            target_post_id: target_id.clone(),
            edge_type: "topic_overlap".to_string(),
            weight: *weight,
        };
        db::upsert_edge(pool, &edge).await?;
        edge_count += 1;
    }

    info!("Computed {} edges for post {}", edge_count, post_id);
    Ok(edge_count)
}

pub async fn compute_edges_for_recent(pool: &PgPool) -> Result<u32, AppError> {
    // Find posts that were recently analyzed but may not have edges yet
    let recently_analyzed: Vec<String> = sqlx::query_scalar::<_, String>(
        r#"SELECT p.id FROM posts p
           WHERE p.analyzed_at IS NOT NULL
           AND NOT EXISTS (
               SELECT 1 FROM post_edges pe WHERE pe.source_post_id = p.id
           )
           AND EXISTS (
               SELECT 1 FROM post_topics pt WHERE pt.post_id = p.id
           )
           LIMIT 50"#,
    )
    .fetch_all(pool)
    .await?;

    let mut total_edges: u32 = 0;
    for post_id in &recently_analyzed {
        total_edges += compute_edges_for_post(pool, post_id).await?;
    }

    Ok(total_edges)
}
```

- [ ] **Step 3: Add modules to main.rs**

Add `mod analysis;` and `mod graph;` to `postgraph-server/src/main.rs`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/analysis.rs postgraph-server/src/graph.rs postgraph-server/src/main.rs
git commit -m "feat: add analysis orchestration and edge computation"
```

---

## Chunk 3: REST API Routes + Background Scheduler

### Task 8: Auth Middleware + App State

**Files:**
- Create: `postgraph-server/src/auth.rs`
- Create: `postgraph-server/src/state.rs`
- Modify: `postgraph-server/src/main.rs`

- [ ] **Step 1: Create state.rs**

```rust
use sqlx::PgPool;
use std::sync::Arc;
use crate::mercury::MercuryClient;
use crate::threads::ThreadsClient;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub threads: Arc<ThreadsClient>,
    pub mercury: Arc<MercuryClient>,
    pub api_key: String,
}
```

- [ ] **Step 2: Create auth.rs**

```rust
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::state::AppState;

pub async fn require_api_key(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(header) if header.strip_prefix("Bearer ").unwrap_or("") == state.api_key => {
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
```

- [ ] **Step 3: Add modules to main.rs**

Add `mod auth;` and `mod state;` to `postgraph-server/src/main.rs`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/auth.rs postgraph-server/src/state.rs postgraph-server/src/main.rs
git commit -m "feat: add auth middleware and app state"
```

---

### Task 9: API Routes

**Files:**
- Create: `postgraph-server/src/routes/mod.rs`
- Create: `postgraph-server/src/routes/graph.rs`
- Create: `postgraph-server/src/routes/posts.rs`
- Create: `postgraph-server/src/routes/analytics.rs`
- Create: `postgraph-server/src/routes/sync.rs`
- Modify: `postgraph-server/src/main.rs`

- [ ] **Step 1: Create routes directory**

Run: `mkdir -p postgraph-server/src/routes`

- [ ] **Step 2: Create routes/posts.rs**

```rust
use axum::{extract::State, Json};
use crate::db;
use crate::state::AppState;
use crate::types::Post;

pub async fn list_posts(State(state): State<AppState>) -> Result<Json<Vec<Post>>, axum::http::StatusCode> {
    db::get_all_posts(&state.pool)
        .await
        .map(Json)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}
```

- [ ] **Step 3: Create routes/graph.rs**

```rust
use axum::{extract::State, Json};
use serde::Serialize;
use sqlx;
use crate::db;
use crate::state::AppState;

#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub size: f32,
    pub sentiment: Option<f32>,
    pub topics: Vec<String>,
}

#[derive(Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub weight: f32,
    pub edge_type: String,
}

#[derive(Serialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub async fn get_graph(State(state): State<AppState>) -> Result<Json<GraphData>, axum::http::StatusCode> {
    let posts = db::get_all_posts(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let edges = db::get_all_edges(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch topics for each post
    let all_post_topics: Vec<(String, String)> = sqlx::query_as::<_, (String, String)>(
        "SELECT pt.post_id, t.name FROM post_topics pt JOIN topics t ON pt.topic_id = t.id",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let nodes: Vec<GraphNode> = posts
        .iter()
        .filter(|p| p.analyzed_at.is_some())
        .map(|p| {
            let topics: Vec<String> = all_post_topics
                .iter()
                .filter(|(pid, _)| pid == &p.id)
                .map(|(_, name)| name.clone())
                .collect();

            let engagement = (p.likes + p.replies_count + p.reposts + p.quotes) as f32;
            let size = (engagement + 1.0).ln().max(0.0) + 1.0;

            GraphNode {
                id: p.id.clone(),
                label: p.text.as_deref().unwrap_or("").chars().take(80).collect(),
                size,
                sentiment: p.sentiment,
                topics,
            }
        })
        .collect();

    let graph_edges: Vec<GraphEdge> = edges
        .iter()
        .map(|e| GraphEdge {
            source: e.source_post_id.clone(),
            target: e.target_post_id.clone(),
            weight: e.weight,
            edge_type: e.edge_type.clone(),
        })
        .collect();

    Ok(Json(GraphData {
        nodes,
        edges: graph_edges,
    }))
}
```

- [ ] **Step 4: Create routes/analytics.rs**

```rust
use axum::{extract::State, Json};
use serde::Serialize;
use sqlx;
use crate::db;
use crate::state::AppState;

#[derive(Serialize)]
pub struct AnalyticsData {
    pub total_posts: usize,
    pub analyzed_posts: usize,
    pub total_topics: usize,
    pub topics: Vec<TopicSummary>,
    pub engagement_over_time: Vec<EngagementPoint>,
}

#[derive(Serialize)]
pub struct TopicSummary {
    pub name: String,
    pub post_count: i64,
    pub avg_engagement: f64,
}

#[derive(Serialize)]
pub struct EngagementPoint {
    pub date: String,
    pub likes: i64,
    pub replies: i64,
    pub reposts: i64,
}

pub async fn get_analytics(
    State(state): State<AppState>,
) -> Result<Json<AnalyticsData>, axum::http::StatusCode> {
    let posts = db::get_all_posts(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let topics = db::get_all_topics(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let topic_summaries: Vec<TopicSummary> = sqlx::query_as::<_, (String, i64, f64)>(
        r#"SELECT t.name, COUNT(pt.post_id) as post_count,
           AVG(p.likes + p.replies_count + p.reposts + p.quotes)::float8 as avg_engagement
           FROM topics t
           JOIN post_topics pt ON t.id = pt.topic_id
           JOIN posts p ON pt.post_id = p.id
           GROUP BY t.name
           ORDER BY post_count DESC"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(|(name, count, avg)| TopicSummary {
        name,
        post_count: count,
        avg_engagement: avg,
    })
    .collect();

    let engagement_over_time: Vec<EngagementPoint> = sqlx::query_as::<_, (String, i64, i64, i64)>(
        r#"SELECT DATE(captured_at)::text as date,
           SUM(likes)::bigint, SUM(replies_count)::bigint, SUM(reposts)::bigint
           FROM engagement_snapshots
           GROUP BY DATE(captured_at)
           ORDER BY date"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(|(date, likes, replies, reposts)| EngagementPoint {
        date,
        likes,
        replies,
        reposts,
    })
    .collect();

    let analyzed_count = posts.iter().filter(|p| p.analyzed_at.is_some()).count();

    Ok(Json(AnalyticsData {
        total_posts: posts.len(),
        analyzed_posts: analyzed_count,
        total_topics: topics.len(),
        topics: topic_summaries,
        engagement_over_time,
    }))
}
```

- [ ] **Step 5: Create routes/sync.rs**

```rust
use axum::{extract::State, Json};
use serde::Serialize;
use tracing::info;

use crate::analysis;
use crate::graph;
use crate::state::AppState;
use crate::sync::run_sync;

#[derive(Serialize)]
pub struct SyncResult {
    pub posts_synced: u32,
    pub posts_analyzed: u32,
    pub edges_computed: u32,
}

pub async fn trigger_sync(
    State(state): State<AppState>,
) -> Result<Json<SyncResult>, axum::http::StatusCode> {
    info!("Manual sync triggered");

    let posts_synced = run_sync(&state.pool, &state.threads)
        .await
        .map_err(|e| {
            tracing::error!("Sync failed: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let posts_analyzed = analysis::run_analysis(&state.pool, &state.mercury)
        .await
        .map_err(|e| {
            tracing::error!("Analysis failed: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let edges_computed = graph::compute_edges_for_recent(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("Edge computation failed: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(SyncResult {
        posts_synced,
        posts_analyzed,
        edges_computed,
    }))
}
```

- [ ] **Step 6: Create routes/mod.rs**

```rust
pub mod analytics;
pub mod graph;
pub mod posts;
pub mod sync;
```

- [ ] **Step 7: Rewrite main.rs with full router**

```rust
mod analysis;
mod auth;
mod db;
mod error;
mod graph;
mod mercury;
mod routes;
mod state;
mod sync;
mod threads;
mod types;

use axum::{middleware, routing::{get, post}, Router};
use shuttle_runtime::ShuttleAxum;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::mercury::MercuryClient;
use crate::state::AppState;
use crate::threads::ThreadsClient;

#[shuttle_runtime::main]
async fn main(
    #[shuttle_shared_db::Postgres] pool: PgPool,
) -> ShuttleAxum {
    sqlx::migrate!().run(&pool).await.expect("migrations failed");

    let threads_token = std::env::var("THREADS_ACCESS_TOKEN")
        .expect("THREADS_ACCESS_TOKEN must be set");
    let mercury_key = std::env::var("MERCURY_API_KEY")
        .expect("MERCURY_API_KEY must be set");
    let mercury_url = std::env::var("MERCURY_API_URL")
        .unwrap_or_else(|_| "https://api.inceptionlabs.ai/v1".to_string());
    let api_key = std::env::var("POSTGRAPH_API_KEY")
        .expect("POSTGRAPH_API_KEY must be set");

    let state = AppState {
        pool: pool.clone(),
        threads: Arc::new(ThreadsClient::new(threads_token)),
        mercury: Arc::new(MercuryClient::new(mercury_key, mercury_url)),
        api_key,
    };

    // Spawn background sync task (first run after 30s delay, then every 15 min)
    let bg_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(15 * 60));
        loop {
            interval.tick().await;
            info!("Background sync starting");
            if let Err(e) = sync::run_sync(&bg_state.pool, &bg_state.threads).await {
                tracing::error!("Background sync failed: {e}");
                continue;
            }
            if let Err(e) = analysis::run_analysis(&bg_state.pool, &bg_state.mercury).await {
                tracing::error!("Background analysis failed: {e}");
            }
            if let Err(e) = graph::compute_edges_for_recent(&bg_state.pool).await {
                tracing::error!("Background edge computation failed: {e}");
            }
        }
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_routes = Router::new()
        .route("/api/posts", get(routes::posts::list_posts))
        .route("/api/graph", get(routes::graph::get_graph))
        .route("/api/analytics", get(routes::analytics::get_analytics))
        .route("/api/sync", post(routes::sync::trigger_sync))
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_api_key));

    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(api_routes)
        .layer(cors)
        .with_state(state);

    Ok(router.into())
}
```

- [ ] **Step 8: Verify it compiles**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 9: Commit**

```bash
git add postgraph-server/src/routes/ postgraph-server/src/main.rs
git commit -m "feat: add REST API routes and background sync scheduler"
```

---

## Chunk 4: Svelte Frontend — Scaffold + Graph

### Task 10: SvelteKit Project Setup

**Files:**
- Create: `web/` (SvelteKit project via scaffolding)
- Create: `web/netlify.toml`
- Create: `web/.env.example`

- [ ] **Step 1: Scaffold SvelteKit project**

Run from project root:
```bash
cd web && npx sv create . --template minimal --types ts
```
Select: Svelte 5, TypeScript, minimal template

- [ ] **Step 2: Install dependencies**

```bash
cd web && npm install sigma graphology graphology-layout-forceatlas2 graphology-communities-louvain chart.js
```

- [ ] **Step 3: Create web/netlify.toml**

```toml
[build]
  command = "npm run build"
  publish = "build"

[build.environment]
  NODE_VERSION = "20"
```

- [ ] **Step 4: Create web/.env.example**

```
PUBLIC_API_URL=http://localhost:8000
PUBLIC_API_KEY=your_api_key_here
```

- [ ] **Step 5: Verify it builds**

Run: `cd web && npm run build`
Expected: Builds successfully

- [ ] **Step 6: Commit**

```bash
git add web/
git commit -m "feat: scaffold SvelteKit project with graph dependencies"
```

---

### Task 11: API Client + Stores

**Files:**
- Create: `web/src/lib/api.ts`
- Create: `web/src/lib/stores/graph.ts`

- [ ] **Step 1: Create web/src/lib/api.ts**

```typescript
import { env } from '$env/dynamic/public';

const API_URL = env.PUBLIC_API_URL || 'http://localhost:8000';
const API_KEY = env.PUBLIC_API_KEY || '';

async function fetchApi<T>(path: string): Promise<T> {
  const res = await fetch(`${API_URL}${path}`, {
    headers: {
      'Authorization': `Bearer ${API_KEY}`,
      'Content-Type': 'application/json',
    },
  });
  if (!res.ok) throw new Error(`API error: ${res.status}`);
  return res.json();
}

export interface GraphNode {
  id: string;
  label: string;
  size: number;
  sentiment: number | null;
  topics: string[];
}

export interface GraphEdge {
  source: string;
  target: string;
  weight: number;
  edge_type: string;
}

export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface AnalyticsData {
  total_posts: number;
  analyzed_posts: number;
  total_topics: number;
  topics: TopicSummary[];
  engagement_over_time: EngagementPoint[];
}

export interface TopicSummary {
  name: string;
  post_count: number;
  avg_engagement: number;
}

export interface EngagementPoint {
  date: string;
  likes: number;
  replies: number;
  reposts: number;
}

export interface Post {
  id: string;
  text: string | null;
  timestamp: string;
  likes: number;
  replies_count: number;
  reposts: number;
  quotes: number;
  sentiment: number | null;
}

export interface SyncResult {
  posts_synced: number;
  posts_analyzed: number;
  edges_computed: number;
}

export const api = {
  getGraph: () => fetchApi<GraphData>('/api/graph'),
  getPosts: () => fetchApi<Post[]>('/api/posts'),
  getAnalytics: () => fetchApi<AnalyticsData>('/api/analytics'),
  triggerSync: () => fetch(`${API_URL}/api/sync`, {
    method: 'POST',
    headers: { 'Authorization': `Bearer ${API_KEY}` },
  }).then(r => r.json() as Promise<SyncResult>),
};
```

- [ ] **Step 2: Create web/src/lib/stores/graph.ts**

```typescript
import { writable } from 'svelte/store';
import Graph from 'graphology';
import { api, type GraphData } from '$lib/api';

export const graphData = writable<GraphData | null>(null);
export const graphInstance = writable<Graph | null>(null);
export const selectedNode = writable<string | null>(null);
export const loading = writable(false);
export const error = writable<string | null>(null);

export async function loadGraph() {
  loading.set(true);
  error.set(null);
  try {
    const data = await api.getGraph();
    graphData.set(data);

    const graph = new Graph();
    for (const node of data.nodes) {
      graph.addNode(node.id, {
        label: node.label,
        size: node.size,
        sentiment: node.sentiment,
        topics: node.topics,
        x: Math.random() * 100,
        y: Math.random() * 100,
      });
    }
    for (const edge of data.edges) {
      if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
        graph.addEdge(edge.source, edge.target, {
          weight: edge.weight,
          type: edge.edge_type,
        });
      }
    }
    graphInstance.set(graph);
  } catch (e) {
    error.set(e instanceof Error ? e.message : 'Failed to load graph');
  } finally {
    loading.set(false);
  }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd web && npx svelte-check`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add web/src/lib/
git commit -m "feat: add API client and Graphology store"
```

---

### Task 12: Graph Component

**Files:**
- Create: `web/src/lib/components/Graph.svelte`
- Modify: `web/src/routes/+page.svelte`

- [ ] **Step 1: Create web/src/lib/components/Graph.svelte**

```svelte
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Sigma from 'sigma';
  import forceAtlas2 from 'graphology-layout-forceatlas2';
  import louvain from 'graphology-communities-louvain';
  import { graphInstance, selectedNode, loading } from '$lib/stores/graph';
  import type Graph from 'graphology';

  let container: HTMLDivElement;
  let sigma: Sigma | null = null;

  const COLORS = [
    '#e6194b', '#3cb44b', '#4363d8', '#f58231', '#911eb4',
    '#42d4f4', '#f032e6', '#bfef45', '#fabed4', '#469990',
  ];

  function initSigma(graph: Graph) {
    if (sigma) sigma.kill();

    // Run community detection for coloring
    louvain.assign(graph);

    // Assign colors by community
    graph.forEachNode((node, attrs) => {
      const community = (attrs as any).community || 0;
      graph.setNodeAttribute(node, 'color', COLORS[community % COLORS.length]);
    });

    // Run ForceAtlas2 layout
    forceAtlas2.assign(graph, { iterations: 100, settings: { gravity: 1 } });

    sigma = new Sigma(graph, container, {
      renderEdgeLabels: false,
      defaultEdgeType: 'line',
    });

    sigma.on('clickNode', ({ node }) => {
      selectedNode.set(node);
    });

    sigma.on('clickStage', () => {
      selectedNode.set(null);
    });
  }

  const unsubscribe = graphInstance.subscribe((graph) => {
    if (graph && container) initSigma(graph);
  });

  onMount(() => {
    const graph = $graphInstance;
    if (graph) initSigma(graph);
  });

  onDestroy(() => {
    unsubscribe();
    if (sigma) sigma.kill();
  });
</script>

<div class="graph-container" bind:this={container}>
  {#if $loading}
    <div class="loading">Loading graph...</div>
  {/if}
</div>

<style>
  .graph-container {
    width: 100%;
    height: 100%;
    position: relative;
  }
  .loading {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    color: #888;
  }
</style>
```

- [ ] **Step 2: Create initial route page web/src/routes/+page.svelte**

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import Graph from '$lib/components/Graph.svelte';
  import { loadGraph, selectedNode, graphData } from '$lib/stores/graph';

  onMount(() => {
    loadGraph();
  });
</script>

<div class="app">
  <header>
    <h1>postgraph</h1>
    <span class="stats">
      {#if $graphData}
        {$graphData.nodes.length} posts | {$graphData.edges.length} connections
      {/if}
    </span>
  </header>

  <main>
    <div class="graph-panel">
      <Graph />
    </div>

    {#if $selectedNode}
      <aside class="detail-panel">
        <h2>Post Detail</h2>
        <p>ID: {$selectedNode}</p>
        <!-- Post detail sidebar will be expanded later -->
      </aside>
    {/if}
  </main>
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: #0a0a0a;
    color: #eee;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid #333;
  }
  h1 { margin: 0; font-size: 1.2rem; }
  .stats { color: #888; font-size: 0.85rem; }
  main {
    display: flex;
    flex: 1;
    overflow: hidden;
  }
  .graph-panel {
    flex: 1;
  }
  .detail-panel {
    width: 320px;
    padding: 1rem;
    border-left: 1px solid #333;
    overflow-y: auto;
  }
</style>
```

- [ ] **Step 3: Verify it builds**

Run: `cd web && npm run build`
Expected: Builds successfully

- [ ] **Step 4: Commit**

```bash
git add web/src/
git commit -m "feat: add Sigma.js graph component with ForceAtlas2 layout"
```

---

## Chunk 5: Svelte Frontend — Analytics + FilterBar

### Task 13: FilterBar Component

**Files:**
- Create: `web/src/lib/components/FilterBar.svelte`
- Create: `web/src/lib/stores/filters.ts`

- [ ] **Step 1: Create web/src/lib/stores/filters.ts**

```typescript
import { writable, derived } from 'svelte/store';

export interface Filters {
  topics: string[];
  dateFrom: string | null;
  dateTo: string | null;
  minEngagement: number;
  edgeTypes: string[];
  searchQuery: string;
}

export const filters = writable<Filters>({
  topics: [],
  dateFrom: null,
  dateTo: null,
  minEngagement: 0,
  edgeTypes: ['topic_overlap', 'theme', 'reply_chain', 'temporal_proximity'],
  searchQuery: '',
});

export function resetFilters() {
  filters.set({
    topics: [],
    dateFrom: null,
    dateTo: null,
    minEngagement: 0,
    edgeTypes: ['topic_overlap', 'theme', 'reply_chain', 'temporal_proximity'],
    searchQuery: '',
  });
}
```

- [ ] **Step 2: Create web/src/lib/components/FilterBar.svelte**

```svelte
<script lang="ts">
  import { filters, resetFilters } from '$lib/stores/filters';
  import { graphData } from '$lib/stores/graph';

  let allTopics: string[] = [];
  $: if ($graphData) {
    const topicSet = new Set<string>();
    for (const node of $graphData.nodes) {
      for (const t of node.topics) topicSet.add(t);
    }
    allTopics = [...topicSet].sort();
  }

  function toggleTopic(topic: string) {
    filters.update(f => {
      const topics = f.topics.includes(topic)
        ? f.topics.filter(t => t !== topic)
        : [...f.topics, topic];
      return { ...f, topics };
    });
  }
</script>

<div class="filter-bar">
  <input
    type="text"
    placeholder="Search posts..."
    bind:value={$filters.searchQuery}
    class="search"
  />

  <div class="filter-group">
    <label>Min engagement</label>
    <input type="range" min="0" max="1000" bind:value={$filters.minEngagement} />
    <span>{$filters.minEngagement}</span>
  </div>

  <div class="filter-group">
    <label>From</label>
    <input type="date" bind:value={$filters.dateFrom} />
    <label>To</label>
    <input type="date" bind:value={$filters.dateTo} />
  </div>

  <div class="topics">
    {#each allTopics as topic}
      <button
        class="topic-chip"
        class:active={$filters.topics.includes(topic)}
        on:click={() => toggleTopic(topic)}
      >
        {topic}
      </button>
    {/each}
  </div>

  <button class="reset" on:click={resetFilters}>Reset</button>
</div>

<style>
  .filter-bar {
    display: flex;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid #333;
    flex-wrap: wrap;
    align-items: center;
  }
  .search {
    background: #1a1a1a;
    border: 1px solid #444;
    color: #eee;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
  }
  .filter-group {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.8rem;
  }
  .filter-group input[type="date"] {
    background: #1a1a1a;
    border: 1px solid #444;
    color: #eee;
    padding: 0.2rem;
    border-radius: 4px;
  }
  .topics {
    display: flex;
    gap: 0.3rem;
    flex-wrap: wrap;
  }
  .topic-chip {
    background: #222;
    border: 1px solid #555;
    color: #ccc;
    padding: 0.2rem 0.5rem;
    border-radius: 12px;
    cursor: pointer;
    font-size: 0.75rem;
  }
  .topic-chip.active {
    background: #4363d8;
    border-color: #4363d8;
    color: white;
  }
  .reset {
    background: #333;
    border: 1px solid #555;
    color: #ccc;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
    cursor: pointer;
  }
  label { color: #888; font-size: 0.8rem; }
</style>
```

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/stores/filters.ts web/src/lib/components/FilterBar.svelte
git commit -m "feat: add FilterBar component with topic, date, engagement filters"
```

---

### Task 14: Dashboard / Analytics View

**Files:**
- Create: `web/src/lib/components/Dashboard.svelte`
- Create: `web/src/routes/analytics/+page.svelte`
- Modify: `web/src/routes/+layout.svelte` (add navigation)

- [ ] **Step 1: Create web/src/lib/components/Dashboard.svelte**

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import Chart from 'chart.js/auto';
  import { api, type AnalyticsData } from '$lib/api';

  let analytics: AnalyticsData | null = null;
  let engagementCanvas: HTMLCanvasElement;
  let topicsCanvas: HTMLCanvasElement;
  let engagementChart: Chart | null = null;
  let topicsChart: Chart | null = null;

  onMount(async () => {
    analytics = await api.getAnalytics();
    if (!analytics) return;

    // Engagement over time
    engagementChart = new Chart(engagementCanvas, {
      type: 'line',
      data: {
        labels: analytics.engagement_over_time.map(e => e.date),
        datasets: [
          { label: 'Likes', data: analytics.engagement_over_time.map(e => e.likes), borderColor: '#e6194b' },
          { label: 'Replies', data: analytics.engagement_over_time.map(e => e.replies), borderColor: '#3cb44b' },
          { label: 'Reposts', data: analytics.engagement_over_time.map(e => e.reposts), borderColor: '#4363d8' },
        ],
      },
      options: {
        responsive: true,
        plugins: { legend: { labels: { color: '#ccc' } } },
        scales: {
          x: { ticks: { color: '#888' }, grid: { color: '#222' } },
          y: { ticks: { color: '#888' }, grid: { color: '#222' } },
        },
      },
    });

    // Topics breakdown
    topicsChart = new Chart(topicsCanvas, {
      type: 'bar',
      data: {
        labels: analytics.topics.map(t => t.name),
        datasets: [{
          label: 'Posts',
          data: analytics.topics.map(t => t.post_count),
          backgroundColor: '#4363d8',
        }],
      },
      options: {
        responsive: true,
        indexAxis: 'y',
        plugins: { legend: { display: false } },
        scales: {
          x: { ticks: { color: '#888' }, grid: { color: '#222' } },
          y: { ticks: { color: '#ccc' }, grid: { color: '#222' } },
        },
      },
    });
  });
</script>

<div class="dashboard">
  {#if analytics}
    <div class="stats-row">
      <div class="stat">
        <span class="value">{analytics.total_posts}</span>
        <span class="label">Total Posts</span>
      </div>
      <div class="stat">
        <span class="value">{analytics.analyzed_posts}</span>
        <span class="label">Analyzed</span>
      </div>
      <div class="stat">
        <span class="value">{analytics.total_topics}</span>
        <span class="label">Topics</span>
      </div>
    </div>

    <div class="charts">
      <div class="chart-card">
        <h3>Engagement Over Time</h3>
        <canvas bind:this={engagementCanvas}></canvas>
      </div>
      <div class="chart-card">
        <h3>Topics Breakdown</h3>
        <canvas bind:this={topicsCanvas}></canvas>
      </div>
    </div>
  {:else}
    <p>Loading analytics...</p>
  {/if}
</div>

<style>
  .dashboard { padding: 1rem; }
  .stats-row { display: flex; gap: 2rem; margin-bottom: 1.5rem; }
  .stat { text-align: center; }
  .value { display: block; font-size: 2rem; font-weight: bold; }
  .label { color: #888; font-size: 0.85rem; }
  .charts { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }
  .chart-card {
    background: #111;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 1rem;
  }
  h3 { margin: 0 0 0.5rem; font-size: 1rem; }
</style>
```

- [ ] **Step 2: Create web/src/routes/analytics/+page.svelte**

```svelte
<script lang="ts">
  import Dashboard from '$lib/components/Dashboard.svelte';
</script>

<Dashboard />
```

- [ ] **Step 3: Create web/src/routes/+layout.svelte with navigation**

```svelte
<script lang="ts">
  import { page } from '$app/stores';
</script>

<div class="layout">
  <nav>
    <a href="/" class:active={$page.url.pathname === '/'}>Graph</a>
    <a href="/analytics" class:active={$page.url.pathname === '/analytics'}>Analytics</a>
  </nav>
  <div class="content">
    <slot />
  </div>
</div>

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    background: #0a0a0a;
    color: #eee;
  }
  .layout { display: flex; flex-direction: column; height: 100vh; }
  nav {
    display: flex;
    gap: 1rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid #333;
  }
  nav a {
    color: #888;
    text-decoration: none;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
  }
  nav a.active { color: #fff; background: #333; }
  .content { flex: 1; overflow: hidden; }
</style>
```

- [ ] **Step 4: Verify it builds**

Run: `cd web && npm run build`
Expected: Builds successfully

- [ ] **Step 5: Commit**

```bash
git add web/src/
git commit -m "feat: add analytics dashboard with Chart.js and navigation layout"
```

---

## Chunk 6: Polish + CLAUDE.md

### Task 15: Update +page.svelte with FilterBar

**Files:**
- Modify: `web/src/routes/+page.svelte`

- [ ] **Step 1: Update +page.svelte to include FilterBar**

Add FilterBar import and component above the graph:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import Graph from '$lib/components/Graph.svelte';
  import FilterBar from '$lib/components/FilterBar.svelte';
  import { loadGraph, selectedNode, graphData } from '$lib/stores/graph';

  onMount(() => {
    loadGraph();
  });
</script>

<div class="app">
  <FilterBar />
  <main>
    <div class="graph-panel">
      <Graph />
    </div>
    {#if $selectedNode}
      <aside class="detail-panel">
        <h2>Post Detail</h2>
        <p>ID: {$selectedNode}</p>
      </aside>
    {/if}
  </main>
</div>

<style>
  .app { display: flex; flex-direction: column; height: 100%; }
  main { display: flex; flex: 1; overflow: hidden; }
  .graph-panel { flex: 1; }
  .detail-panel {
    width: 320px;
    padding: 1rem;
    border-left: 1px solid #333;
    overflow-y: auto;
  }
</style>
```

- [ ] **Step 2: Commit**

```bash
git add web/src/routes/+page.svelte
git commit -m "feat: integrate FilterBar into graph view"
```

---

### Task 16: CLAUDE.md

**Files:**
- Create: `CLAUDE.md`

- [ ] **Step 1: Create CLAUDE.md**

```markdown
# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**postgraph** is a Threads analytics platform. Rust backend syncs posts from the Threads API, analyzes them with Mercury LLM (Inception Labs), computes a topic-based relationship graph, and serves data to a Svelte dashboard with Sigma.js graph visualization.

## Architecture

- **postgraph-server/** — Rust API server (Shuttle + axum + sqlx)
- **web/** — Svelte frontend (SvelteKit, deployed to Netlify)
- **Shuttle Postgres** — provisioned automatically by Shuttle

## Build Commands

### Rust backend
```bash
cargo check --workspace              # Quick compile check
cargo build --workspace              # Full build
cargo fmt --all                       # Format
cargo clippy --workspace --all-targets # Lint
cargo test --workspace                # Tests
```

### Svelte frontend
```bash
cd web && npm install                 # Install deps
cd web && npm run dev                 # Dev server
cd web && npm run build               # Production build
cd web && npx svelte-check            # Type check
```

### Running locally
```bash
# Backend (requires Shuttle CLI: cargo install cargo-shuttle)
cargo shuttle run

# Frontend (separate terminal)
cd web && npm run dev
```

## Environment Variables

See `.env.example`. Required:
- `THREADS_ACCESS_TOKEN` — Threads API long-lived token
- `MERCURY_API_KEY` — Inception Labs API key
- `MERCURY_API_URL` — Mercury endpoint (default: https://api.inceptionlabs.ai/v1)
- `POSTGRAPH_API_KEY` — API key for frontend-to-backend auth

## Key Patterns

- Threads API client in `threads.rs` — follows ndl's reqwest patterns, access_token as query param
- Mercury client in `mercury.rs` — OpenAI-compatible API (POST /chat/completions)
- Background sync runs every 15 min via tokio::time::interval in main.rs
- Edges are pre-computed server-side, not in the frontend
- Routes split by domain: `routes/graph.rs`, `routes/posts.rs`, `routes/analytics.rs`, `routes/sync.rs`

## Pre-commit Checklist

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo check --workspace
cd web && npx svelte-check
```

## Code Conventions

- Edition 2024 Rust
- thiserror for custom error types
- Async throughout (tokio runtime)
- No unwrap in production paths
- Modules are single files (no mod/ directories, except routes/)

## Database

Migrations in `postgraph-server/migrations/`. Run automatically on startup via `sqlx::migrate!()`.

## Reference

- Design spec: `docs/superpowers/specs/2026-03-11-postgraph-design.md`
- ndl reference project: github.com/pgray/ndl (Threads API patterns)
- Threads API base: `https://graph.threads.net/v1.0/`
- Mercury API: OpenAI-compatible at configured endpoint
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add CLAUDE.md with project guidance"
```

---

### Task 17: Final Verification

- [ ] **Step 1: Full backend compile check**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 2: Full frontend build**

Run: `cd web && npm run build`
Expected: Builds successfully

- [ ] **Step 3: Format and lint**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets`
Expected: No warnings or errors

- [ ] **Step 4: Final commit if any formatting changes**

```bash
git add -A && git commit -m "chore: format and lint cleanup"
```
