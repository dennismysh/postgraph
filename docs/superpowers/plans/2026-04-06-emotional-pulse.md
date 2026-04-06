# Emotional Pulse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add emotion classification per post and a radar chart + Mercury narrative section to the Insights page.

**Architecture:** Extend the existing Mercury analysis prompt to tag each post with one of 7 creator-oriented emotions. Store the emotion as a column on `posts`. A new set of `/api/emotions/` endpoints serves the aggregated radar chart data and Mercury-generated narrative. The frontend renders a radar chart (Chart.js) and narrative card below the existing insights grid.

**Tech Stack:** Rust (axum, sqlx), Mercury LLM (OpenAI-compatible), Svelte 5 (SvelteKit), Chart.js (radar), PostgreSQL

---

### Task 1: Database Migrations

**Files:**
- Create: `postgraph-server/migrations/013_add_emotion_column.sql`
- Create: `postgraph-server/migrations/014_emotion_narratives.sql`
- Modify: `postgraph-server/src/types.rs:6-24` (Post struct)

- [ ] **Step 1: Create migration to add emotion column to posts**

Create `postgraph-server/migrations/013_add_emotion_column.sql`:

```sql
ALTER TABLE posts ADD COLUMN emotion TEXT;
```

- [ ] **Step 2: Create migration for emotion_narratives table**

Create `postgraph-server/migrations/014_emotion_narratives.sql`:

```sql
CREATE TABLE emotion_narratives (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    trigger_type TEXT NOT NULL,
    narrative JSONB NOT NULL,
    context JSONB NOT NULL
);
```

- [ ] **Step 3: Add emotion field to Post struct**

In `postgraph-server/src/types.rs`, add `emotion` to the `Post` struct after the `sentiment` field:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Post {
    pub id: String,
    pub text: Option<String>,
    pub media_type: Option<String>,
    pub media_url: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub permalink: Option<String>,
    pub views: i32,
    pub likes: i32,
    pub replies_count: i32,
    pub reposts: i32,
    pub quotes: i32,
    pub shares: i32,
    pub intent_id: Option<Uuid>,
    pub subject_id: Option<Uuid>,
    pub sentiment: Option<f32>,
    pub emotion: Option<String>,
    pub synced_at: DateTime<Utc>,
    pub analyzed_at: Option<DateTime<Utc>>,
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: Compiles successfully (sqlx may warn about unchecked queries, that's fine).

- [ ] **Step 5: Commit**

```bash
git add postgraph-server/migrations/013_add_emotion_column.sql postgraph-server/migrations/014_emotion_narratives.sql postgraph-server/src/types.rs
git commit -m "feat(emotions): add emotion column and emotion_narratives table"
```

---

### Task 2: Extend Mercury Analysis Prompt

**Files:**
- Modify: `postgraph-server/src/mercury.rs:40-51` (AnalyzedPost struct)
- Modify: `postgraph-server/src/mercury.rs:105-147` (analyze_posts prompt)
- Modify: `postgraph-server/src/analysis.rs:53-65` (store emotion after analysis)
- Modify: `postgraph-server/src/db.rs:101-108` (mark_post_analyzed)

- [ ] **Step 1: Add emotion field to AnalyzedPost**

In `postgraph-server/src/mercury.rs`, update the `AnalyzedPost` struct:

```rust
#[derive(Debug, Deserialize)]
pub struct AnalyzedPost {
    pub post_id: String,
    pub intent: String,
    pub subject: String,
    pub sentiment: f32,
    pub emotion: String,
}
```

- [ ] **Step 2: Extend the analyze_posts prompt**

In `postgraph-server/src/mercury.rs`, update the prompt inside `analyze_posts` to add an Emotion section. Replace the existing prompt `format!` block (lines ~105-147) with:

```rust
        let prompt = format!(
            r#"You are analyzing social media posts for a content analytics platform.

For each post, extract:
1. **Intent** — what the post is trying to do (one per post)
2. **Subject** — what the post is about (one per post)
3. **Sentiment** — emotional tone (-1.0 to 1.0)
4. **Emotion** — the dominant emotional quality of the post (one per post)

## Intent (pick exactly one)
The communicative purpose of the post. Seed examples:
- Question: asking the audience something
- Hot take: strong opinion meant to provoke thought
- Humor: joke, wordplay, absurdist observation
- Story: personal anecdote or experience
- Tip: sharing something useful or instructional
- Hype: excitement, celebrating a win or milestone
- Rant: frustration, complaint, venting
- Observation: noticing something interesting, neutral tone
- Promotion: sharing own work, project, or product

You may create new intents if a post genuinely doesn't fit any of these, but apply the reusability test first.

## Subject (pick exactly one)
The topic domain of the post. Seed examples:
- AI & LLMs, Software dev, Side projects, Social media, Productivity, Daily life, Gaming, Career, Health, Culture, Tech industry, Politics

You may create new subjects at this same granularity level.

## Emotion (pick exactly one)
The dominant emotional quality of the post. Pick from this fixed list only:
- Vulnerable: openness, personal sharing, admitting uncertainty
- Curious: questions, exploration, wonder
- Playful: humor, wit, lightheartedness
- Confident: strong opinions, assertions, expertise
- Reflective: introspection, lessons learned, looking back
- Frustrated: venting, complaints, friction
- Provocative: hot takes, challenging norms, debate-starting

Always pick exactly one from this list. Do not create new emotions.

## Rules
1. REUSABILITY TEST: Before creating a new intent or subject, ask: "Would this apply to at least 10 posts from a typical creator?" If no, use a broader existing tag.
2. NO COMPOUND TAGS: "Coffee humor" is wrong. That's intent=Humor, subject=Daily life.
3. PREFER EXISTING: Always reuse an existing intent/subject before creating a new one.
4. SHORT NAMES: Max 3 words per tag.
5. NEVER describe a single post's specific content as a tag. "UNO house rules" → subject=Gaming, intent=Question. "Parking preference" → subject=Daily life, intent=Question.
6. EMOTION IS FIXED: Only use one of the 7 listed emotions. Never invent new ones.

Existing intents: {intents_list}
Existing subjects: {subjects_list}

Posts: {posts_json_str}

Respond with ONLY valid JSON:
{{"posts": [{{"post_id": "...", "intent": "...", "subject": "...", "sentiment": 0.5, "emotion": "curious"}}]}}"#
        );
```

- [ ] **Step 3: Update db::mark_post_analyzed to accept and store emotion**

In `postgraph-server/src/db.rs`, update `mark_post_analyzed`:

```rust
pub async fn mark_post_analyzed(pool: &PgPool, post_id: &str, sentiment: f32, emotion: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE posts SET analyzed_at = NOW(), sentiment = $1, emotion = $2 WHERE id = $3")
        .bind(sentiment)
        .bind(emotion)
        .bind(post_id)
        .execute(pool)
        .await?;
    Ok(())
}
```

- [ ] **Step 4: Update analysis.rs to pass emotion through**

In `postgraph-server/src/analysis.rs`, update the loop in `run_analysis` that processes results. Change the `mark_post_analyzed` call:

```rust
        db::mark_post_analyzed(pool, &result.post_id, result.sentiment, &result.emotion).await?;
```

- [ ] **Step 5: Update the media-only fallback in analysis.rs**

In `postgraph-server/src/analysis.rs`, the media-only fallback (line ~35) also calls `mark_post_analyzed`. Update it:

```rust
        for post in &unanalyzed {
            db::mark_post_analyzed(pool, &post.id, 0.0, "reflective").await?;
        }
```

- [ ] **Step 6: Update reset_all_analysis to clear emotion**

In `postgraph-server/src/db.rs`, update the `reset_all_analysis` function's UPDATE query:

```rust
    let result = sqlx::query(
        "UPDATE posts SET analyzed_at = NULL, sentiment = NULL, intent_id = NULL, subject_id = NULL, emotion = NULL",
    )
    .execute(pool)
    .await?;
```

- [ ] **Step 7: Verify it compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: Compiles successfully.

- [ ] **Step 8: Commit**

```bash
git add postgraph-server/src/mercury.rs postgraph-server/src/analysis.rs postgraph-server/src/db.rs
git commit -m "feat(emotions): extend analysis prompt with emotion classification"
```

---

### Task 3: Emotions Summary Endpoint

**Files:**
- Create: `postgraph-server/src/emotions.rs`
- Create: `postgraph-server/src/routes/emotions.rs`
- Modify: `postgraph-server/src/routes/mod.rs`
- Modify: `postgraph-server/src/main.rs` (module declaration + route registration)

- [ ] **Step 1: Create the emotions module**

Create `postgraph-server/src/emotions.rs`:

```rust
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;

use crate::error::AppError;
use crate::mercury::MercuryClient;

pub const EMOTIONS: &[&str] = &[
    "vulnerable",
    "curious",
    "playful",
    "confident",
    "reflective",
    "frustrated",
    "provocative",
];

// ── Summary Types ───────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct EmotionStat {
    pub name: String,
    pub post_count: i64,
    pub percentage: f64,
    pub avg_views: f64,
    pub avg_likes: f64,
    pub avg_replies: f64,
    pub avg_reposts: f64,
    pub top_post_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmotionsSummary {
    pub window_start: String,
    pub window_end: String,
    pub total_posts: i64,
    pub emotions: Vec<EmotionStat>,
}

// ── Narrative Types ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct EmotionObservation {
    pub text: String,
    pub cited_posts: Vec<String>,
    pub emotion: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmotionNarrative {
    pub headline: String,
    pub observations: Vec<EmotionObservation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredNarrative {
    pub id: String,
    pub generated_at: chrono::DateTime<Utc>,
    pub trigger_type: String,
    pub narrative: EmotionNarrative,
}

// ── compute_summary ─────────────────────────────────────────────────

pub async fn compute_summary(pool: &PgPool) -> Result<EmotionsSummary, AppError> {
    let now = Utc::now();
    let window_start = now - chrono::Duration::days(30);

    let total_row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM posts WHERE emotion IS NOT NULL AND timestamp >= $1",
    )
    .bind(window_start)
    .fetch_one(pool)
    .await?;
    let total_posts = total_row.0;

    let rows: Vec<(String, i64, f64, f64, f64, f64)> = sqlx::query_as(
        r#"SELECT
               emotion,
               COUNT(*) AS post_count,
               COALESCE(AVG(views::float8), 0.0) AS avg_views,
               COALESCE(AVG(likes::float8), 0.0) AS avg_likes,
               COALESCE(AVG(replies_count::float8), 0.0) AS avg_replies,
               COALESCE(AVG(reposts::float8), 0.0) AS avg_reposts
           FROM posts
           WHERE emotion IS NOT NULL AND timestamp >= $1
           GROUP BY emotion
           ORDER BY post_count DESC"#,
    )
    .bind(window_start)
    .fetch_all(pool)
    .await?;

    // For each emotion, find the top post by views
    let mut emotions = Vec::new();
    for (name, post_count, avg_views, avg_likes, avg_replies, avg_reposts) in rows {
        let top_post: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM posts WHERE emotion = $1 AND timestamp >= $2 ORDER BY views DESC LIMIT 1",
        )
        .bind(&name)
        .bind(window_start)
        .fetch_optional(pool)
        .await?;

        let percentage = if total_posts > 0 {
            (post_count as f64 / total_posts as f64) * 100.0
        } else {
            0.0
        };

        emotions.push(EmotionStat {
            name,
            post_count,
            percentage,
            avg_views,
            avg_likes,
            avg_replies,
            avg_reposts,
            top_post_id: top_post.map(|(id,)| id),
        });
    }

    Ok(EmotionsSummary {
        window_start: window_start.format("%Y-%m-%d").to_string(),
        window_end: now.format("%Y-%m-%d").to_string(),
        total_posts,
        emotions,
    })
}

// ── generate_narrative ──────────────────────────────────────────────

pub async fn generate_narrative(
    pool: &PgPool,
    mercury: &MercuryClient,
    trigger_type: &str,
) -> Result<StoredNarrative, AppError> {
    let summary = compute_summary(pool).await?;

    if summary.total_posts < 5 {
        return Err(AppError::MercuryApi(format!(
            "Insufficient data: need at least 5 posts with emotions in the last 30 days, found {}",
            summary.total_posts
        )));
    }

    info!(
        "Generating emotion narrative (trigger={trigger_type}, posts={})",
        summary.total_posts
    );

    let narrative = mercury.generate_emotion_narrative(&summary).await?;

    let context_json = serde_json::to_value(&summary)?;
    let narrative_json = serde_json::to_value(&narrative)?;

    let row: (uuid::Uuid, chrono::DateTime<Utc>) = sqlx::query_as(
        r#"INSERT INTO emotion_narratives (trigger_type, narrative, context)
           VALUES ($1, $2, $3)
           RETURNING id, generated_at"#,
    )
    .bind(trigger_type)
    .bind(&narrative_json)
    .bind(&context_json)
    .fetch_one(pool)
    .await?;

    info!("Stored emotion narrative id={}", row.0);

    Ok(StoredNarrative {
        id: row.0.to_string(),
        generated_at: row.1,
        trigger_type: trigger_type.to_string(),
        narrative,
    })
}

// ── get_latest_narrative ────────────────────────────────────────────

pub async fn get_latest_narrative(pool: &PgPool) -> Result<Option<StoredNarrative>, AppError> {
    let row: Option<(uuid::Uuid, chrono::DateTime<Utc>, String, serde_json::Value)> =
        sqlx::query_as(
            r#"SELECT id, generated_at, trigger_type, narrative
               FROM emotion_narratives
               ORDER BY generated_at DESC
               LIMIT 1"#,
        )
        .fetch_optional(pool)
        .await?;

    match row {
        None => Ok(None),
        Some((id, generated_at, trigger_type, narrative_json)) => {
            let narrative: EmotionNarrative = serde_json::from_value(narrative_json)?;
            Ok(Some(StoredNarrative {
                id: id.to_string(),
                generated_at,
                trigger_type,
                narrative,
            }))
        }
    }
}
```

- [ ] **Step 2: Create the emotions route handler**

Create `postgraph-server/src/routes/emotions.rs`:

```rust
use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::emotions;
use crate::state::AppState;

#[derive(Serialize)]
pub struct EmotionsSummaryResponse {
    pub window_start: String,
    pub window_end: String,
    pub total_posts: i64,
    pub emotions: Vec<emotions::EmotionStat>,
}

#[derive(Serialize)]
pub struct NarrativeResponse {
    pub id: String,
    pub generated_at: String,
    pub trigger_type: String,
    pub narrative: emotions::EmotionNarrative,
}

#[derive(Serialize)]
pub struct EmotionsError {
    pub error: String,
}

pub async fn get_summary(
    State(state): State<AppState>,
) -> Result<Json<EmotionsSummaryResponse>, (axum::http::StatusCode, Json<EmotionsError>)> {
    let summary = emotions::compute_summary(&state.pool).await.map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(EmotionsError {
                error: e.to_string(),
            }),
        )
    })?;

    Ok(Json(EmotionsSummaryResponse {
        window_start: summary.window_start,
        window_end: summary.window_end,
        total_posts: summary.total_posts,
        emotions: summary.emotions,
    }))
}

pub async fn get_narrative(
    State(state): State<AppState>,
) -> Result<Json<NarrativeResponse>, (axum::http::StatusCode, Json<EmotionsError>)> {
    let narrative = emotions::get_latest_narrative(&state.pool)
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(EmotionsError {
                    error: e.to_string(),
                }),
            )
        })?;

    match narrative {
        Some(n) => Ok(Json(NarrativeResponse {
            id: n.id,
            generated_at: n.generated_at.to_rfc3339(),
            trigger_type: n.trigger_type,
            narrative: n.narrative,
        })),
        None => Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(EmotionsError {
                error: "No emotion narrative generated yet".to_string(),
            }),
        )),
    }
}

pub async fn generate_narrative(
    State(state): State<AppState>,
) -> Result<Json<NarrativeResponse>, (axum::http::StatusCode, Json<EmotionsError>)> {
    let narrative = emotions::generate_narrative(&state.pool, &state.mercury, "manual")
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(EmotionsError {
                    error: e.to_string(),
                }),
            )
        })?;

    Ok(Json(NarrativeResponse {
        id: narrative.id,
        generated_at: narrative.generated_at.to_rfc3339(),
        trigger_type: narrative.trigger_type,
        narrative: narrative.narrative,
    }))
}
```

- [ ] **Step 3: Register the emotions module and routes**

In `postgraph-server/src/routes/mod.rs`, add:

```rust
pub mod emotions;
```

In `postgraph-server/src/main.rs`, add the module declaration alongside the others:

```rust
mod emotions;
```

In `postgraph-server/src/main.rs`, add routes to the `api_routes` Router, after the insights routes (before the `.layer(middleware...)` call):

```rust
        .route("/api/emotions/summary", get(routes::emotions::get_summary))
        .route("/api/emotions/narrative", get(routes::emotions::get_narrative))
        .route("/api/emotions/narrative/generate", post(routes::emotions::generate_narrative))
```

- [ ] **Step 4: Skip compilation check — depends on Task 4 (mercury method not yet created)**

---

### Task 4: Mercury Emotion Narrative Generation

**Files:**
- Modify: `postgraph-server/src/mercury.rs` (add `generate_emotion_narrative` method)

- [ ] **Step 1: Add the generate_emotion_narrative method to MercuryClient**

In `postgraph-server/src/mercury.rs`, add this method to the `impl MercuryClient` block, after the existing `generate_insights` method. Add the necessary import at the top of the file:

```rust
use crate::emotions::{EmotionNarrative, EmotionsSummary};
```

Then add the method:

```rust
    pub async fn generate_emotion_narrative(
        &self,
        summary: &EmotionsSummary,
    ) -> Result<EmotionNarrative, AppError> {
        let context_json = serde_json::to_string_pretty(summary)?;

        let system_prompt = r#"You are a candid friend who is also a world-class content strategist. You've just reviewed 30 days of someone's social media posts, classified by emotional tone, alongside their engagement data (views, likes, replies, reposts). You care about this person's growth and you're direct, specific, and grounded in the numbers.

Respond with ONLY valid JSON matching this exact structure:
{
  "headline": "<one punchy sentence capturing the most important emotion-engagement insight>",
  "observations": [
    {
      "text": "<specific, data-grounded observation about how an emotion correlates with audience response>",
      "cited_posts": ["<post id>", ...],
      "emotion": "<emotion name>"
    }
  ]
}

Rules:
- Return exactly 3-5 observations.
- Focus on the creator-audience relationship: which emotions resonate, which fall flat, which get reach but not engagement (or vice versa).
- Compare emotions against each other: "Your curious posts get 2x the views of your confident posts."
- Comment on emotional range: is the creator one-note or diverse? Is that helping or hurting?
- cited_posts should reference actual post IDs from the top_post_id fields when available.
- The headline should sound like something a friend would say, not a corporate summary.
- Be specific: cite numbers, percentages, emotion names. Avoid vague statements.
- Do not wrap JSON in markdown code fences."#;

        let request = ChatRequest {
            model: "mercury-2".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: format!(
                        "Here is the emotion breakdown for the last 30 days:\n\n{context_json}"
                    ),
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

        let narrative: EmotionNarrative = serde_json::from_str(json_str).map_err(|e| {
            AppError::MercuryApi(format!("Failed to parse emotion narrative: {e}. Raw: {json_str}"))
        })?;

        Ok(narrative)
    }
```

- [ ] **Step 2: Verify full compilation**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: Compiles successfully.

- [ ] **Step 3: Commit Tasks 3 + 4 together**

```bash
git add postgraph-server/src/emotions.rs postgraph-server/src/routes/emotions.rs postgraph-server/src/routes/mod.rs postgraph-server/src/main.rs postgraph-server/src/mercury.rs
git commit -m "feat(emotions): add emotions module, endpoints, and Mercury narrative generation"
```

---

### Task 5: Backfill Endpoint

**Files:**
- Modify: `postgraph-server/src/mercury.rs` (add `classify_emotions` method)
- Modify: `postgraph-server/src/emotions.rs` (add `backfill_emotions` function)
- Modify: `postgraph-server/src/routes/emotions.rs` (add `backfill` handler)
- Modify: `postgraph-server/src/main.rs` (register backfill route)

- [ ] **Step 1: Add a lightweight classify_emotions method to MercuryClient**

In `postgraph-server/src/mercury.rs`, add a struct and method. The struct goes near the existing `AnalyzedPost`:

```rust
#[derive(Debug, Deserialize)]
pub struct ClassifiedEmotion {
    pub post_id: String,
    pub emotion: String,
}

#[derive(Debug, Deserialize)]
struct EmotionClassificationResponse {
    pub posts: Vec<ClassifiedEmotion>,
}
```

Add this method to `impl MercuryClient`:

```rust
    pub async fn classify_emotions(
        &self,
        posts: &[(String, String)],
    ) -> Result<Vec<ClassifiedEmotion>, AppError> {
        let posts_json: Vec<serde_json::Value> = posts
            .iter()
            .map(|(id, text)| serde_json::json!({"id": id, "text": text}))
            .collect();
        let posts_json_str = serde_json::to_string_pretty(&posts_json).unwrap_or_default();

        let prompt = format!(
            r#"Classify the dominant emotion of each social media post. Pick exactly one from this fixed list:
- Vulnerable: openness, personal sharing, admitting uncertainty
- Curious: questions, exploration, wonder
- Playful: humor, wit, lightheartedness
- Confident: strong opinions, assertions, expertise
- Reflective: introspection, lessons learned, looking back
- Frustrated: venting, complaints, friction
- Provocative: hot takes, challenging norms, debate-starting

Posts: {posts_json_str}

Respond with ONLY valid JSON:
{{"posts": [{{"post_id": "...", "emotion": "curious"}}]}}"#
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

        let result: EmotionClassificationResponse =
            serde_json::from_str(json_str).map_err(|e| {
                AppError::MercuryApi(format!(
                    "Failed to parse emotion classification: {e}. Raw: {json_str}"
                ))
            })?;

        Ok(result.posts)
    }
```

- [ ] **Step 2: Add backfill_emotions function to emotions.rs**

In `postgraph-server/src/emotions.rs`, add at the bottom:

```rust
// ── backfill_emotions ───────────────────────────────────────────────

fn backfill_batch_size() -> i64 {
    std::env::var("ANALYSIS_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16)
}

pub async fn backfill_emotions(
    pool: &PgPool,
    mercury: &MercuryClient,
) -> Result<u32, AppError> {
    let batch_size = backfill_batch_size();
    let mut total_classified: u32 = 0;

    loop {
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            r#"SELECT id, text FROM posts
               WHERE analyzed_at IS NOT NULL AND emotion IS NULL AND text IS NOT NULL
               ORDER BY timestamp DESC
               LIMIT $1"#,
        )
        .bind(batch_size)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let posts_for_llm: Vec<(String, String)> = rows
            .into_iter()
            .filter_map(|(id, text)| text.map(|t| (id, t)))
            .collect();

        if posts_for_llm.is_empty() {
            break;
        }

        info!(
            "Backfilling emotions for {} posts",
            posts_for_llm.len()
        );

        let results = mercury.classify_emotions(&posts_for_llm).await?;

        for result in &results {
            let emotion = result.emotion.to_lowercase();
            if EMOTIONS.contains(&emotion.as_str()) {
                sqlx::query("UPDATE posts SET emotion = $1 WHERE id = $2")
                    .bind(&emotion)
                    .bind(&result.post_id)
                    .execute(pool)
                    .await?;
                total_classified += 1;
            } else {
                tracing::warn!(
                    "Mercury returned unknown emotion '{}' for post {}, skipping",
                    result.emotion,
                    result.post_id
                );
            }
        }

        info!("Backfill batch complete: {} classified so far", total_classified);
    }

    info!("Emotion backfill complete: {} total posts classified", total_classified);
    Ok(total_classified)
}
```

- [ ] **Step 3: Add backfill route handler**

In `postgraph-server/src/routes/emotions.rs`, add:

```rust
#[derive(Serialize)]
pub struct BackfillResponse {
    pub classified: u32,
}

pub async fn backfill(
    State(state): State<AppState>,
) -> Result<Json<BackfillResponse>, (axum::http::StatusCode, Json<EmotionsError>)> {
    let classified = emotions::backfill_emotions(&state.pool, &state.mercury)
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(EmotionsError {
                    error: e.to_string(),
                }),
            )
        })?;

    Ok(Json(BackfillResponse { classified }))
}
```

- [ ] **Step 4: Register backfill route**

In `postgraph-server/src/main.rs`, add to the `api_routes` alongside the other emotion routes:

```rust
        .route("/api/emotions/backfill", post(routes::emotions::backfill))
```

- [ ] **Step 5: Verify it compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: Compiles successfully.

- [ ] **Step 6: Commit**

```bash
git add postgraph-server/src/mercury.rs postgraph-server/src/emotions.rs postgraph-server/src/routes/emotions.rs postgraph-server/src/main.rs
git commit -m "feat(emotions): add backfill endpoint for existing posts"
```

---

### Task 6: Nightly Emotion Narrative Generation

**Files:**
- Modify: `postgraph-server/src/main.rs` (nightly sync block, ~lines 237-243)

- [ ] **Step 1: Add emotion narrative generation after insights in nightly sync**

In `postgraph-server/src/main.rs`, in the nightly sync `tokio::spawn` block, add after the insights report generation (after the `match insights::generate_report(...)` block, before `info!("Nightly sync complete")`):

```rust
            // Generate emotion narrative
            match emotions::generate_narrative(&nightly_state.pool, &nightly_state.mercury, "nightly")
                .await
            {
                Ok(n) => info!("Nightly emotion narrative generated: {}", n.id),
                Err(e) => tracing::error!("Nightly emotion narrative generation failed: {e}"),
            }
```

- [ ] **Step 2: Verify it compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: Compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/main.rs
git commit -m "feat(emotions): add nightly emotion narrative generation"
```

---

### Task 7: Frontend Proxy Routes and API Client

**Files:**
- Create: `web/src/routes/api/emotions/summary/+server.ts`
- Create: `web/src/routes/api/emotions/narrative/+server.ts`
- Create: `web/src/routes/api/emotions/narrative/generate/+server.ts`
- Create: `web/src/routes/api/emotions/backfill/+server.ts`
- Modify: `web/src/lib/api.ts` (add types and API methods)

- [ ] **Step 1: Create proxy route for emotions summary**

Create `web/src/routes/api/emotions/summary/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  return proxyToBackend('/api/emotions/summary');
};
```

- [ ] **Step 2: Create proxy route for emotion narrative (GET)**

Create `web/src/routes/api/emotions/narrative/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  return proxyToBackend('/api/emotions/narrative');
};
```

- [ ] **Step 3: Create proxy route for narrative generation (POST)**

Create `web/src/routes/api/emotions/narrative/generate/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async () => {
  return proxyToBackend('/api/emotions/narrative/generate', { method: 'POST' });
};
```

- [ ] **Step 4: Create proxy route for backfill (POST)**

Create `web/src/routes/api/emotions/backfill/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async () => {
  return proxyToBackend('/api/emotions/backfill', { method: 'POST' });
};
```

- [ ] **Step 5: Add TypeScript types to api.ts**

In `web/src/lib/api.ts`, add these interfaces after the existing `InsightsResponse` interface (around line 235):

```typescript
export interface EmotionStat {
  name: string;
  post_count: number;
  percentage: number;
  avg_views: number;
  avg_likes: number;
  avg_replies: number;
  avg_reposts: number;
  top_post_id: string | null;
}

export interface EmotionsSummaryResponse {
  window_start: string;
  window_end: string;
  total_posts: number;
  emotions: EmotionStat[];
}

export interface EmotionObservation {
  text: string;
  cited_posts: string[];
  emotion: string;
}

export interface EmotionNarrative {
  headline: string;
  observations: EmotionObservation[];
}

export interface EmotionNarrativeResponse {
  id: string;
  generated_at: string;
  trigger_type: string;
  narrative: EmotionNarrative;
}
```

- [ ] **Step 6: Add API methods to the api object**

In `web/src/lib/api.ts`, add these methods to the `api` object (before the closing `};`):

```typescript
  getEmotionsSummary: () => fetchApi<EmotionsSummaryResponse>('/api/emotions/summary'),
  getEmotionNarrative: () => fetchApi<EmotionNarrativeResponse>('/api/emotions/narrative'),
  generateEmotionNarrative: () => fetch('/api/emotions/narrative/generate', {
    method: 'POST',
  }).then(async r => {
    if (!r.ok) {
      const body = await r.json().catch(() => ({ error: `Generate failed (${r.status})` }));
      throw new Error(body.error ?? `Generate failed (${r.status})`);
    }
    return r.json() as Promise<EmotionNarrativeResponse>;
  }),
  backfillEmotions: () => fetch('/api/emotions/backfill', {
    method: 'POST',
  }).then(async r => {
    if (!r.ok) {
      const body = await r.json().catch(() => ({ error: `Backfill failed (${r.status})` }));
      throw new Error(body.error ?? `Backfill failed (${r.status})`);
    }
    return r.json() as Promise<{ classified: number }>;
  }),
```

- [ ] **Step 7: Verify frontend type checks**

Run: `cd "/Users/dennis/programming projects/postgraph/web" && npx svelte-check`
Expected: No errors.

- [ ] **Step 8: Commit**

```bash
git add web/src/routes/api/emotions/ web/src/lib/api.ts
git commit -m "feat(emotions): add frontend proxy routes and API client"
```

---

### Task 8: Emotional Pulse Svelte Component

**Files:**
- Create: `web/src/lib/components/EmotionalPulse.svelte`
- Modify: `web/src/lib/components/Insights.svelte` (import and render EmotionalPulse below existing grid)

- [ ] **Step 1: Create the EmotionalPulse component**

Create `web/src/lib/components/EmotionalPulse.svelte`:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { EmotionsSummaryResponse, EmotionNarrativeResponse, Post } from '$lib/api';
  import { Chart, RadarController, RadialLinearScale, PointElement, LineElement, Filler, Tooltip } from 'chart.js';

  Chart.register(RadarController, RadialLinearScale, PointElement, LineElement, Filler, Tooltip);

  let summary: EmotionsSummaryResponse | null = $state(null);
  let narrative: EmotionNarrativeResponse | null = $state(null);
  let posts: Post[] = $state([]);
  let loading = $state(true);
  let regenerating = $state(false);
  let error = $state('');
  let canvas: HTMLCanvasElement;
  let chart: Chart | null = $state(null);

  const EMOTION_COLORS: Record<string, string> = {
    vulnerable: '#c084fc',
    curious: '#60a5fa',
    playful: '#4ade80',
    confident: '#facc15',
    reflective: '#a78bfa',
    frustrated: '#f87171',
    provocative: '#fb923c',
  };

  const EMOTION_ORDER = ['vulnerable', 'curious', 'playful', 'confident', 'reflective', 'frustrated', 'provocative'];

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
    return text.slice(0, len).trimEnd() + '\u2026';
  }

  function fetchWithTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
    return Promise.race([
      promise,
      new Promise<T>((_, reject) => setTimeout(() => reject(new Error('Timeout')), ms)),
    ]);
  }

  function buildChart() {
    if (!canvas || !summary) return;
    if (chart) chart.destroy();

    const labels = EMOTION_ORDER.map(e => e.charAt(0).toUpperCase() + e.slice(1));
    const data = EMOTION_ORDER.map(e => {
      const stat = summary!.emotions.find(s => s.name.toLowerCase() === e);
      return stat ? stat.percentage : 0;
    });

    chart = new Chart(canvas, {
      type: 'radar',
      data: {
        labels,
        datasets: [{
          data,
          backgroundColor: 'rgba(96, 165, 250, 0.15)',
          borderColor: 'rgba(96, 165, 250, 0.8)',
          borderWidth: 2,
          pointBackgroundColor: EMOTION_ORDER.map(e => EMOTION_COLORS[e]),
          pointBorderColor: EMOTION_ORDER.map(e => EMOTION_COLORS[e]),
          pointRadius: 5,
          pointHoverRadius: 7,
        }],
      },
      options: {
        responsive: true,
        maintainAspectRatio: true,
        plugins: {
          legend: { display: false },
          tooltip: {
            callbacks: {
              label: (ctx) => {
                const emotion = EMOTION_ORDER[ctx.dataIndex];
                const stat = summary!.emotions.find(s => s.name.toLowerCase() === emotion);
                if (!stat) return `${ctx.parsed.r.toFixed(1)}%`;
                return `${ctx.parsed.r.toFixed(1)}% (${stat.post_count} posts, ${Math.round(stat.avg_views)} avg views)`;
              },
            },
          },
        },
        scales: {
          r: {
            beginAtZero: true,
            grid: { color: 'rgba(255, 255, 255, 0.08)' },
            angleLines: { color: 'rgba(255, 255, 255, 0.08)' },
            pointLabels: {
              color: '#999',
              font: { size: 12 },
            },
            ticks: {
              display: false,
            },
          },
        },
      },
    });
  }

  async function loadData() {
    try {
      summary = await fetchWithTimeout(api.getEmotionsSummary(), 10000);
    } catch {
      summary = null;
    }

    try {
      narrative = await fetchWithTimeout(api.getEmotionNarrative(), 10000);
    } catch {
      narrative = null;
    }

    if (narrative) {
      try {
        posts = await fetchWithTimeout(api.getPosts(), 10000);
      } catch {
        posts = [];
      }
    }

    loading = false;
  }

  async function regenerate() {
    regenerating = true;
    error = '';
    try {
      narrative = await api.generateEmotionNarrative();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to generate narrative';
    } finally {
      regenerating = false;
    }
  }

  onMount(() => {
    loadData();
  });

  $effect(() => {
    if (summary && canvas) {
      buildChart();
    }
  });
</script>

<div class="emotional-pulse">
  {#if loading}
    <div class="header">
      <div>
        <h3>Emotional Pulse</h3>
        <span class="subtitle">Loading...</span>
      </div>
    </div>
    <div class="skeleton-block"></div>
  {:else if !summary || summary.total_posts === 0}
    <div class="header">
      <div>
        <h3>Emotional Pulse</h3>
        <span class="subtitle">No emotion data yet</span>
      </div>
    </div>
    <div class="empty">
      <p>Posts haven't been classified with emotions yet.</p>
    </div>
  {:else}
    <div class="header">
      <div>
        <h3>Emotional Pulse</h3>
        <span class="subtitle">
          {summary.total_posts} posts · last 30 days
        </span>
      </div>
      {#if narrative}
        <button class="regen-btn" onclick={regenerate} disabled={regenerating}>
          {regenerating ? 'Generating...' : '\u21BB Regenerate'}
        </button>
      {/if}
    </div>

    {#if error}
      <div class="error-toast">{error}</div>
    {/if}

    <div class="content" class:regenerating>
      <div class="chart-container">
        <canvas bind:this={canvas}></canvas>
      </div>

      {#if narrative}
        <div class="narrative-card">
          <p class="narrative-headline">{narrative.narrative.headline}</p>
          <div class="observations">
            {#each narrative.narrative.observations as obs}
              <div class="observation">
                <span class="emotion-tag" style="color: {EMOTION_COLORS[obs.emotion.toLowerCase()] ?? '#888'}">
                  {obs.emotion}
                </span>
                <p class="obs-text">{obs.text}</p>
                {#each obs.cited_posts as postId}
                  {@const post = getPostById(postId)}
                  {#if post}
                    <a
                      class="cited-post"
                      href={post.permalink ?? '#'}
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      \u2192 {truncate(post.text ?? '(no text)', 80)} \u00B7 {post.views.toLocaleString()} views
                    </a>
                  {/if}
                {/each}
              </div>
            {/each}
          </div>
        </div>
      {:else}
        <div class="empty narrative-empty">
          <p>No emotion narrative generated yet.</p>
          <button class="generate-btn" onclick={regenerate} disabled={regenerating}>
            {regenerating ? 'Generating...' : 'Generate Narrative'}
          </button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .emotional-pulse {
    margin-top: 32px;
    padding-top: 24px;
    border-top: 1px solid #222;
  }
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 20px;
  }
  h3 {
    font-size: 18px;
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
  .content {
    transition: opacity 0.3s;
  }
  .content.regenerating {
    opacity: 0.4;
    pointer-events: none;
  }
  .chart-container {
    max-width: 400px;
    margin: 0 auto 24px;
  }
  .narrative-card {
    background: #111;
    border: 1px solid #222;
    border-radius: 8px;
    padding: 20px;
  }
  .narrative-headline {
    font-size: 15px;
    color: #ddd;
    line-height: 1.5;
    font-weight: 500;
    margin: 0 0 16px 0;
  }
  .observations {
    border-top: 1px solid #222;
    padding-top: 12px;
  }
  .observation {
    margin-bottom: 12px;
  }
  .emotion-tag {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .obs-text {
    font-size: 13px;
    color: #bbb;
    line-height: 1.6;
    margin: 4px 0;
  }
  .cited-post {
    display: block;
    font-size: 12px;
    color: #60a5fa;
    text-decoration: none;
    margin-bottom: 4px;
  }
  .cited-post:hover {
    text-decoration: underline;
  }
  .empty {
    text-align: center;
    padding: 40px 20px;
    color: #888;
  }
  .narrative-empty {
    padding: 24px 20px;
  }
  .generate-btn {
    margin-top: 12px;
    background: #333;
    border: 1px solid #444;
    color: #ddd;
    padding: 8px 20px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }
  .generate-btn:hover {
    background: #3a3a3a;
  }
  .generate-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .skeleton-block {
    height: 300px;
    background: #111;
    border-radius: 8px;
    animation: pulse 1.5s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.7; }
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
</style>
```

- [ ] **Step 2: Add EmotionalPulse to the Insights page**

In `web/src/lib/components/Insights.svelte`, add the import at the top of the `<script>` block:

```typescript
  import EmotionalPulse from '$lib/components/EmotionalPulse.svelte';
```

Then add the component at the end of the `.insights` div, just before the closing `</div>` (after the grid's closing `{/if}`):

```svelte
  <EmotionalPulse />
```

- [ ] **Step 3: Verify frontend builds**

Run: `cd "/Users/dennis/programming projects/postgraph/web" && npx svelte-check`
Expected: No errors.

- [ ] **Step 4: Commit**

```bash
git add web/src/lib/components/EmotionalPulse.svelte web/src/lib/components/Insights.svelte
git commit -m "feat(emotions): add Emotional Pulse radar chart and narrative component"
```

---

### Task 9: Final Verification

- [ ] **Step 1: Run Rust formatting and linting**

```bash
cd "/Users/dennis/programming projects/postgraph"
cargo fmt --all
cargo clippy --workspace --all-targets
```

Fix any warnings or errors.

- [ ] **Step 2: Run full Rust compilation**

```bash
cargo check --workspace
```

Expected: Clean compilation.

- [ ] **Step 3: Run frontend checks**

```bash
cd "/Users/dennis/programming projects/postgraph/web"
npx svelte-check
```

Expected: No errors.

- [ ] **Step 4: Commit any formatting fixes**

```bash
cd "/Users/dennis/programming projects/postgraph"
git add -A
git commit -m "chore: formatting and lint fixes"
```

(Skip if no changes.)
