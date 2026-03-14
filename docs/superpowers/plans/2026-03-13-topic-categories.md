# Topic Categories Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Mercury LLM-discovered categories to the flat topic system, providing stable graph coloring, category filtering, and reduced visual noise.

**Architecture:** A new `categories` table with a FK on `topics.category_id`. Mercury groups all topics into categories via a dedicated API call. The frontend replaces Louvain community detection with server-provided category colors. A category filter in the FilterBar filters both graphs.

**Tech Stack:** Rust (axum, sqlx, tokio), Svelte (SvelteKit), Sigma.js, Mercury LLM (OpenAI-compatible API)

**Spec:** `docs/superpowers/specs/2026-03-13-topic-categories-design.md`

---

## File Structure

### New files
- `postgraph-server/migrations/005_add_categories.sql` — schema migration
- `postgraph-server/src/routes/categorize.rs` — categorize endpoints
- `web/src/routes/api/categorize/+server.ts` — SvelteKit proxy for POST /api/categorize
- `web/src/routes/api/categorize/status/+server.ts` — SvelteKit proxy for GET /api/categorize/status
- `web/src/routes/api/categories/+server.ts` — SvelteKit proxy for GET /api/categories

### Modified files
- `postgraph-server/src/types.rs` — add `Category` type
- `postgraph-server/src/db.rs` — add category CRUD functions
- `postgraph-server/src/mercury.rs` — add `categorize_topics()` and `assign_topic_category()`
- `postgraph-server/src/analysis.rs` — call incremental category assignment for new topics
- `postgraph-server/src/state.rs` — add categorize progress fields to `AppState`
- `postgraph-server/src/main.rs` — register new routes, initialize new state fields
- `postgraph-server/src/routes/mod.rs` — declare `categorize` module
- `postgraph-server/src/routes/graph.rs` — add category data to both graph responses, add category filter param
- `postgraph-server/src/routes/reanalyze.rs` — auto-trigger categorization after reanalysis
- `postgraph-server/src/routes/analyze.rs` — auto-trigger categorization after first analysis
- `web/src/lib/api.ts` — add category types and API methods
- `web/src/lib/stores/filters.ts` — add `category` field
- `web/src/lib/components/TagGraph.svelte` — replace Louvain with category colors, add legend
- `web/src/lib/components/Graph.svelte` — replace Louvain with category colors, add legend, add category filter
- `web/src/lib/components/FilterBar.svelte` — add category dropdown and recategorize button
- `web/src/lib/components/Dashboard.svelte` — color topic bars by category

---

## Chunk 1: Backend Database & Types

### Task 1: Database Migration

**Files:**
- Create: `postgraph-server/migrations/005_add_categories.sql`

- [ ] **Step 1: Create migration file**

```sql
-- 005_add_categories.sql
CREATE TABLE categories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    color TEXT
);

ALTER TABLE topics ADD COLUMN category_id UUID REFERENCES categories(id) ON DELETE SET NULL;
```

- [ ] **Step 2: Verify migration compiles**

Run: `cd postgraph-server && cargo check`
Expected: compiles (migration runs at startup, so just check no Rust errors)

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/migrations/005_add_categories.sql
git commit -m "feat: add categories table and topics.category_id migration"
```

### Task 2: Rust Types

**Files:**
- Modify: `postgraph-server/src/types.rs`

- [ ] **Step 1: Add `category_id` to Topic struct and add Category struct**

Update the `Topic` struct (line 24-29) to include the new column:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Topic {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub category_id: Option<Uuid>,
}
```

Add after `Topic` (line 29):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Category {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/types.rs
git commit -m "feat: add Category type"
```

### Task 3: Database Functions

**Files:**
- Modify: `postgraph-server/src/db.rs`

- [ ] **Step 1: Add color palette constant and category DB functions**

Add after the `use` statements at the top of `db.rs`:

```rust
pub const CATEGORY_COLORS: &[&str] = &[
    "#e6194b", "#3cb44b", "#4363d8", "#f58231", "#911eb4",
    "#42d4f4", "#f032e6", "#bfef45", "#fabed4", "#469990",
    "#dcbeff", "#9A6324", "#800000", "#aaffc3", "#808000",
];
```

Add after the `get_all_topics` function (line 135):

```rust
// -- Categories --

pub async fn get_all_categories(pool: &PgPool) -> sqlx::Result<Vec<Category>> {
    sqlx::query_as::<_, Category>("SELECT * FROM categories ORDER BY name")
        .fetch_all(pool)
        .await
}

pub async fn upsert_category(
    pool: &PgPool,
    name: &str,
    description: &str,
    color: &str,
) -> sqlx::Result<Category> {
    sqlx::query_as::<_, Category>(
        r#"INSERT INTO categories (name, description, color)
           VALUES ($1, $2, $3)
           ON CONFLICT (name) DO UPDATE SET description = EXCLUDED.description
           RETURNING *"#,
    )
    .bind(name)
    .bind(description)
    .bind(color)
    .fetch_one(pool)
    .await
}

pub async fn set_topic_category(
    pool: &PgPool,
    topic_name: &str,
    category_id: uuid::Uuid,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE topics SET category_id = $1 WHERE name = $2")
        .bind(category_id)
        .bind(topic_name)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_orphaned_categories(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM categories WHERE id NOT IN (SELECT DISTINCT category_id FROM topics WHERE category_id IS NOT NULL)",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn get_categories_with_topics(
    pool: &PgPool,
) -> sqlx::Result<Vec<(Category, Vec<String>)>> {
    let categories = get_all_categories(pool).await?;
    let mut result = Vec::new();
    for cat in categories {
        let topic_names: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM topics WHERE category_id = $1 ORDER BY name",
        )
        .bind(cat.id)
        .fetch_all(pool)
        .await?;
        let names: Vec<String> = topic_names.into_iter().map(|(n,)| n).collect();
        result.push((cat, names));
    }
    Ok(result)
}
```

- [ ] **Step 2: Add `use crate::types::Category` if not already covered by the wildcard import**

The file already has `use crate::types::*;` on line 1, so `Category` is already in scope.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles (migration hasn't run yet against a live DB, but types check out)

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/db.rs
git commit -m "feat: add category database functions"
```

### Task 4: AppState Fields

**Files:**
- Modify: `postgraph-server/src/state.rs`

- [ ] **Step 1: Add categorize progress fields**

Add after `sync_total` (line 20):

```rust
    pub categorize_running: Arc<AtomicBool>,
    pub categorize_progress: Arc<AtomicU32>,
    pub categorize_total: Arc<AtomicU32>,
```

- [ ] **Step 2: Update AppState initialization in main.rs**

In `postgraph-server/src/main.rs`, find the `AppState { ... }` initialization block and add the new fields:

```rust
        categorize_running: Arc::new(AtomicBool::new(false)),
        categorize_progress: Arc::new(AtomicU32::new(0)),
        categorize_total: Arc::new(AtomicU32::new(0)),
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/state.rs postgraph-server/src/main.rs
git commit -m "feat: add categorize progress fields to AppState"
```

---

## Chunk 2: Mercury LLM Integration

### Task 5: Mercury Categorization Functions

**Files:**
- Modify: `postgraph-server/src/mercury.rs`

- [ ] **Step 1: Add response types for categorization**

Add after `AnalysisResponse` (line 56):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryGroup {
    pub name: String,
    pub description: String,
    pub topics: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CategorizeResponse {
    pub categories: Vec<CategoryGroup>,
}

#[derive(Debug, Deserialize)]
pub struct AssignCategoryResponse {
    pub category: String,
}
```

- [ ] **Step 2: Add `categorize_topics` method to MercuryClient**

Add after the `analyze_posts` method (after line 171):

```rust
    pub async fn categorize_topics(
        &self,
        topics: &[(String, String)], // (name, description)
    ) -> Result<CategorizeResponse, AppError> {
        let topics_json: Vec<serde_json::Value> = topics
            .iter()
            .map(|(name, desc)| serde_json::json!({"name": name, "description": desc}))
            .collect();
        let topics_str = serde_json::to_string_pretty(&topics_json).unwrap_or_default();

        let prompt = format!(
            r#"Group these topics into broad categories. Each topic should belong to exactly one category. Let the number of categories emerge naturally from the data.

Topics:
{topics_str}

Respond with ONLY valid JSON in this exact format:
{{
  "categories": [
    {{
      "name": "Category Name",
      "description": "Brief description of what this category covers",
      "topics": ["Topic A", "Topic B"]
    }}
  ]
}}"#
        );

        let request = ChatRequest {
            model: "mercury-2".to_string(),
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

        let json_str = content
            .trim()
            .strip_prefix("```json")
            .or_else(|| content.trim().strip_prefix("```"))
            .unwrap_or(content.trim())
            .strip_suffix("```")
            .unwrap_or(content.trim())
            .trim();

        let result: CategorizeResponse = serde_json::from_str(json_str).map_err(|e| {
            AppError::MercuryApi(format!("Failed to parse categorize response: {e}. Raw: {json_str}"))
        })?;

        Ok(result)
    }

    pub async fn assign_topic_category(
        &self,
        topic_name: &str,
        categories: &[(String, String)], // (name, description)
    ) -> Result<AssignCategoryResponse, AppError> {
        let cats_json: Vec<serde_json::Value> = categories
            .iter()
            .map(|(name, desc)| serde_json::json!({"name": name, "description": desc}))
            .collect();
        let cats_str = serde_json::to_string(&cats_json).unwrap_or_default();

        let prompt = format!(
            r#"Given the topic "{topic_name}" and these existing categories: {cats_str}, which category does this topic belong to? Return ONLY valid JSON: {{"category": "<category_name>"}}"#
        );

        let request = ChatRequest {
            model: "mercury-2".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
            }],
            temperature: 0.1,
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

        let json_str = content
            .trim()
            .strip_prefix("```json")
            .or_else(|| content.trim().strip_prefix("```"))
            .unwrap_or(content.trim())
            .strip_suffix("```")
            .unwrap_or(content.trim())
            .trim();

        let result: AssignCategoryResponse = serde_json::from_str(json_str).map_err(|e| {
            AppError::MercuryApi(format!("Failed to parse assign response: {e}. Raw: {json_str}"))
        })?;

        Ok(result)
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/mercury.rs
git commit -m "feat: add Mercury categorize_topics and assign_topic_category"
```

### Task 6: Categorize Route

**Files:**
- Create: `postgraph-server/src/routes/categorize.rs`
- Modify: `postgraph-server/src/routes/mod.rs`
- Modify: `postgraph-server/src/main.rs`

- [ ] **Step 1: Create routes/categorize.rs**

```rust
use axum::{Json, extract::State};
use serde::Serialize;
use std::sync::atomic::Ordering;
use tracing::info;

use crate::db;
use crate::state::AppState;

#[derive(Serialize)]
pub struct CategorizeStartResult {
    pub started: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct CategorizeStatus {
    pub running: bool,
    pub progress: u32,
    pub total: u32,
}

#[derive(Serialize)]
pub struct CategoryWithTopics {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub topics: Vec<String>,
}

#[derive(Serialize)]
pub struct CategoriesResponse {
    pub categories: Vec<CategoryWithTopics>,
}

pub async fn start_categorize(
    State(state): State<AppState>,
) -> Result<Json<CategorizeStartResult>, (axum::http::StatusCode, Json<CategorizeStartResult>)> {
    // Block if analysis is running — return HTTP 409
    if state.analysis_running.load(Ordering::SeqCst) {
        return Err((axum::http::StatusCode::CONFLICT, Json(CategorizeStartResult {
            started: false,
            message: "Analysis in progress, try again after it completes".to_string(),
        })));
    }

    if state.categorize_running.swap(true, Ordering::SeqCst) {
        return Ok(Json(CategorizeStartResult {
            started: false,
            message: "Categorization already in progress".to_string(),
        }));
    }

    let topic_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM topics")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| {
                state.categorize_running.store(false, Ordering::SeqCst);
                tracing::error!("Failed to count topics: {e}");
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            })?;

    state.categorize_progress.store(0, Ordering::SeqCst);
    state.categorize_total.store(topic_count as u32, Ordering::SeqCst);

    info!("Starting background categorization of {topic_count} topics");

    let bg_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = run_full_categorization(&bg_state).await {
            tracing::error!("Categorization failed: {e}");
        }
        bg_state.categorize_running.store(false, Ordering::SeqCst);
        info!("Background categorization finished");
    });

    Ok(Json(CategorizeStartResult {
        started: true,
        message: format!("{topic_count} topics queued for categorization"),
    }))
}

pub async fn run_full_categorization(
    state: &AppState,
) -> Result<(), crate::error::AppError> {
    let topics = db::get_all_topics(&state.pool).await?;
    if topics.is_empty() {
        return Ok(());
    }

    let topic_pairs: Vec<(String, String)> = topics
        .iter()
        .map(|t| (t.name.clone(), t.description.clone().unwrap_or_default()))
        .collect();

    let result = state.mercury.categorize_topics(&topic_pairs).await?;

    // Get existing categories to preserve colors
    let existing_cats = db::get_all_categories(&state.pool).await?;
    let existing_color_map: std::collections::HashMap<String, String> = existing_cats
        .iter()
        .filter_map(|c| c.color.as_ref().map(|color| (c.name.clone(), color.clone())))
        .collect();

    let mut color_idx = 0;
    let mut used_colors: std::collections::HashSet<String> = existing_color_map.values().cloned().collect();

    let mut progress: u32 = 0;

    for group in &result.categories {
        // Preserve existing color or assign next unused one
        let color = if let Some(existing) = existing_color_map.get(&group.name) {
            existing.clone()
        } else {
            // Find next unused color from palette
            loop {
                let c = db::CATEGORY_COLORS[color_idx % db::CATEGORY_COLORS.len()].to_string();
                color_idx += 1;
                if !used_colors.contains(&c) || color_idx > db::CATEGORY_COLORS.len() {
                    used_colors.insert(c.clone());
                    break c;
                }
            }
        };

        let category = db::upsert_category(&state.pool, &group.name, &group.description, &color).await?;

        for topic_name in &group.topics {
            db::set_topic_category(&state.pool, topic_name, category.id).await?;
            progress += 1;
            state.categorize_progress.store(progress, Ordering::SeqCst);
        }
    }

    // Delete orphaned categories
    let deleted = db::delete_orphaned_categories(&state.pool).await?;
    if deleted > 0 {
        info!("Deleted {deleted} orphaned categories");
    }

    Ok(())
}

pub async fn categorize_status(State(state): State<AppState>) -> Json<CategorizeStatus> {
    Json(CategorizeStatus {
        running: state.categorize_running.load(Ordering::SeqCst),
        progress: state.categorize_progress.load(Ordering::SeqCst),
        total: state.categorize_total.load(Ordering::SeqCst),
    })
}

pub async fn list_categories(
    State(state): State<AppState>,
) -> Result<Json<CategoriesResponse>, axum::http::StatusCode> {
    let cats_with_topics = db::get_categories_with_topics(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let categories: Vec<CategoryWithTopics> = cats_with_topics
        .into_iter()
        .map(|(cat, topics)| CategoryWithTopics {
            id: cat.id.to_string(),
            name: cat.name,
            description: cat.description,
            color: cat.color,
            topics,
        })
        .collect();

    Ok(Json(CategoriesResponse { categories }))
}
```

- [ ] **Step 2: Add `categorize` module to routes/mod.rs**

Add to `postgraph-server/src/routes/mod.rs`:

```rust
pub mod categorize;
```

- [ ] **Step 3: Register routes in main.rs**

In `postgraph-server/src/main.rs`, add to the `api_routes` Router (after line 184):

```rust
        .route("/api/categorize", post(routes::categorize::start_categorize))
        .route("/api/categorize/status", get(routes::categorize::categorize_status))
        .route("/api/categories", get(routes::categorize::list_categories))
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/routes/categorize.rs postgraph-server/src/routes/mod.rs postgraph-server/src/main.rs
git commit -m "feat: add categorize endpoints"
```

### Task 7: Incremental Category Assignment in Analysis

**Files:**
- Modify: `postgraph-server/src/analysis.rs`

- [ ] **Step 1: Update run_analysis to accept MercuryClient by Arc and do incremental assignment**

Update the function signature and add incremental logic. After the `upsert_topic` call (line 51-52), check if the topic is new (no category) and assign one:

```rust
use crate::db;
use crate::error::AppError;
use crate::mercury::MercuryClient;
use sqlx::PgPool;
use tracing::{info, warn};

fn analysis_batch_size() -> i64 {
    std::env::var("ANALYSIS_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16)
}

pub async fn run_analysis(pool: &PgPool, mercury: &MercuryClient) -> Result<u32, AppError> {
    let unanalyzed = db::get_unanalyzed_posts(pool, analysis_batch_size()).await?;
    if unanalyzed.is_empty() {
        return Ok(0);
    }

    let existing_topics = db::get_all_topics(pool).await?;
    let topic_names: Vec<String> = existing_topics.iter().map(|t| t.name.clone()).collect();

    // Pre-load categories for incremental assignment
    let categories = db::get_all_categories(pool).await?;
    let cat_pairs: Vec<(String, String)> = categories
        .iter()
        .map(|c| (c.name.clone(), c.description.clone().unwrap_or_default()))
        .collect();

    let posts_for_llm: Vec<(String, String)> = unanalyzed
        .iter()
        .filter_map(|p| p.text.as_ref().map(|text| (p.id.clone(), text.clone())))
        .collect();

    if posts_for_llm.is_empty() {
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
        for topic_assignment in &analyzed.topics {
            let topic =
                db::upsert_topic(pool, &topic_assignment.name, &topic_assignment.description)
                    .await?;
            db::upsert_post_topic(pool, &analyzed.post_id, topic.id, topic_assignment.weight)
                .await?;

            // Incremental category assignment: if topic has no category and categories exist
            if !cat_pairs.is_empty() && topic.category_id.is_none() {
                {
                    match mercury.assign_topic_category(&topic_assignment.name, &cat_pairs).await {
                        Ok(resp) => {
                            // Find matching category
                            if let Some(cat) = categories.iter().find(|c| c.name == resp.category) {
                                let _ = db::set_topic_category(pool, &topic_assignment.name, cat.id).await;
                            }
                        }
                        Err(e) => {
                            warn!("Incremental category assignment failed for '{}': {e}", topic_assignment.name);
                            // Non-fatal: topic stays uncategorized
                        }
                    }
                }
            }
        }

        db::mark_post_analyzed(pool, &analyzed.post_id, analyzed.sentiment).await?;
        analyzed_count += 1;
    }

    info!("Analyzed {} posts", analyzed_count);
    Ok(analyzed_count)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/analysis.rs
git commit -m "feat: add incremental category assignment during analysis"
```

### Task 8: Auto-categorize After Reanalysis

**Files:**
- Modify: `postgraph-server/src/routes/reanalyze.rs`

- [ ] **Step 1: Add auto-categorization after reanalysis completes**

In the background task within `trigger_reanalyze`, **after** setting `analysis_running` to false (after edge computation), add:

```rust
        // Set analysis_running = false FIRST so categorize doesn't conflict
        bg_state.analysis_running.store(false, Ordering::SeqCst);

        // Auto-trigger categorization after reanalysis
        info!("Reanalysis complete, running categorization...");
        bg_state.categorize_running.store(true, Ordering::SeqCst);
        if let Err(e) = crate::routes::categorize::run_full_categorization(&bg_state).await {
            tracing::error!("Auto-categorization after reanalysis failed: {e}");
        }
        bg_state.categorize_running.store(false, Ordering::SeqCst);
```

**Important:** Remove the original `bg_state.analysis_running.store(false, ...)` line that was before this block, since we now set it explicitly above.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/routes/reanalyze.rs
git commit -m "feat: auto-trigger categorization after reanalysis"
```

### Task 8b: Auto-categorize After First Analysis

**Files:**
- Modify: `postgraph-server/src/routes/analyze.rs`

- [ ] **Step 1: Add auto-categorization after analysis if no categories exist**

In the `start_analyze` background task, after the edge computation loop and before setting `analysis_running` to false, add:

```rust
        // Auto-categorize if no categories exist yet
        let cat_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories")
            .fetch_one(&bg_state.pool)
            .await
            .unwrap_or(0);
        if cat_count == 0 && total_analyzed > 0 {
            info!("No categories exist, running auto-categorization...");
            bg_state.categorize_running.store(true, Ordering::SeqCst);
            if let Err(e) = crate::routes::categorize::run_full_categorization(&bg_state).await {
                tracing::error!("Auto-categorization failed: {e}");
            }
            bg_state.categorize_running.store(false, Ordering::SeqCst);
        }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/routes/analyze.rs
git commit -m "feat: auto-categorize after first analysis when no categories exist"
```

---

## Chunk 3: Graph API Modifications

### Task 9: Tag Graph with Category Data

**Files:**
- Modify: `postgraph-server/src/routes/graph.rs`

- [ ] **Step 1: Update TagGraphNode to include category fields**

Update the `TagGraphNode` struct (lines 33-39):

```rust
#[derive(Serialize)]
pub struct TagGraphNode {
    pub id: String,
    pub label: String,
    pub post_count: i32,
    pub total_engagement: i64,
    pub post_ids: Vec<String>,
    pub category_id: Option<String>,
    pub category_name: Option<String>,
    pub category_color: Option<String>,
}
```

- [ ] **Step 2: Update get_tag_graph query to join categories**

Update the SQL query in `get_tag_graph` (line 127-134) to join categories:

```rust
    let rows = sqlx::query_as::<_, (String, String, String, i64, Option<String>, Option<String>, Option<String>)>(
        r#"SELECT t.id::text, t.name, pt.post_id,
                  COALESCE(p.likes + p.replies_count + p.reposts + p.quotes, 0)::bigint AS engagement,
                  c.id::text AS category_id, c.name AS category_name, c.color AS category_color
           FROM topics t
           JOIN post_topics pt ON pt.topic_id = t.id
           JOIN posts p ON p.id = pt.post_id AND p.analyzed_at IS NOT NULL
           LEFT JOIN categories c ON t.category_id = c.id
           ORDER BY t.name"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
```

- [ ] **Step 3: Update topic_data map to include category info**

Update the map type and population (lines 140-155):

```rust
    // Build topic -> (name, post_ids, total_engagement, category_id, category_name, category_color)
    let mut topic_data: HashMap<String, (String, Vec<String>, i64, Option<String>, Option<String>, Option<String>)> = HashMap::new();
    let mut post_topics_map: HashMap<String, Vec<String>> = HashMap::new();

    for (topic_id, topic_name, post_id, engagement, cat_id, cat_name, cat_color) in &rows {
        let entry = topic_data
            .entry(topic_id.clone())
            .or_insert_with(|| (topic_name.clone(), Vec::new(), 0, cat_id.clone(), cat_name.clone(), cat_color.clone()));
        entry.1.push(post_id.clone());
        entry.2 += engagement;

        post_topics_map
            .entry(post_id.clone())
            .or_default()
            .push(topic_id.clone());
    }
```

- [ ] **Step 4: Update node construction**

Update the node building (lines 158-167):

```rust
    let nodes: Vec<TagGraphNode> = topic_data
        .iter()
        .map(|(topic_id, (name, post_ids, total_eng, cat_id, cat_name, cat_color))| TagGraphNode {
            id: topic_id.clone(),
            label: name.clone(),
            post_count: post_ids.len() as i32,
            total_engagement: *total_eng,
            post_ids: post_ids.clone(),
            category_id: cat_id.clone(),
            category_name: cat_name.clone(),
            category_color: cat_color.clone(),
        })
        .collect();
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles

- [ ] **Step 6: Commit**

```bash
git add postgraph-server/src/routes/graph.rs
git commit -m "feat: include category data in tag graph response"
```

### Task 10: Post Graph with Category Data and Filter

**Files:**
- Modify: `postgraph-server/src/routes/graph.rs`

- [ ] **Step 1: Add category fields to GraphNode**

Update `GraphNode` struct (lines 8-16):

```rust
#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub size: f32,
    pub sentiment: Option<f32>,
    pub topics: Vec<String>,
    pub timestamp: Option<String>,
    pub engagement: i32,
    pub category: Option<NodeCategory>,
}

#[derive(Serialize, Clone)]
pub struct NodeCategory {
    pub name: String,
    pub color: String,
}
```

- [ ] **Step 2: Add query params struct for get_graph**

Add before `get_graph` function:

```rust
#[derive(serde::Deserialize)]
pub struct GraphQuery {
    pub category: Option<String>,
}
```

- [ ] **Step 3: Update get_graph to compute dominant category and support filtering**

Update the function signature and query to include category data:

```rust
pub async fn get_graph(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<GraphQuery>,
) -> Result<Json<GraphData>, axum::http::StatusCode> {
    // Run queries in parallel — now also fetch topic weights with category info
    let (posts_result, edges_result, topics_result) = tokio::join!(
        db::get_posts_for_graph(&state.pool),
        db::get_all_edges(&state.pool),
        sqlx::query_as::<_, (String, String, f32, Option<String>, Option<String>)>(
            r#"SELECT pt.post_id, t.name, pt.weight, c.name AS cat_name, c.color AS cat_color
               FROM post_topics pt
               JOIN topics t ON pt.topic_id = t.id
               LEFT JOIN categories c ON t.category_id = c.id"#,
        )
        .fetch_all(&state.pool),
    );

    let posts = posts_result.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let edges = edges_result.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let all_post_topics =
        topics_result.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build topic map and category weight map per post
    let mut topic_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut category_weights: HashMap<String, HashMap<String, (f32, String)>> = HashMap::new(); // post_id -> cat_name -> (total_weight, color)

    for (pid, name, weight, cat_name, cat_color) in &all_post_topics {
        topic_map
            .entry(pid.clone())
            .or_default()
            .push(name.clone());

        if let (Some(cn), Some(cc)) = (cat_name, cat_color) {
            let entry = category_weights
                .entry(pid.clone())
                .or_default()
                .entry(cn.clone())
                .or_insert((0.0, cc.clone()));
            entry.0 += weight;
        }
    }

    let nodes: Vec<GraphNode> = posts
        .iter()
        .filter(|p| p.analyzed_at.is_some())
        .filter_map(|p| {
            let topics: Vec<String> = topic_map
                .get(&p.id)
                .cloned()
                .unwrap_or_default();

            // Compute dominant category
            let dominant_category = category_weights
                .get(&p.id)
                .and_then(|cats| {
                    cats.iter()
                        .max_by(|a, b| a.1.0.partial_cmp(&b.1.0).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|(name, (_, color))| NodeCategory {
                            name: name.clone(),
                            color: color.clone(),
                        })
                });

            // Apply category filter
            if let Some(ref filter_cat) = query.category {
                match &dominant_category {
                    Some(cat) if &cat.name == filter_cat => {}
                    _ => return None,
                }
            }

            let engagement = (p.likes + p.replies_count + p.reposts + p.quotes) as f32;
            let size = (engagement + 1.0).ln().max(0.0) + 1.0;

            Some(GraphNode {
                id: p.id.clone(),
                label: p.text_preview.clone().unwrap_or_default(),
                size,
                sentiment: p.sentiment,
                topics,
                timestamp: Some(p.timestamp.format("%Y-%m-%d").to_string()),
                engagement: p.likes + p.replies_count + p.reposts + p.quotes,
                category: dominant_category,
            })
        })
        .collect();

    // Collect valid node IDs for edge filtering
    let node_ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();

    let graph_edges: Vec<GraphEdge> = edges
        .iter()
        .filter(|e| node_ids.contains(e.source_post_id.as_str()) && node_ids.contains(e.target_post_id.as_str()))
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

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --workspace`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/routes/graph.rs
git commit -m "feat: add dominant category and category filter to post graph"
```

- [ ] **Step 6: Run cargo fmt and clippy**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets`
Expected: no errors

- [ ] **Step 7: Commit if formatting changes**

```bash
git add -A && git commit -m "chore: format and lint"
```

---

## Chunk 4: Frontend — API & Stores

### Task 11: Frontend API Types and Methods

**Files:**
- Modify: `web/src/lib/api.ts`

- [ ] **Step 1: Add category-related types**

Add after `TagGraphData` interface (line 139):

```typescript
export interface CategoryData {
  id: string;
  name: string;
  description: string | null;
  color: string | null;
  topics: string[];
}

export interface CategoriesResponse {
  categories: CategoryData[];
}

export interface CategorizeStartResult {
  started: boolean;
  message: string;
}

export interface CategorizeStatus {
  running: boolean;
  progress: number;
  total: number;
}
```

Update `TagGraphNode` interface (lines 121-127) to add category fields:

```typescript
export interface TagGraphNode {
  id: string;
  label: string;
  post_count: number;
  total_engagement: number;
  post_ids: string[];
  category_id: string | null;
  category_name: string | null;
  category_color: string | null;
}
```

Update `GraphNode` interface (lines 9-17) to add category:

```typescript
export interface GraphNode {
  id: string;
  label: string;
  size: number;
  sentiment: number | null;
  topics: string[];
  timestamp: string | null;
  engagement: number;
  category: { name: string; color: string } | null;
}
```

- [ ] **Step 2: Add API methods**

Add to the `api` object (after `getAnalyzeStatus`, line 175):

```typescript
  getCategories: () => fetchApi<CategoriesResponse>('/api/categories'),
  triggerCategorize: () => fetch('/api/categorize', {
    method: 'POST',
  }).then(async r => {
    if (!r.ok) {
      const body = await r.json().catch(() => ({ error: `Categorize failed (${r.status})` }));
      throw new Error(body.error ?? `Categorize failed (${r.status})`);
    }
    return r.json() as Promise<CategorizeStartResult>;
  }),
  getCategorizeStatus: () => fetchApi<CategorizeStatus>('/api/categorize/status'),
```

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/api.ts
git commit -m "feat: add category types and API methods to frontend"
```

### Task 12: SvelteKit API Proxies

**Files:**
- Create: `web/src/routes/api/categorize/+server.ts`
- Create: `web/src/routes/api/categorize/status/+server.ts`
- Create: `web/src/routes/api/categories/+server.ts`

- [ ] **Step 1: Create POST /api/categorize proxy**

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async () => {
  return proxyToBackend('/api/categorize', { method: 'POST' });
};
```

- [ ] **Step 2: Create GET /api/categorize/status proxy**

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  return proxyToBackend('/api/categorize/status');
};
```

- [ ] **Step 3: Create GET /api/categories proxy**

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  return proxyToBackend('/api/categories');
};
```

- [ ] **Step 4: Commit**

```bash
git add web/src/routes/api/categorize/ web/src/routes/api/categories/
git commit -m "feat: add SvelteKit proxy routes for categories"
```

### Task 13: Filters Store Update

**Files:**
- Modify: `web/src/lib/stores/filters.ts`

- [ ] **Step 1: Add category field to Filters interface and defaults**

Update the `Filters` interface (lines 3-10):

```typescript
export interface Filters {
  topics: string[];
  category: string | null;
  dateFrom: string | null;
  dateTo: string | null;
  minEngagement: number;
  edgeTypes: string[];
  searchQuery: string;
}
```

Update the default value (lines 12-19):

```typescript
export const filters = writable<Filters>({
  topics: [],
  category: null,
  dateFrom: null,
  dateTo: null,
  minEngagement: 0,
  edgeTypes: ['topic_overlap', 'theme', 'reply_chain', 'temporal_proximity'],
  searchQuery: '',
});
```

Update `resetFilters` (lines 21-30):

```typescript
export function resetFilters() {
  filters.set({
    topics: [],
    category: null,
    dateFrom: null,
    dateTo: null,
    minEngagement: 0,
    edgeTypes: ['topic_overlap', 'theme', 'reply_chain', 'temporal_proximity'],
    searchQuery: '',
  });
}
```

- [ ] **Step 2: Commit**

```bash
git add web/src/lib/stores/filters.ts
git commit -m "feat: add category field to filters store"
```

---

## Chunk 5: Frontend — Graph Components

### Task 14: TagGraph with Category Coloring

**Files:**
- Modify: `web/src/lib/components/TagGraph.svelte`

- [ ] **Step 1: Replace Louvain coloring with category-based coloring**

Key changes to `TagGraph.svelte`:

1. Remove the `COLORS` array and `graphology-communities-louvain` import
2. In `initSigma()`, replace `louvain.assign(graph)` with category-based coloring:
   - For each node, read `category_color` from the node's attributes
   - If null, use `#888` (uncategorized gray)
3. Add the category legend below the graph container

The node attributes are set in `tagGraph.ts` store when building the graph from API data. The `TagGraphNode` now has `category_color`. When adding nodes to the graphology graph in `tagGraph.ts`, include `category_color` as a node attribute.

Update `web/src/lib/stores/tagGraph.ts` to pass category_color when creating nodes:

In the `addNode` call, add the `category_color` attribute:

```typescript
graph.addNode(node.id, {
  label: node.label,
  size: Math.log(node.post_count + 1) * 3 + 2,
  x: Math.random() * 100,
  y: Math.random() * 100,
  category_color: node.category_color,
  category_name: node.category_name,
});
```

In `TagGraph.svelte`, replace Louvain coloring with:

```typescript
// Color nodes by category instead of Louvain
graph.forEachNode((node, attrs) => {
  graph.setNodeAttribute(node, 'color', attrs.category_color || '#888');
});
```

Remove: `import louvain from 'graphology-communities-louvain';` and the `louvain.assign(graph)` call.

Add a category legend component after the Sigma container div. Extract unique categories from graph nodes:

```svelte
{#if categories.length > 0}
  <div class="category-legend">
    {#each categories as cat}
      <button class="legend-item" onclick={() => onCategoryClick(cat.name)}>
        <span class="legend-dot" style="background: {cat.color}"></span>
        {cat.name}
      </button>
    {/each}
  </div>
{/if}
```

- [ ] **Step 2: Commit**

```bash
git add web/src/lib/components/TagGraph.svelte web/src/lib/stores/tagGraph.ts
git commit -m "feat: replace Louvain with category coloring in tag graph"
```

### Task 15: Post Graph with Category Coloring

**Files:**
- Modify: `web/src/lib/components/Graph.svelte`
- Modify: `web/src/lib/stores/graph.ts`

- [ ] **Step 1: Update graph store to include category data**

In `web/src/lib/stores/graph.ts`, when adding nodes, include the `category` data:

```typescript
graph.addNode(node.id, {
  label: node.label,
  size: node.size,
  x: Math.random() * 100,
  y: Math.random() * 100,
  category_name: node.category?.name || null,
  category_color: node.category?.color || null,
});
```

- [ ] **Step 2: Update Graph.svelte to use category colors**

Replace Louvain coloring with category-based coloring (same pattern as TagGraph):

```typescript
// Color nodes by dominant category instead of Louvain
graph.forEachNode((node, attrs) => {
  graph.setNodeAttribute(node, 'color', attrs.category_color || '#888');
});
```

Remove the Louvain import and `louvain.assign(graph)` call.

- [ ] **Step 3: Update currentFilters initialization**

In `Graph.svelte`, update the hardcoded `currentFilters` initialization (line 66-73) to include the new `category` field:

```typescript
let currentFilters: Filters = {
    topics: [],
    category: null,
    dateFrom: null,
    dateTo: null,
    minEngagement: 0,
    edgeTypes: [],
    searchQuery: '',
};
```

- [ ] **Step 4: Update nodeMatchesFilters to check category**

In the `nodeMatchesFilters` function, add category check:

```typescript
// Category filter
if ($filters.category && attrs.category_name !== $filters.category) {
  return false;
}
```

- [ ] **Step 5: Add category legend (same pattern as TagGraph)**

- [ ] **Step 6: Commit**

```bash
git add web/src/lib/components/Graph.svelte web/src/lib/stores/graph.ts
git commit -m "feat: replace Louvain with category coloring in post graph"
```

### Task 16: FilterBar Category Dropdown and Recategorize Button

**Files:**
- Modify: `web/src/lib/components/FilterBar.svelte`

- [ ] **Step 1: Add category dropdown**

Add a category dropdown select element. Fetch categories from the API and populate the dropdown. When a category is selected, update `$filters.category`.

```svelte
<select bind:value={$filters.category}>
  <option value={null}>All Categories</option>
  {#each categoryList as cat}
    <option value={cat.name}>{cat.name}</option>
  {/each}
</select>
```

Load categories on mount:

```typescript
let categoryList: { name: string; color: string }[] = [];

onMount(async () => {
  try {
    const resp = await api.getCategories();
    categoryList = resp.categories.map(c => ({ name: c.name, color: c.color || '#888' }));
  } catch { /* ignore if no categories yet */ }
});
```

- [ ] **Step 2: Add Recategorize button**

Add next to the existing Reanalyze button, following the same pattern (button + progress bar + polling):

```svelte
<button onclick={handleRecategorize} disabled={categorizeRunning}>
  {categorizeRunning ? 'Categorizing...' : 'Recategorize'}
</button>
```

With polling logic matching the existing analyze/sync pattern.

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/components/FilterBar.svelte
git commit -m "feat: add category dropdown and recategorize button to FilterBar"
```

### Task 17: Dashboard Topic Chart Colors

**Files:**
- Modify: `web/src/lib/components/Dashboard.svelte`

- [ ] **Step 1: Color topic bars by category**

In the Topics chart configuration, fetch categories and create a color map from topic name to category color. Use this map to set `backgroundColor` on each bar instead of a single color.

```typescript
// Build topic -> color map from categories
const topicColorMap: Record<string, string> = {};
try {
  const catResp = await api.getCategories();
  for (const cat of catResp.categories) {
    for (const topic of cat.topics) {
      topicColorMap[topic] = cat.color || '#888';
    }
  }
} catch { /* no categories yet */ }

// In chart config:
backgroundColor: topics.map(t => topicColorMap[t.name] || '#888'),
```

- [ ] **Step 2: Commit**

```bash
git add web/src/lib/components/Dashboard.svelte
git commit -m "feat: color topic chart bars by category"
```

---

## Chunk 6: Final Integration & Cleanup

### Task 18: Frontend Build Check

- [ ] **Step 1: Run Svelte type check**

Run: `cd web && npx svelte-check`
Expected: no errors

- [ ] **Step 2: Run frontend build**

Run: `cd web && npm run build`
Expected: builds successfully

- [ ] **Step 3: Fix any type errors or build issues**

- [ ] **Step 4: Commit fixes if any**

### Task 19: Backend Final Check

- [ ] **Step 1: Run full Rust check suite**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets && cargo check --workspace`
Expected: all pass

- [ ] **Step 2: Commit formatting if needed**

### Task 20: Remove Louvain Dependency (if no longer used)

- [ ] **Step 1: Check if graphology-communities-louvain is still imported anywhere**

Search for `louvain` in `web/src/`. If it's no longer used in any component, remove it:

Run: `cd web && npm uninstall graphology-communities-louvain`

- [ ] **Step 2: Commit**

```bash
git add web/package.json web/package-lock.json
git commit -m "chore: remove unused louvain dependency"
```
