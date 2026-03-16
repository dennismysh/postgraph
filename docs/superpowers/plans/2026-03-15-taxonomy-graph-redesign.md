# Taxonomy & Graph Redesign Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the topics/categories system with a two-layer intent+subject taxonomy and rebuild the graph to show subjects as nodes instead of individual posts.

**Architecture:** Single LLM call extracts one intent (communicative purpose) and one subject (topic domain) per post. Subjects become graph nodes connected by shared intent patterns. Old topics/categories/post_edges tables are dropped.

**Tech Stack:** Rust (axum, sqlx, tokio), PostgreSQL, Svelte (SvelteKit), Sigma.js, Mercury LLM (OpenAI-compatible)

**Spec:** `docs/superpowers/specs/2026-03-15-taxonomy-and-graph-redesign.md`

---

## Chunk 1: Database Migration & Types

### Task 1: SQL Migration — Create New Tables

**Files:**
- Create: `postgraph-server/migrations/008_taxonomy_redesign.sql`

- [ ] **Step 1: Write migration**

```sql
-- Create intents table
CREATE TABLE intents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    color TEXT NOT NULL
);

-- Create subjects table
CREATE TABLE subjects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    color TEXT NOT NULL
);

-- Add intent_id and subject_id to posts
ALTER TABLE posts ADD COLUMN intent_id UUID REFERENCES intents(id) ON DELETE SET NULL;
ALTER TABLE posts ADD COLUMN subject_id UUID REFERENCES subjects(id) ON DELETE SET NULL;

-- Create subject_edges table
CREATE TABLE subject_edges (
    source_subject_id UUID NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    target_subject_id UUID NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    weight REAL NOT NULL,
    shared_intents INTEGER NOT NULL,
    PRIMARY KEY (source_subject_id, target_subject_id)
);
```

- [ ] **Step 2: Verify migration compiles with sqlx**

Run: `cd postgraph-server && cargo check`
Expected: compiles (migration isn't checked at compile time, but ensures no syntax issues in surrounding code)

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/migrations/008_taxonomy_redesign.sql
git commit -m "feat: add migration for intents, subjects, subject_edges tables"
```

---

### Task 2: New Rust Types

**Files:**
- Modify: `postgraph-server/src/types.rs`

- [ ] **Step 1: Add Intent, Subject, and SubjectEdge structs**

Add after the existing `Post` struct (keep existing types for now — they'll be removed in the cleanup task):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Intent {
    pub id: uuid::Uuid,
    pub name: String,
    pub description: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Subject {
    pub id: uuid::Uuid,
    pub name: String,
    pub description: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SubjectEdge {
    pub source_subject_id: uuid::Uuid,
    pub target_subject_id: uuid::Uuid,
    pub weight: f32,
    pub shared_intents: i32,
}
```

- [ ] **Step 2: Add intent_id and subject_id to Post struct**

Add two fields to the `Post` struct after `shares`:

```rust
pub intent_id: Option<uuid::Uuid>,
pub subject_id: Option<uuid::Uuid>,
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check --workspace`
Expected: compiles with existing warnings only

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/types.rs
git commit -m "feat: add Intent, Subject, SubjectEdge types and FK fields on Post"
```

---

### Task 3: New Database Functions

**Files:**
- Modify: `postgraph-server/src/db.rs`

- [ ] **Step 1: Add intent/subject CRUD functions**

Add after the existing edge functions (keep old functions for now):

```rust
// -- Intents --

pub async fn upsert_intent(pool: &PgPool, name: &str, description: &str, color: &str) -> sqlx::Result<Intent> {
    sqlx::query_as::<_, Intent>(
        r#"INSERT INTO intents (name, description, color)
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

pub async fn get_all_intents(pool: &PgPool) -> sqlx::Result<Vec<Intent>> {
    sqlx::query_as::<_, Intent>("SELECT * FROM intents ORDER BY name")
        .fetch_all(pool)
        .await
}

// -- Subjects --

pub async fn upsert_subject(pool: &PgPool, name: &str, description: &str, color: &str) -> sqlx::Result<Subject> {
    sqlx::query_as::<_, Subject>(
        r#"INSERT INTO subjects (name, description, color)
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

pub async fn get_all_subjects(pool: &PgPool) -> sqlx::Result<Vec<Subject>> {
    sqlx::query_as::<_, Subject>("SELECT * FROM subjects ORDER BY name")
        .fetch_all(pool)
        .await
}

pub async fn set_post_intent_subject(
    pool: &PgPool,
    post_id: &str,
    intent_id: uuid::Uuid,
    subject_id: uuid::Uuid,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE posts SET intent_id = $1, subject_id = $2 WHERE id = $3")
        .bind(intent_id)
        .bind(subject_id)
        .bind(post_id)
        .execute(pool)
        .await?;
    Ok(())
}

// -- Subject Edges --

pub async fn delete_all_subject_edges(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM subject_edges")
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn upsert_subject_edge(
    pool: &PgPool,
    source: uuid::Uuid,
    target: uuid::Uuid,
    weight: f32,
    shared_intents: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"INSERT INTO subject_edges (source_subject_id, target_subject_id, weight, shared_intents)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (source_subject_id, target_subject_id)
           DO UPDATE SET weight = EXCLUDED.weight, shared_intents = EXCLUDED.shared_intents"#,
    )
    .bind(source)
    .bind(target)
    .bind(weight)
    .bind(shared_intents)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_all_subject_edges(pool: &PgPool) -> sqlx::Result<Vec<SubjectEdge>> {
    sqlx::query_as::<_, SubjectEdge>("SELECT * FROM subject_edges")
        .fetch_all(pool)
        .await
}

pub async fn get_posts_by_subject(
    pool: &PgPool,
    subject_id: uuid::Uuid,
    intent_filter: Option<uuid::Uuid>,
) -> sqlx::Result<Vec<Post>> {
    if let Some(intent_id) = intent_filter {
        sqlx::query_as::<_, Post>(
            "SELECT * FROM posts WHERE subject_id = $1 AND intent_id = $2 ORDER BY timestamp DESC",
        )
        .bind(subject_id)
        .bind(intent_id)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, Post>(
            "SELECT * FROM posts WHERE subject_id = $1 ORDER BY timestamp DESC",
        )
        .bind(subject_id)
        .fetch_all(pool)
        .await
    }
}
```

- [ ] **Step 2: Add next_color helper for color assignment**

Add near the top of `db.rs`, after the `CATEGORY_COLORS` constant:

```rust
/// Assign the next unused color from the palette. Falls back to cycling if all used.
pub fn next_color(used_count: usize) -> &'static str {
    CATEGORY_COLORS[used_count % CATEGORY_COLORS.len()]
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check --workspace`
Expected: compiles (new functions reference new types from types.rs)

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/db.rs
git commit -m "feat: add database functions for intents, subjects, subject_edges"
```

---

## Chunk 2: Mercury & Analysis Pipeline

### Task 4: Rewrite Mercury analyze_posts()

**Files:**
- Modify: `postgraph-server/src/mercury.rs`

- [ ] **Step 1: Add new response types for intent/subject analysis**

Replace `TopicAssignment` and the `topics` field in `AnalyzedPost` with:

```rust
#[derive(Debug, Deserialize)]
pub struct AnalyzedPost {
    pub post_id: String,
    pub intent: String,
    pub subject: String,
    pub sentiment: f32,
}

#[derive(Debug, Deserialize)]
pub struct AnalysisResponse {
    pub posts: Vec<AnalyzedPost>,
}
```

Remove the old `TopicAssignment` struct.

- [ ] **Step 2: Rewrite analyze_posts() with new prompt**

Replace the existing `analyze_posts()` function body. The function signature changes to accept existing intents/subjects instead of topics:

```rust
pub async fn analyze_posts(
    &self,
    posts: &[(String, String)],
    existing_intents: &[String],
    existing_subjects: &[String],
) -> Result<Vec<AnalyzedPost>, AppError> {
```

The system prompt is the one from the spec (Section 4: LLM Analysis Prompt). Insert `existing_intents` and `existing_subjects` as comma-separated lists into the `{intents_list}` and `{subjects_list}` placeholders.

The posts JSON format stays the same: `[{"id": "...", "text": "..."}]`.

Parse the response as `AnalysisResponse` and return `response.posts`.

- [ ] **Step 3: Delete categorize_topics() and assign_topic_category()**

Remove these two functions entirely. They are no longer called anywhere.

- [ ] **Step 4: Verify compilation**

Run: `cargo check --workspace`
Expected: Compilation errors in `analysis.rs` and routes that call the old functions — this is expected, we'll fix them next.

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/mercury.rs
git commit -m "feat: rewrite Mercury analysis for intent/subject taxonomy"
```

---

### Task 5: Rewrite Analysis Pipeline

**Files:**
- Modify: `postgraph-server/src/analysis.rs`

- [ ] **Step 1: Rewrite run_analysis() for intent/subject flow**

The new flow:
1. Fetch unanalyzed posts (same as before)
2. Get existing intent names and subject names from DB
3. Call Mercury `analyze_posts()` with the new signature
4. For each result: upsert intent (with color), upsert subject (with color), set FK on post, mark analyzed with sentiment

```rust
pub async fn run_analysis(
    pool: &PgPool,
    mercury: &MercuryClient,
    progress: Option<(&Arc<AtomicU32>, &Arc<AtomicU32>)>,
) -> Result<u32, AppError> {
    let batch_size: i64 = std::env::var("ANALYSIS_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);

    let mut total_analyzed: u32 = 0;

    loop {
        let unanalyzed = db::get_unanalyzed_posts(pool, batch_size).await?;
        if unanalyzed.is_empty() {
            break;
        }

        let existing_intents: Vec<String> = db::get_all_intents(pool)
            .await?
            .into_iter()
            .map(|i| i.name)
            .collect();
        let existing_subjects: Vec<String> = db::get_all_subjects(pool)
            .await?
            .into_iter()
            .map(|s| s.name)
            .collect();

        let post_pairs: Vec<(String, String)> = unanalyzed
            .iter()
            .filter_map(|p| p.text.as_ref().map(|t| (p.id.clone(), t.clone())))
            .collect();

        if post_pairs.is_empty() {
            break;
        }

        let results = mercury.analyze_posts(&post_pairs, &existing_intents, &existing_subjects).await?;

        for result in &results {
            // Count existing to determine color index
            let intent_count = db::get_all_intents(pool).await?.len();
            let intent = db::upsert_intent(
                pool,
                &result.intent,
                "",
                db::next_color(intent_count),
            ).await?;

            let subject_count = db::get_all_subjects(pool).await?.len();
            let subject = db::upsert_subject(
                pool,
                &result.subject,
                "",
                db::next_color(subject_count),
            ).await?;

            db::set_post_intent_subject(pool, &result.post_id, intent.id, subject.id).await?;
            db::mark_post_analyzed(pool, &result.post_id, result.sentiment).await?;

            total_analyzed += 1;
            if let Some((prog, tot)) = &progress {
                prog.store(total_analyzed, std::sync::atomic::Ordering::SeqCst);
            }
        }

        if let Some((_, tot)) = &progress {
            tot.store(total_analyzed, std::sync::atomic::Ordering::SeqCst);
        }

        info!("Analyzed batch: {} posts", results.len());
    }

    Ok(total_analyzed)
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --workspace`
Expected: May still have errors in routes that call old categorize functions — addressed in next tasks.

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/analysis.rs
git commit -m "feat: rewrite analysis pipeline for intent/subject taxonomy"
```

---

### Task 6: Rewrite Edge Computation

**Files:**
- Modify: `postgraph-server/src/graph.rs`

- [ ] **Step 1: Replace post-level edge functions with subject-level**

Delete the existing `compute_edges_for_post()` and `compute_edges_for_recent()` functions. Replace with:

```rust
use crate::db;
use crate::error::AppError;
use sqlx::PgPool;
use tracing::info;

/// Compute edges between subjects based on shared intent patterns.
/// Two subjects are connected if they share at least 2 intent types.
/// Weight = shared_intents / total_distinct_intents.
pub async fn compute_subject_edges(pool: &PgPool) -> Result<u32, AppError> {
    // Clear existing edges
    db::delete_all_subject_edges(pool).await?;

    // Get all subjects
    let subjects = db::get_all_subjects(pool).await?;
    let total_intents = db::get_all_intents(pool).await?.len() as f32;

    if total_intents == 0.0 {
        return Ok(0);
    }

    let mut edge_count: u32 = 0;

    // For each pair of subjects, compute shared intents
    for i in 0..subjects.len() {
        for j in (i + 1)..subjects.len() {
            let shared: (i64,) = sqlx::query_as(
                r#"SELECT COUNT(*)::bigint FROM (
                    SELECT DISTINCT intent_id FROM posts WHERE subject_id = $1 AND intent_id IS NOT NULL
                    INTERSECT
                    SELECT DISTINCT intent_id FROM posts WHERE subject_id = $2 AND intent_id IS NOT NULL
                ) shared"#,
            )
            .bind(subjects[i].id)
            .bind(subjects[j].id)
            .fetch_one(pool)
            .await?;

            let shared_count = shared.0 as i32;
            if shared_count >= 2 {
                let weight = shared_count as f32 / total_intents;
                db::upsert_subject_edge(
                    pool,
                    subjects[i].id,
                    subjects[j].id,
                    weight,
                    shared_count,
                ).await?;
                edge_count += 1;
            }
        }
    }

    info!("Computed {edge_count} subject edges across {} subjects", subjects.len());
    Ok(edge_count)
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --workspace`

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/graph.rs
git commit -m "feat: rewrite edge computation for subject-level graph"
```

---

## Chunk 3: Backend Routes

### Task 7: Rewrite Graph API Routes

**Files:**
- Modify: `postgraph-server/src/routes/graph.rs`

- [ ] **Step 1: Replace response types and get_graph handler**

Replace the existing `GraphNode`, `GraphEdge`, `GraphData` structs and `get_graph()` handler with subject-level equivalents:

```rust
#[derive(Serialize)]
pub struct SubjectNode {
    pub id: String,
    pub label: String,
    pub post_count: i64,
    pub avg_engagement: f64,
    pub color: String,
}

#[derive(Serialize)]
pub struct SubjectGraphEdge {
    pub source: String,
    pub target: String,
    pub weight: f32,
    pub shared_intents: i32,
}

#[derive(Serialize)]
pub struct IntentInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    pub post_count: i64,
}

#[derive(Serialize)]
pub struct SubjectGraphData {
    pub nodes: Vec<SubjectNode>,
    pub edges: Vec<SubjectGraphEdge>,
    pub intents: Vec<IntentInfo>,
}

#[derive(Deserialize)]
pub struct GraphQuery {
    pub intent: Option<String>,
}
```

The `get_graph()` handler queries `subjects` for nodes (with post counts and avg engagement), `subject_edges` for edges, and `intents` for the filter list. If `?intent=X` is provided, filter the post counts to only count posts with that intent.

- [ ] **Step 2: Remove or simplify get_tag_graph**

The old tag graph is replaced by the subject graph. Remove `get_tag_graph()` or keep as a stub that returns an empty response.

- [ ] **Step 3: Verify compilation**

Run: `cargo check --workspace`

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/routes/graph.rs
git commit -m "feat: rewrite graph routes for subject-level visualization"
```

---

### Task 8: Add Subject Posts Route

**Files:**
- Create: `postgraph-server/src/routes/subjects.rs`
- Modify: `postgraph-server/src/routes/mod.rs`

- [ ] **Step 1: Create subjects route handler**

```rust
use crate::db;
use crate::state::AppState;
use axum::{Json, extract::Path, extract::Query, extract::State};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct SubjectPost {
    pub id: String,
    pub text: Option<String>,
    pub intent: String,
    pub engagement: i64,
    pub views: i32,
    pub timestamp: String,
}

#[derive(Serialize)]
pub struct SubjectPostsResponse {
    pub subject: String,
    pub posts: Vec<SubjectPost>,
}

#[derive(Deserialize)]
pub struct SubjectPostsQuery {
    pub intent: Option<String>,
}

pub async fn get_subject_posts(
    State(state): State<AppState>,
    Path(subject_id): Path<uuid::Uuid>,
    Query(query): Query<SubjectPostsQuery>,
) -> Result<Json<SubjectPostsResponse>, axum::http::StatusCode> {
    // Look up the subject name
    let subjects = db::get_all_subjects(&state.pool).await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let subject = subjects.iter().find(|s| s.id == subject_id)
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    // Resolve optional intent filter
    let intent_id = if let Some(ref intent_name) = query.intent {
        let intents = db::get_all_intents(&state.pool).await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
        intents.iter().find(|i| i.name == *intent_name).map(|i| i.id)
    } else {
        None
    };

    let posts = db::get_posts_by_subject(&state.pool, subject_id, intent_id).await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Look up intent names for each post
    let all_intents = db::get_all_intents(&state.pool).await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let response_posts: Vec<SubjectPost> = posts.into_iter().map(|p| {
        let intent_name = p.intent_id
            .and_then(|iid| all_intents.iter().find(|i| i.id == iid))
            .map(|i| i.name.clone())
            .unwrap_or_default();
        SubjectPost {
            id: p.id,
            text: p.text,
            intent: intent_name,
            engagement: (p.likes + p.replies_count + p.reposts + p.quotes) as i64,
            views: p.views,
            timestamp: p.timestamp.to_rfc3339(),
        }
    }).collect();

    Ok(Json(SubjectPostsResponse {
        subject: subject.name.clone(),
        posts: response_posts,
    }))
}
```

- [ ] **Step 2: Register in routes/mod.rs**

Add `pub mod subjects;` to `postgraph-server/src/routes/mod.rs`.

- [ ] **Step 3: Verify compilation**

Run: `cargo check --workspace`

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/routes/subjects.rs postgraph-server/src/routes/mod.rs
git commit -m "feat: add /api/subjects/{id}/posts endpoint"
```

---

### Task 9: Update main.rs — Routes, State, Background Tasks

**Files:**
- Modify: `postgraph-server/src/main.rs`
- Modify: `postgraph-server/src/state.rs`

- [ ] **Step 1: Remove categorize fields from AppState**

In `state.rs`, remove `categorize_running`, `categorize_progress`, `categorize_total` fields.

In `main.rs`, remove their initialization in the `AppState` construction.

- [ ] **Step 2: Update route registration**

Remove routes:
- `/api/categorize`
- `/api/categorize/status`
- `/api/categories`

Add route:
- `.route("/api/subjects/{id}/posts", get(routes::subjects::get_subject_posts))`

- [ ] **Step 3: Update background sync and nightly sync**

Replace `graph::compute_edges_for_recent()` calls with `graph::compute_subject_edges()` calls in both the 15-minute background sync and the nightly sync.

- [ ] **Step 4: Remove categorize route module import**

Remove `pub mod categorize;` from `routes/mod.rs` and delete `postgraph-server/src/routes/categorize.rs`.

- [ ] **Step 5: Update analyze and reanalyze routes**

In `routes/analyze.rs`: remove the auto-categorization trigger block (the `if cat_count == 0` logic).

In `routes/reanalyze.rs`: remove the categorization trigger after reanalysis. Update `reset_all_analysis` in `db.rs` to also null out `intent_id` and `subject_id`, and clear `subject_edges`.

- [ ] **Step 6: Update analytics route**

In `routes/analytics.rs`: replace `total_topics` / `topics` in the `AnalyticsData` response with subject/intent counts. Update the SQL queries to join on `subjects`/`intents` instead of `topics`/`post_topics`.

- [ ] **Step 7: Verify full compilation**

Run: `cargo check --workspace`
Expected: Clean compilation

- [ ] **Step 8: Commit**

```bash
git add -A postgraph-server/src/
git commit -m "feat: update routes, state, and background tasks for new taxonomy"
```

---

## Chunk 4: Frontend

### Task 10: Update API Types and Client

**Files:**
- Modify: `web/src/lib/api.ts`

- [ ] **Step 1: Update graph-related types**

Replace `GraphNode`, `GraphEdge`, `GraphData` types with the new subject-level types matching the backend response. Add `SubjectPostsResponse` type. Remove `getCategories`, `triggerCategorize`, `getCategorizeStatus` methods. Add `getSubjectPosts(subjectId, intentFilter?)` method.

Update `AnalyticsData` to replace `total_topics` / `topics` with `total_subjects`, `subjects`, `total_intents`, `intents`.

- [ ] **Step 2: Commit**

```bash
git add web/src/lib/api.ts
git commit -m "feat: update API types for intent/subject taxonomy"
```

---

### Task 11: Rewrite Graph.svelte for Subject Network

**Files:**
- Modify: `web/src/lib/components/Graph.svelte`

- [ ] **Step 1: Update graph rendering for subject nodes**

Key changes:
- Nodes are subjects (15-25 nodes), not posts (819 nodes)
- Node size based on `post_count` (logarithmic scale)
- Node color from `subject.color`
- Labels always visible (few enough nodes)
- Click handler: show sidebar with posts for that subject (call `getSubjectPosts`)
- ForceAtlas2 settings: higher gravity for tighter layout with fewer nodes

- [ ] **Step 2: Add intent filter dropdown**

Add a dropdown at the top of the graph that filters by intent. When an intent is selected, refetch graph data with `?intent=IntentName` to update node sizes.

- [ ] **Step 3: Add subject detail sidebar**

When a subject node is clicked, show a sidebar panel with the subject's posts (fetched from `/api/subjects/{id}/posts`). Each post shows text preview, intent badge, engagement, and date.

- [ ] **Step 4: Commit**

```bash
git add web/src/lib/components/Graph.svelte
git commit -m "feat: rewrite graph for subject-level visualization with intent filtering"
```

---

### Task 12: Update FilterBar and Page

**Files:**
- Modify: `web/src/lib/components/FilterBar.svelte`
- Modify: `web/src/routes/+page.svelte`

- [ ] **Step 1: Simplify FilterBar for intent/subject**

Replace topic pills and category dropdown with:
- Intent dropdown (select to filter graph by intent)
- Subject count display
- Remove "Recategorize" button

- [ ] **Step 2: Update +page.svelte**

Remove references to categories and categorize status. Update the post detail panel to show intent and subject instead of topics. Remove the Tags Graph tab (or repurpose if keeping).

- [ ] **Step 3: Remove old proxy routes**

Delete:
- `web/src/routes/api/categories/+server.ts`
- `web/src/routes/api/categorize/+server.ts`
- `web/src/routes/api/categorize/status/+server.ts`

Add:
- `web/src/routes/api/subjects/[id]/posts/+server.ts` — proxy to backend

- [ ] **Step 4: Verify frontend build**

Run: `cd web && npx svelte-check`
Expected: No new errors from our changes

- [ ] **Step 5: Commit**

```bash
git add -A web/
git commit -m "feat: update frontend for intent/subject taxonomy and subject graph"
```

---

## Chunk 5: Reanalysis & Cleanup

### Task 13: Deploy and Reanalyze

- [ ] **Step 1: Push all changes**

```bash
git push
```

- [ ] **Step 2: Wait for deploy, then trigger reanalysis**

After Railway deploy completes, trigger a full reanalysis via the dashboard "Reanalyze" button or:
```
POST /api/reanalyze
```

This will clear old analysis data and re-run all 819 posts through the new intent/subject pipeline.

- [ ] **Step 3: Verify results**

Check the graph page — should show ~15-25 subject nodes with edges between related subjects. Verify intent filtering works. Click a subject to see its posts in the sidebar.

---

### Task 14: Drop Old Tables

**Files:**
- Create: `postgraph-server/migrations/009_drop_old_analysis_tables.sql`

Only do this AFTER verifying the new system works correctly.

- [ ] **Step 1: Write cleanup migration**

```sql
-- Drop old analysis tables (replaced by intents/subjects/subject_edges)
DROP TABLE IF EXISTS post_edges CASCADE;
DROP TABLE IF EXISTS post_topics CASCADE;
DROP TABLE IF EXISTS categories CASCADE;
DROP TABLE IF EXISTS topics CASCADE;
```

- [ ] **Step 2: Remove old types and DB functions**

Remove from `types.rs`: `Topic`, `Category`, `PostTopic`, `PostEdge` structs.

Remove from `db.rs`: `upsert_topic`, `get_all_topics`, `get_all_categories`, `upsert_category`, `set_topic_category`, `delete_orphaned_categories`, `get_categories_with_topics`, `upsert_post_topic`, `upsert_edge`, `get_all_edges`, `get_topics_for_post`.

- [ ] **Step 3: Final compilation check**

Run: `cargo check --workspace && cd web && npx svelte-check`

- [ ] **Step 4: Commit and push**

```bash
git add -A
git commit -m "chore: drop old topics/categories tables and unused code"
git push
```
