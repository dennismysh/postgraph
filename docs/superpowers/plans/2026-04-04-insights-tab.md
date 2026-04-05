# Insights Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an LLM-powered Insights tab that analyzes the last 30 days of Threads activity into four narrative sections (What's Working, What's Not Working, On Brand, Off Pattern) with post citations.

**Architecture:** Rust pre-computes a structured analytics context (post stats, subject/intent comparisons, trends), Mercury narrates it into four sections with a candid-friend voice, and reports are stored in a `insights_reports` table. Hybrid refresh: nightly auto + manual on-demand.

**Tech Stack:** Rust (axum, sqlx, serde), Mercury LLM (OpenAI-compatible API), SvelteKit, PostgreSQL (JSONB)

**Spec:** `docs/superpowers/specs/2026-04-04-insights-tab-design.md`

---

### Task 1: Database Migration

**Files:**
- Create: `postgraph-server/migrations/012_insights_reports.sql`

- [ ] **Step 1: Create the migration file**

```sql
CREATE TABLE insights_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    trigger_type TEXT NOT NULL,
    report JSONB NOT NULL,
    context JSONB NOT NULL
);
```

- [ ] **Step 2: Verify the migration compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles (migrations run at startup via `sqlx::migrate!()`)

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/migrations/012_insights_reports.sql
git commit -m "feat(insights): add insights_reports table migration"
```

---

### Task 2: InsightsContext Computation

**Files:**
- Create: `postgraph-server/src/insights.rs`
- Modify: `postgraph-server/src/main.rs:1` (add `mod insights;`)

This task builds the Rust function that queries all existing tables and assembles the structured analytics context that Mercury will narrate. No Mercury call yet — just the data gathering.

- [ ] **Step 1: Add module declaration**

In `postgraph-server/src/main.rs`, add `mod insights;` after line 5 (`mod graph;`):

```rust
mod insights;
```

- [ ] **Step 2: Create insights.rs with context types and computation**

Create `postgraph-server/src/insights.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;

use crate::error::AppError;
use crate::mercury::MercuryClient;

// ── Context types (sent to Mercury) ──────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct InsightsContext {
    pub window_start: String,
    pub window_end: String,
    pub posts: Vec<PostSummary>,
    pub top_posts: Vec<PostSummary>,
    pub bottom_posts: Vec<PostSummary>,
    pub subject_stats: Vec<CategoryStats>,
    pub intent_stats: Vec<CategoryStats>,
    pub posting_frequency: FrequencyStats,
    pub sentiment: SentimentStats,
    pub daily_views: Vec<DailyViewPoint>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostSummary {
    pub id: String,
    pub text: String,
    pub permalink: Option<String>,
    pub timestamp: String,
    pub views: i32,
    pub likes: i32,
    pub replies: i32,
    pub reposts: i32,
    pub quotes: i32,
    pub intent: Option<String>,
    pub subject: Option<String>,
    pub sentiment: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CategoryStats {
    pub name: String,
    pub recent_post_count: i64,
    pub recent_avg_views: f64,
    pub recent_avg_engagement: f64,
    pub alltime_post_count: i64,
    pub alltime_avg_views: f64,
    pub alltime_avg_engagement: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FrequencyStats {
    pub recent_posts_per_week: f64,
    pub alltime_posts_per_week: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SentimentStats {
    pub recent_avg: f64,
    pub alltime_avg: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DailyViewPoint {
    pub date: String,
    pub views: i64,
}

// ── Report types (stored in DB) ──────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct InsightsReport {
    pub headline: String,
    pub sections: Vec<InsightsSection>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InsightsSection {
    pub key: String,
    pub title: String,
    pub summary: String,
    pub items: Vec<InsightsItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InsightsItem {
    pub observation: String,
    pub cited_posts: Vec<String>,
    pub tone: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredReport {
    pub id: String,
    pub generated_at: DateTime<Utc>,
    pub trigger_type: String,
    pub report: InsightsReport,
}

// ── Context computation ──────────────────────────────────────────

pub async fn compute_context(pool: &PgPool) -> Result<InsightsContext, AppError> {
    let now = Utc::now();
    let thirty_days_ago = now - chrono::Duration::days(30);
    let window_start = thirty_days_ago.format("%Y-%m-%d").to_string();
    let window_end = now.format("%Y-%m-%d").to_string();

    // Posts in the last 30 days with intent/subject names
    let posts: Vec<PostSummary> = sqlx::query_as::<_, (
        String,             // id
        Option<String>,     // text
        Option<String>,     // permalink
        DateTime<Utc>,      // timestamp
        i32, i32, i32, i32, i32, // views, likes, replies_count, reposts, quotes
        Option<String>,     // intent name
        Option<String>,     // subject name
        Option<f32>,        // sentiment
    )>(
        r#"SELECT p.id, p.text, p.permalink, p.timestamp,
                  p.views, p.likes, p.replies_count, p.reposts, p.quotes,
                  i.name AS intent_name, s.name AS subject_name, p.sentiment
           FROM posts p
           LEFT JOIN intents i ON p.intent_id = i.id
           LEFT JOIN subjects s ON p.subject_id = s.id
           WHERE p.timestamp >= $1 AND p.analyzed_at IS NOT NULL
           ORDER BY p.timestamp DESC"#,
    )
    .bind(thirty_days_ago)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| PostSummary {
        id: row.0,
        text: row.1.unwrap_or_default().chars().take(200).collect(),
        permalink: row.2,
        timestamp: row.3.format("%Y-%m-%d %H:%M").to_string(),
        views: row.4,
        likes: row.5,
        replies: row.6,
        reposts: row.7,
        quotes: row.8,
        intent: row.9,
        subject: row.10,
        sentiment: row.11,
    })
    .collect();

    // Top 5 and bottom 5 by views
    let mut sorted_by_views = posts.clone();
    sorted_by_views.sort_by(|a, b| b.views.cmp(&a.views));
    let top_posts: Vec<PostSummary> = sorted_by_views.iter().take(5).cloned().collect();
    let bottom_posts: Vec<PostSummary> = sorted_by_views.iter().rev().take(5).cloned().collect();

    // Per-subject stats: recent (30d) vs all-time
    let subject_stats: Vec<CategoryStats> = sqlx::query_as::<_, (
        String, i64, f64, f64, i64, f64, f64,
    )>(
        r#"WITH recent AS (
               SELECT s.name,
                      COUNT(*) AS post_count,
                      COALESCE(AVG(p.views), 0) AS avg_views,
                      COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes), 0) AS avg_engagement
               FROM posts p
               JOIN subjects s ON p.subject_id = s.id
               WHERE p.timestamp >= $1 AND p.analyzed_at IS NOT NULL
               GROUP BY s.name
           ),
           alltime AS (
               SELECT s.name,
                      COUNT(*) AS post_count,
                      COALESCE(AVG(p.views), 0) AS avg_views,
                      COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes), 0) AS avg_engagement
               FROM posts p
               JOIN subjects s ON p.subject_id = s.id
               WHERE p.analyzed_at IS NOT NULL
               GROUP BY s.name
           )
           SELECT r.name,
                  r.post_count, r.avg_views, r.avg_engagement,
                  a.post_count, a.avg_views, a.avg_engagement
           FROM recent r
           JOIN alltime a ON r.name = a.name
           ORDER BY r.post_count DESC"#,
    )
    .bind(thirty_days_ago)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| CategoryStats {
        name: row.0,
        recent_post_count: row.1,
        recent_avg_views: row.2,
        recent_avg_engagement: row.3,
        alltime_post_count: row.4,
        alltime_avg_views: row.5,
        alltime_avg_engagement: row.6,
    })
    .collect();

    // Per-intent stats: same structure
    let intent_stats: Vec<CategoryStats> = sqlx::query_as::<_, (
        String, i64, f64, f64, i64, f64, f64,
    )>(
        r#"WITH recent AS (
               SELECT i.name,
                      COUNT(*) AS post_count,
                      COALESCE(AVG(p.views), 0) AS avg_views,
                      COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes), 0) AS avg_engagement
               FROM posts p
               JOIN intents i ON p.intent_id = i.id
               WHERE p.timestamp >= $1 AND p.analyzed_at IS NOT NULL
               GROUP BY i.name
           ),
           alltime AS (
               SELECT i.name,
                      COUNT(*) AS post_count,
                      COALESCE(AVG(p.views), 0) AS avg_views,
                      COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes), 0) AS avg_engagement
               FROM posts p
               JOIN intents i ON p.intent_id = i.id
               WHERE p.analyzed_at IS NOT NULL
               GROUP BY i.name
           )
           SELECT r.name,
                  r.post_count, r.avg_views, r.avg_engagement,
                  a.post_count, a.avg_views, a.avg_engagement
           FROM recent r
           JOIN alltime a ON r.name = a.name
           ORDER BY r.post_count DESC"#,
    )
    .bind(thirty_days_ago)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| CategoryStats {
        name: row.0,
        recent_post_count: row.1,
        recent_avg_views: row.2,
        recent_avg_engagement: row.3,
        alltime_post_count: row.4,
        alltime_avg_views: row.5,
        alltime_avg_engagement: row.6,
    })
    .collect();

    // Posting frequency: recent vs all-time
    let (recent_count, recent_weeks): (i64, f64) = sqlx::query_as(
        r#"SELECT COUNT(*),
                  GREATEST(EXTRACT(EPOCH FROM (NOW() - $1::timestamptz)) / 604800.0, 1.0)
           FROM posts
           WHERE timestamp >= $1 AND analyzed_at IS NOT NULL"#,
    )
    .bind(thirty_days_ago)
    .fetch_one(pool)
    .await?;

    let (alltime_count, alltime_weeks): (i64, f64) = sqlx::query_as(
        r#"SELECT COUNT(*),
                  GREATEST(EXTRACT(EPOCH FROM (NOW() - MIN(timestamp))) / 604800.0, 1.0)
           FROM posts
           WHERE analyzed_at IS NOT NULL"#,
    )
    .fetch_one(pool)
    .await?;

    let posting_frequency = FrequencyStats {
        recent_posts_per_week: recent_count as f64 / recent_weeks,
        alltime_posts_per_week: alltime_count as f64 / alltime_weeks,
    };

    // Sentiment: recent vs all-time
    let (recent_sentiment,): (Option<f64>,) = sqlx::query_as(
        r#"SELECT AVG(sentiment::float8)
           FROM posts
           WHERE timestamp >= $1 AND sentiment IS NOT NULL"#,
    )
    .bind(thirty_days_ago)
    .fetch_one(pool)
    .await?;

    let (alltime_sentiment,): (Option<f64>,) = sqlx::query_as(
        r#"SELECT AVG(sentiment::float8)
           FROM posts
           WHERE sentiment IS NOT NULL"#,
    )
    .fetch_one(pool)
    .await?;

    let sentiment = SentimentStats {
        recent_avg: recent_sentiment.unwrap_or(0.0),
        alltime_avg: alltime_sentiment.unwrap_or(0.0),
    };

    // Daily views for the 30-day window
    let daily_views: Vec<DailyViewPoint> = sqlx::query_as::<_, (String, i64)>(
        r#"SELECT date::text, views
           FROM daily_views
           WHERE date >= $1::date
           ORDER BY date"#,
    )
    .bind(thirty_days_ago.date_naive())
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(date, views)| DailyViewPoint { date, views })
    .collect();

    let post_count = posts.len();
    info!("Computed insights context: {post_count} posts, {} subjects, {} intents",
          subject_stats.len(), intent_stats.len());

    Ok(InsightsContext {
        window_start,
        window_end,
        posts,
        top_posts,
        bottom_posts,
        subject_stats,
        intent_stats,
        posting_frequency,
        sentiment,
        daily_views,
    })
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles (the `generate_report` and `MercuryClient` usage comes in the next task)

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/insights.rs postgraph-server/src/main.rs
git commit -m "feat(insights): add InsightsContext computation"
```

---

### Task 3: Mercury Insights Prompt

**Files:**
- Modify: `postgraph-server/src/mercury.rs` (add `generate_insights` method)

Add a new method to `MercuryClient` that takes the pre-computed context and returns a structured insights report.

- [ ] **Step 1: Add the InsightsReport import and method to mercury.rs**

At the top of `postgraph-server/src/mercury.rs`, add the import after line 3:

```rust
use crate::insights::{InsightsContext, InsightsReport};
```

Then add this method to the `impl MercuryClient` block, after the `analyze_posts` method (after line 192, before the closing `}`):

```rust
    pub async fn generate_insights(
        &self,
        context: &InsightsContext,
    ) -> Result<InsightsReport, AppError> {
        let context_json = serde_json::to_string_pretty(context)
            .map_err(|e| AppError::MercuryApi(format!("Failed to serialize context: {e}")))?;

        let system_prompt = r#"You're a sharp social media analyst reviewing a creator's last 30 days of Threads activity. You're direct, a little informal, and you don't sugarcoat. But you back everything up with data. When you cite a post, reference it by its ID.

Organize your analysis into exactly four sections:

1. "working" — What's Working: subjects, intents, or patterns that are outperforming. Cite the standout posts.
2. "not_working" — What's Not Working: underperformers. Be specific about why (low engagement vs expectations, sentiment mismatch, etc.)
3. "on_brand" — On Brand: behavior consistent with established patterns. Posting cadence, topic mix, sentiment that matches baseline.
4. "off_pattern" — Off Pattern: deviations from the norm. Could be positive or negative — new topics tried, frequency changes, tone shifts.

Each section should have:
- A "summary": one paragraph of candid analysis
- 2-4 "items", each with an "observation" (one sentence), "cited_posts" (1-2 post IDs from the data), and "tone" ("positive", "negative", or "neutral")

Also write a "headline": one punchy sentence summarizing the whole month.

Respond with ONLY valid JSON matching this structure:
{"headline": "...", "sections": [{"key": "working", "title": "What's Working", "summary": "...", "items": [{"observation": "...", "cited_posts": ["id1"], "tone": "positive"}]}, {"key": "not_working", "title": "What's Not Working", ...}, {"key": "on_brand", "title": "On Brand", ...}, {"key": "off_pattern", "title": "Off Pattern", ...}]}"#;

        let request = ChatRequest {
            model: "mercury-2".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: format!("Here is the analytics data for the last 30 days:\n\n{context_json}"),
                },
            ],
            temperature: 0.5,
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

        let report: InsightsReport = serde_json::from_str(json_str).map_err(|e| {
            AppError::MercuryApi(format!("Failed to parse insights response: {e}. Raw: {json_str}"))
        })?;

        Ok(report)
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/mercury.rs
git commit -m "feat(insights): add Mercury generate_insights prompt"
```

---

### Task 4: Report Generation & Storage

**Files:**
- Modify: `postgraph-server/src/insights.rs` (add `generate_report` and DB functions)

Add the orchestration function that ties context computation → Mercury call → DB storage together, plus the DB read function for fetching the latest report.

- [ ] **Step 1: Add the generate and DB functions to insights.rs**

Add these functions at the bottom of `postgraph-server/src/insights.rs`, after the `compute_context` function:

```rust
// ── Report generation (context → Mercury → DB) ──────────────────

const MIN_POSTS_THRESHOLD: usize = 5;

pub async fn generate_report(
    pool: &PgPool,
    mercury: &MercuryClient,
    trigger_type: &str,
) -> Result<StoredReport, AppError> {
    let context = compute_context(pool).await?;

    if context.posts.len() < MIN_POSTS_THRESHOLD {
        return Err(AppError::MercuryApi(format!(
            "Not enough data: {} analyzed posts in the last 30 days (minimum {})",
            context.posts.len(),
            MIN_POSTS_THRESHOLD,
        )));
    }

    info!("Generating insights report ({trigger_type})...");
    let report = mercury.generate_insights(&context).await?;

    let context_json = serde_json::to_value(&context)
        .map_err(|e| AppError::MercuryApi(format!("Failed to serialize context: {e}")))?;
    let report_json = serde_json::to_value(&report)
        .map_err(|e| AppError::MercuryApi(format!("Failed to serialize report: {e}")))?;

    let (id, generated_at): (uuid::Uuid, DateTime<Utc>) = sqlx::query_as(
        r#"INSERT INTO insights_reports (trigger_type, report, context)
           VALUES ($1, $2, $3)
           RETURNING id, generated_at"#,
    )
    .bind(trigger_type)
    .bind(&report_json)
    .bind(&context_json)
    .fetch_one(pool)
    .await?;

    info!("Insights report generated: {id}");

    Ok(StoredReport {
        id: id.to_string(),
        generated_at,
        trigger_type: trigger_type.to_string(),
        report,
    })
}

// ── DB reads ─────────────────────────────────────────────────────

pub async fn get_latest_report(pool: &PgPool) -> Result<Option<StoredReport>, AppError> {
    let row: Option<(uuid::Uuid, DateTime<Utc>, String, serde_json::Value)> = sqlx::query_as(
        r#"SELECT id, generated_at, trigger_type, report
           FROM insights_reports
           ORDER BY generated_at DESC
           LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await?;

    match row {
        None => Ok(None),
        Some((id, generated_at, trigger_type, report_json)) => {
            let report: InsightsReport = serde_json::from_value(report_json)
                .map_err(|e| AppError::MercuryApi(format!("Corrupt stored report: {e}")))?;
            Ok(Some(StoredReport {
                id: id.to_string(),
                generated_at,
                trigger_type,
                report,
            }))
        }
    }
}
```

- [ ] **Step 2: Add the uuid import at the top of insights.rs**

At the top of `postgraph-server/src/insights.rs`, the imports should be:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;

use crate::error::AppError;
use crate::mercury::MercuryClient;
```

(No new imports needed — `uuid` is used inline as `uuid::Uuid`, and `serde_json` is already available.)

- [ ] **Step 3: Verify it compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/insights.rs
git commit -m "feat(insights): add report generation and storage"
```

---

### Task 5: API Routes

**Files:**
- Create: `postgraph-server/src/routes/insights.rs`
- Modify: `postgraph-server/src/routes/mod.rs` (add `pub mod insights;`)
- Modify: `postgraph-server/src/main.rs` (mount routes)

- [ ] **Step 1: Add the module declaration**

In `postgraph-server/src/routes/mod.rs`, add after the existing modules:

```rust
pub mod insights;
```

- [ ] **Step 2: Create the route handlers**

Create `postgraph-server/src/routes/insights.rs`:

```rust
use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::insights;
use crate::state::AppState;

#[derive(Serialize)]
pub struct InsightsResponse {
    pub id: String,
    pub generated_at: String,
    pub trigger_type: String,
    pub report: insights::InsightsReport,
}

#[derive(Serialize)]
pub struct InsightsError {
    pub error: String,
}

pub async fn get_latest(
    State(state): State<AppState>,
) -> Result<Json<InsightsResponse>, (axum::http::StatusCode, Json<InsightsError>)> {
    let report = insights::get_latest_report(&state.pool)
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(InsightsError {
                    error: e.to_string(),
                }),
            )
        })?;

    match report {
        Some(r) => Ok(Json(InsightsResponse {
            id: r.id,
            generated_at: r.generated_at.to_rfc3339(),
            trigger_type: r.trigger_type,
            report: r.report,
        })),
        None => Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(InsightsError {
                error: "No insights report generated yet".to_string(),
            }),
        )),
    }
}

pub async fn generate(
    State(state): State<AppState>,
) -> Result<Json<InsightsResponse>, (axum::http::StatusCode, Json<InsightsError>)> {
    let report = insights::generate_report(&state.pool, &state.mercury, "manual")
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(InsightsError {
                    error: e.to_string(),
                }),
            )
        })?;

    Ok(Json(InsightsResponse {
        id: report.id,
        generated_at: report.generated_at.to_rfc3339(),
        trigger_type: report.trigger_type,
        report: report.report,
    }))
}
```

- [ ] **Step 3: Mount the routes in main.rs**

In `postgraph-server/src/main.rs`, add these two routes to the `api_routes` Router, after the `/api/subjects/{id}/posts` route (before the `.layer(middleware::from_fn_with_state(` line):

```rust
        .route(
            "/api/insights/latest",
            get(routes::insights::get_latest),
        )
        .route(
            "/api/insights/generate",
            post(routes::insights::generate),
        )
```

- [ ] **Step 4: Verify it compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/src/routes/insights.rs postgraph-server/src/routes/mod.rs postgraph-server/src/main.rs
git commit -m "feat(insights): add GET /api/insights/latest and POST /api/insights/generate"
```

---

### Task 6: Nightly Sync Integration

**Files:**
- Modify: `postgraph-server/src/main.rs:232-235` (add insights generation after edge computation)

- [ ] **Step 1: Add insights generation to the nightly sync**

In `postgraph-server/src/main.rs`, after the edge computation block (after line 234: `tracing::error!("Nightly edge computation failed: {e}");` and its closing `}`), add before the `info!("Nightly sync complete");` line:

```rust
            // Generate insights report
            match insights::generate_report(
                &nightly_state.pool,
                &nightly_state.mercury,
                "nightly",
            )
            .await
            {
                Ok(r) => info!("Nightly insights report generated: {}", r.id),
                Err(e) => tracing::error!("Nightly insights generation failed: {e}"),
            }
```

- [ ] **Step 2: Add the insights import**

At the top of `main.rs`, the `use` for insights should already be available since it's declared as `mod insights;`. No additional import needed — the module is accessed as `insights::generate_report`.

- [ ] **Step 3: Verify it compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/main.rs
git commit -m "feat(insights): add nightly insights generation after sync"
```

---

### Task 7: Frontend Proxy Routes

**Files:**
- Create: `web/src/routes/api/insights/latest/+server.ts`
- Create: `web/src/routes/api/insights/generate/+server.ts`

SvelteKit proxy routes that forward to the Rust backend with auth.

- [ ] **Step 1: Create the GET proxy for latest**

Create `web/src/routes/api/insights/latest/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  return proxyToBackend('/api/insights/latest');
};
```

- [ ] **Step 2: Create the POST proxy for generate**

Create `web/src/routes/api/insights/generate/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async () => {
  return proxyToBackend('/api/insights/generate', { method: 'POST' });
};
```

- [ ] **Step 3: Verify the frontend builds**

Run: `cd "/Users/dennis/programming projects/postgraph/web" && npx svelte-check`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add web/src/routes/api/insights/
git commit -m "feat(insights): add frontend proxy routes for insights API"
```

---

### Task 8: Frontend API Client

**Files:**
- Modify: `web/src/lib/api.ts` (add types and API methods)

- [ ] **Step 1: Add the TypeScript types**

In `web/src/lib/api.ts`, add these interfaces after the existing type definitions (before the `export const api = {` line):

```typescript
export interface InsightsItem {
  observation: string;
  cited_posts: string[];
  tone: 'positive' | 'negative' | 'neutral';
}

export interface InsightsSection {
  key: string;
  title: string;
  summary: string;
  items: InsightsItem[];
}

export interface InsightsReport {
  headline: string;
  sections: InsightsSection[];
}

export interface InsightsResponse {
  id: string;
  generated_at: string;
  trigger_type: string;
  report: InsightsReport;
}
```

- [ ] **Step 2: Add the API methods**

In the `api` object, add these methods (after the last existing method, before the closing `};`):

```typescript
  getInsightsLatest: () => fetchApi<InsightsResponse>('/api/insights/latest'),
  generateInsights: () => fetch('/api/insights/generate', {
    method: 'POST',
  }).then(async r => {
    if (!r.ok) {
      const body = await r.json().catch(() => ({ error: `Generate failed (${r.status})` }));
      throw new Error(body.error ?? `Generate failed (${r.status})`);
    }
    return r.json() as Promise<InsightsResponse>;
  }),
```

- [ ] **Step 3: Verify the frontend builds**

Run: `cd "/Users/dennis/programming projects/postgraph/web" && npx svelte-check`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add web/src/lib/api.ts
git commit -m "feat(insights): add insights types and API client methods"
```

---

### Task 9: Insights Svelte Component

**Files:**
- Create: `web/src/lib/components/Insights.svelte`

The main component with 2x2 grid, headline, regenerate button, and all states (loading, empty, error, regenerating).

- [ ] **Step 1: Create the Insights component**

Create `web/src/lib/components/Insights.svelte`:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { InsightsResponse, InsightsSection, Post } from '$lib/api';

  let report: InsightsResponse | null = null;
  let posts: Post[] = [];
  let loading = true;
  let regenerating = false;
  let error = '';

  const SECTION_STYLES: Record<string, { color: string; icon: string; border: string }> = {
    working: { color: '#4ade80', icon: '●', border: '#1a3a1a' },
    not_working: { color: '#f87171', icon: '●', border: '#3a1a1a' },
    on_brand: { color: '#60a5fa', icon: '●', border: '#1a2a3a' },
    off_pattern: { color: '#facc15', icon: '●', border: '#3a2a1a' },
  };

  function timeAgo(dateStr: string): string {
    const diff = Date.now() - new Date(dateStr).getTime();
    const hours = Math.floor(diff / 3600000);
    if (hours < 1) return 'just now';
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  }

  function getPostById(id: string): Post | undefined {
    return posts.find(p => p.id === id);
  }

  function truncate(text: string, len: number): string {
    if (text.length <= len) return text;
    return text.slice(0, len).trimEnd() + '…';
  }

  async function loadReport() {
    try {
      const [reportData, postsData] = await Promise.all([
        api.getInsightsLatest().catch(() => null),
        api.getPosts(),
      ]);
      report = reportData;
      posts = postsData;
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load insights';
    } finally {
      loading = false;
    }
  }

  async function regenerate() {
    regenerating = true;
    error = '';
    try {
      report = await api.generateInsights();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to generate insights';
    } finally {
      regenerating = false;
    }
  }

  onMount(loadReport);
</script>

<div class="insights">
  {#if loading}
    <div class="header">
      <div>
        <h2>Monthly Insights</h2>
        <span class="subtitle">Loading...</span>
      </div>
    </div>
    <div class="grid">
      {#each Array(4) as _}
        <div class="card skeleton"></div>
      {/each}
    </div>
  {:else if error && !report}
    <div class="header">
      <div>
        <h2>Monthly Insights</h2>
      </div>
    </div>
    <div class="empty">
      <p>{error}</p>
    </div>
  {:else if !report}
    <div class="header">
      <div>
        <h2>Monthly Insights</h2>
        <span class="subtitle">No report generated yet</span>
      </div>
    </div>
    <div class="empty">
      <p>No insights have been generated yet.</p>
      <button class="generate-btn" onclick={regenerate} disabled={regenerating}>
        {regenerating ? 'Generating...' : 'Generate Now'}
      </button>
    </div>
  {:else}
    <div class="header">
      <div>
        <h2>Monthly Insights</h2>
        <span class="subtitle">
          {report.report.sections.length ? '' : ''}Generated {timeAgo(report.generated_at)}
          · {report.trigger_type === 'nightly' ? 'Auto' : 'Manual'}
        </span>
      </div>
      <button class="regen-btn" onclick={regenerate} disabled={regenerating}>
        {regenerating ? 'Generating...' : '↻ Regenerate'}
      </button>
    </div>

    {#if error}
      <div class="error-toast">{error}</div>
    {/if}

    <div class="headline">
      {report.report.headline}
    </div>

    <div class="grid" class:regenerating>
      {#each report.report.sections as section}
        {@const style = SECTION_STYLES[section.key] ?? SECTION_STYLES.working}
        <div class="card" style="border-color: {style.border}">
          <div class="card-header">
            <span class="card-icon" style="color: {style.color}">{style.icon}</span>
            <span class="card-title" style="color: {style.color}">{section.title}</span>
          </div>
          <p class="card-summary">{section.summary}</p>
          {#if section.items.length > 0}
            <div class="card-items">
              {#each section.items as item}
                <div class="item">
                  <p class="observation">{item.observation}</p>
                  {#each item.cited_posts as postId}
                    {@const post = getPostById(postId)}
                    {#if post}
                      <a
                        class="cited-post"
                        href={post.permalink ?? '#'}
                        target="_blank"
                        rel="noopener noreferrer"
                        style="color: {style.color}"
                      >
                        → {truncate(post.text ?? '(no text)', 80)} · {post.views.toLocaleString()} views
                      </a>
                    {/if}
                  {/each}
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .insights {
    max-width: 900px;
    margin: 0 auto;
    padding: 24px;
  }
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 20px;
  }
  h2 {
    font-size: 20px;
    font-weight: 600;
    color: #fff;
    margin: 0;
  }
  .subtitle {
    font-size: 13px;
    color: #888;
  }
  .regen-btn {
    background: #222;
    border: 1px solid #333;
    color: #ccc;
    padding: 8px 16px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }
  .regen-btn:hover {
    background: #2a2a2a;
    border-color: #444;
  }
  .regen-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .headline {
    background: #111;
    border: 1px solid #222;
    border-radius: 8px;
    padding: 16px 20px;
    margin-bottom: 20px;
    font-size: 15px;
    color: #ddd;
    line-height: 1.5;
    font-weight: 500;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    transition: opacity 0.3s;
  }
  .grid.regenerating {
    opacity: 0.4;
    pointer-events: none;
  }
  .card {
    background: #111;
    border: 1px solid #222;
    border-radius: 8px;
    padding: 20px;
    min-height: 180px;
  }
  .card.skeleton {
    animation: pulse 1.5s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.7; }
  }
  .card-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }
  .card-icon {
    font-size: 14px;
  }
  .card-title {
    font-size: 15px;
    font-weight: 600;
  }
  .card-summary {
    font-size: 13px;
    color: #bbb;
    line-height: 1.6;
    margin: 0 0 12px 0;
  }
  .card-items {
    border-top: 1px solid #222;
    padding-top: 10px;
  }
  .item {
    margin-bottom: 8px;
  }
  .observation {
    font-size: 13px;
    color: #aaa;
    margin: 0 0 4px 0;
  }
  .cited-post {
    display: block;
    font-size: 12px;
    text-decoration: none;
    margin-bottom: 4px;
  }
  .cited-post:hover {
    text-decoration: underline;
  }
  .empty {
    text-align: center;
    padding: 60px 20px;
    color: #888;
  }
  .generate-btn {
    margin-top: 16px;
    background: #333;
    border: 1px solid #444;
    color: #ddd;
    padding: 10px 24px;
    border-radius: 6px;
    font-size: 14px;
    cursor: pointer;
  }
  .generate-btn:hover {
    background: #3a3a3a;
  }
  .generate-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .error-toast {
    background: #2a1515;
    border: 1px solid #3a1a1a;
    color: #f87171;
    padding: 10px 16px;
    border-radius: 6px;
    font-size: 13px;
    margin-bottom: 16px;
  }

  @media (max-width: 640px) {
    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>
```

- [ ] **Step 2: Verify the frontend builds**

Run: `cd "/Users/dennis/programming projects/postgraph/web" && npx svelte-check`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/components/Insights.svelte
git commit -m "feat(insights): add Insights Svelte component with 2x2 grid layout"
```

---

### Task 10: Route & Navigation

**Files:**
- Create: `web/src/routes/insights/+page.svelte`
- Modify: `web/src/routes/+layout.svelte` (add nav link)

- [ ] **Step 1: Create the page route**

Create `web/src/routes/insights/+page.svelte`:

```svelte
<script lang="ts">
  import Insights from '$lib/components/Insights.svelte';
</script>

<Insights />
```

- [ ] **Step 2: Add the navigation link**

In `web/src/routes/+layout.svelte`, add the Insights link in the nav. After the V2 link (`<a href="/analytics-v2">V2</a>`), add:

```svelte
      <a href="/insights" class:active={$page.url.pathname === '/insights'}>Insights</a>
```

- [ ] **Step 3: Verify the frontend builds**

Run: `cd "/Users/dennis/programming projects/postgraph/web" && npx svelte-check`
Expected: no errors

- [ ] **Step 4: Verify the full backend compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles

- [ ] **Step 5: Commit**

```bash
git add web/src/routes/insights/ web/src/routes/+layout.svelte
git commit -m "feat(insights): add /insights route and navigation link"
```

---

### Task 11: Manual Smoke Test

**Files:** None (verification only)

- [ ] **Step 1: Run the backend**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo run --package postgraph-server`
Expected: server starts, migration 012 runs, no errors

- [ ] **Step 2: Run the frontend**

Run: `cd "/Users/dennis/programming projects/postgraph/web" && npm run dev`
Expected: dev server starts

- [ ] **Step 3: Navigate to /insights**

Open http://localhost:5173/insights in a browser. Expected: empty state with "No insights have been generated yet" and a "Generate Now" button.

- [ ] **Step 4: Click "Generate Now"**

Click the button. Expected: loading state, then after 5-15 seconds the 2x2 grid appears with Mercury's analysis. Cited posts should link to Threads.

- [ ] **Step 5: Verify the nav link**

Check that "Insights" appears in the top navigation bar and highlights when active.

- [ ] **Step 6: Final lint checks**

Run:
```bash
cd "/Users/dennis/programming projects/postgraph" && cargo fmt --all && cargo clippy --workspace --all-targets
cd "/Users/dennis/programming projects/postgraph/web" && npx svelte-check
```
Expected: no errors or warnings (fix any that appear)

- [ ] **Step 7: Final commit (if any lint fixes)**

```bash
git add -A
git commit -m "chore: lint fixes for insights tab"
```
