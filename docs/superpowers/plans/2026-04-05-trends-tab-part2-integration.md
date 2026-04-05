# Trends Tab Part 2: Postgraph Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate the Reach scraper service with Postgraph's Rust backend and Svelte frontend to create a Trends tab that generates content ideation from trending sources.

**Architecture:** Postgraph backend orchestrates scraping (~20 sources via Reach HTTP API), builds a TrendContext, sends it to Mercury for ideation, stores reports in PostgreSQL, and serves them to a new Svelte Trends page. Follows the same patterns as the existing Insights tab.

**Tech Stack:** Rust (axum, sqlx, reqwest, serde, tokio), Mercury LLM, SvelteKit, PostgreSQL

**Spec:** `docs/superpowers/specs/2026-04-05-trends-tab-design.md`

**Prerequisite:** Part 1 (reach-scraper service) must be deployed and accessible.

---

### Task 1: Database Migration

**Files:**
- Create: `postgraph-server/migrations/013_trend_reports.sql`

- [ ] **Step 1: Create the migration file**

```sql
CREATE TABLE trend_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    trigger_type TEXT NOT NULL,
    report JSONB NOT NULL,
    context JSONB NOT NULL
);
```

- [ ] **Step 2: Verify compilation**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/migrations/013_trend_reports.sql
git commit -m "feat(trends): add trend_reports table migration"
```

---

### Task 2: Reach Client Module

**Files:**
- Create: `postgraph-server/src/reach.rs`
- Modify: `postgraph-server/src/main.rs` (add `mod reach;`)

This module handles HTTP communication with the Reach scraper service. It's a generic client — it doesn't know about specific sources.

- [ ] **Step 1: Add module declaration**

In `postgraph-server/src/main.rs`, add `mod reach;` after `mod insights;` (around line 6):

```rust
mod reach;
```

- [ ] **Step 2: Create reach.rs**

Create `postgraph-server/src/reach.rs`:

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

pub struct ReachClient {
    client: Client,
    base_url: String,
    api_key: String,
}

#[derive(Debug, Serialize)]
pub struct ScrapeRequest {
    pub url: String,
    pub selectors: ScrapeSelectors,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_for: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ScrapeSelectors {
    pub items: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScrapeResponse {
    pub url: String,
    pub items: Vec<ScrapedItem>,
    pub scraped_at: String,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScrapedItem {
    pub title: Option<String>,
    pub score: Option<String>,
    pub link: Option<String>,
    pub snippet: Option<String>,
    pub comments: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BrowseRequest {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_for: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BrowseResponse {
    pub url: String,
    pub title: String,
    pub content: String,
    pub scraped_at: String,
    pub error: Option<String>,
}

impl ReachClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            base_url,
            api_key,
        }
    }

    pub async fn scrape(&self, req: &ScrapeRequest) -> Result<ScrapeResponse, AppError> {
        let resp = self
            .client
            .post(format!("{}/scrape", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::MercuryApi(format!("Scraper error: {body}")));
        }

        let result: ScrapeResponse = resp.json().await?;
        Ok(result)
    }

    pub async fn browse(&self, req: &BrowseRequest) -> Result<BrowseResponse, AppError> {
        let resp = self
            .client
            .post(format!("{}/browse", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::MercuryApi(format!("Scraper error: {body}")));
        }

        let result: BrowseResponse = resp.json().await?;
        Ok(result)
    }

    pub async fn health_check(&self) -> Result<(), AppError> {
        let resp = self
            .client
            .get(format!("{}/health", self.base_url))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(AppError::MercuryApi("Scraper unhealthy".to_string()));
        }
        Ok(())
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles (warnings about unused code are fine — it's used in the next task)

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/reach.rs postgraph-server/src/main.rs
git commit -m "feat(trends): add Reach scraper HTTP client"
```

---

### Task 3: Source Definitions and Trend Scanning

**Files:**
- Create: `postgraph-server/src/trends.rs`
- Modify: `postgraph-server/src/main.rs` (add `mod trends;`)

This module defines the sources, orchestrates scraping, builds the TrendContext, and calls Mercury.

- [ ] **Step 1: Add module declaration**

In `postgraph-server/src/main.rs`, add `mod trends;` after `mod reach;`:

```rust
mod trends;
```

- [ ] **Step 2: Create trends.rs with types, source definitions, and scanning**

Create `postgraph-server/src/trends.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{info, warn};

use crate::error::AppError;
use crate::mercury::MercuryClient;
use crate::reach::{BrowseRequest, ReachClient, ScrapeRequest, ScrapeSelectors};

// ── Types ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct TrendContext {
    pub scanned_at: String,
    pub sources: Vec<SourceResult>,
    pub creator_subjects: Vec<String>,
    pub creator_intents: Vec<String>,
    pub recent_post_subjects: Vec<SubjectFrequency>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceResult {
    pub platform: String,
    pub source_name: String,
    pub items: Vec<TrendItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrendItem {
    pub title: String,
    pub snippet: Option<String>,
    pub url: Option<String>,
    pub score: Option<String>,
    pub comments: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubjectFrequency {
    pub name: String,
    pub post_count_30d: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrendReport {
    pub headline: String,
    pub ideas: Vec<TrendIdea>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrendIdea {
    pub title: String,
    pub angle: String,
    pub why_now: String,
    pub sources: Vec<IdeaSource>,
    pub suggested_intent: String,
    pub suggested_subject: String,
    pub relevance: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IdeaSource {
    pub platform: String,
    pub title: String,
    pub url: Option<String>,
    pub score: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredTrendReport {
    pub id: String,
    pub generated_at: DateTime<Utc>,
    pub trigger_type: String,
    pub report: TrendReport,
}

// ── Source Definitions ───────────────────────────────────────────

struct SourceDef {
    platform: &'static str,
    name: &'static str,
    url: &'static str,
    selectors: ScrapeSelectors,
    wait_for: Option<&'static str>,
    max_items: u32,
}

fn source_definitions() -> Vec<SourceDef> {
    vec![
        // Reddit subreddits
        reddit_source("r/programming", "https://www.reddit.com/r/programming/hot/"),
        reddit_source("r/LocalLLaMA", "https://www.reddit.com/r/LocalLLaMA/hot/"),
        reddit_source("r/MachineLearning", "https://www.reddit.com/r/MachineLearning/hot/"),
        reddit_source("r/ChatGPT", "https://www.reddit.com/r/ChatGPT/hot/"),
        reddit_source("r/SideProject", "https://www.reddit.com/r/SideProject/hot/"),
        reddit_source("r/ExperiencedDevs", "https://www.reddit.com/r/ExperiencedDevs/hot/"),
        reddit_source("r/cscareerquestions", "https://www.reddit.com/r/cscareerquestions/hot/"),
        reddit_source("r/productivity", "https://www.reddit.com/r/productivity/hot/"),
        reddit_source("r/technology", "https://www.reddit.com/r/technology/hot/"),
        // Hacker News
        SourceDef {
            platform: "hackernews",
            name: "Hacker News",
            url: "https://news.ycombinator.com/",
            selectors: ScrapeSelectors {
                items: ".athing".to_string(),
                title: ".titleline > a".to_string(),
                score: None,
                link: Some(".titleline > a".to_string()),
                snippet: None,
                comments: None,
            },
            wait_for: Some(".titleline"),
            max_items: 30,
        },
        SourceDef {
            platform: "hackernews",
            name: "Show HN",
            url: "https://news.ycombinator.com/show",
            selectors: ScrapeSelectors {
                items: ".athing".to_string(),
                title: ".titleline > a".to_string(),
                score: None,
                link: Some(".titleline > a".to_string()),
                snippet: None,
                comments: None,
            },
            wait_for: Some(".titleline"),
            max_items: 20,
        },
        // Tech sites
        SourceDef {
            platform: "techsite",
            name: "The Verge",
            url: "https://www.theverge.com/tech",
            selectors: ScrapeSelectors {
                items: "h2 a, h3 a".to_string(),
                title: "".to_string(), // Self is the title
                score: None,
                link: None, // Self is the link
                snippet: None,
                comments: None,
            },
            wait_for: None,
            max_items: 15,
        },
        SourceDef {
            platform: "techsite",
            name: "Ars Technica",
            url: "https://arstechnica.com/",
            selectors: ScrapeSelectors {
                items: "article, .article".to_string(),
                title: "h2 a, h3 a".to_string(),
                score: None,
                link: Some("h2 a, h3 a".to_string()),
                snippet: Some("p, .excerpt".to_string()),
                comments: None,
            },
            wait_for: None,
            max_items: 15,
        },
        SourceDef {
            platform: "techsite",
            name: "TechCrunch",
            url: "https://techcrunch.com/",
            selectors: ScrapeSelectors {
                items: "article, .post-block".to_string(),
                title: "h2 a, h3 a".to_string(),
                score: None,
                link: Some("h2 a, h3 a".to_string()),
                snippet: Some("p, .excerpt".to_string()),
                comments: None,
            },
            wait_for: None,
            max_items: 15,
        },
        SourceDef {
            platform: "techsite",
            name: "Wired",
            url: "https://www.wired.com/",
            selectors: ScrapeSelectors {
                items: "article, .card".to_string(),
                title: "h2 a, h3 a".to_string(),
                score: None,
                link: Some("h2 a, h3 a".to_string()),
                snippet: Some("p, .dek".to_string()),
                comments: None,
            },
            wait_for: None,
            max_items: 15,
        },
        // arXiv
        SourceDef {
            platform: "arxiv",
            name: "arXiv cs.AI",
            url: "https://arxiv.org/list/cs.AI/recent",
            selectors: ScrapeSelectors {
                items: ".meta".to_string(),
                title: ".list-title a".to_string(),
                score: None,
                link: Some(".list-title a".to_string()),
                snippet: Some(".mathjax".to_string()),
                comments: None,
            },
            wait_for: None,
            max_items: 10,
        },
        SourceDef {
            platform: "arxiv",
            name: "arXiv cs.CL",
            url: "https://arxiv.org/list/cs.CL/recent",
            selectors: ScrapeSelectors {
                items: ".meta".to_string(),
                title: ".list-title a".to_string(),
                score: None,
                link: Some(".list-title a".to_string()),
                snippet: Some(".mathjax".to_string()),
                comments: None,
            },
            wait_for: None,
            max_items: 10,
        },
        SourceDef {
            platform: "arxiv",
            name: "arXiv cs.LG",
            url: "https://arxiv.org/list/cs.LG/recent",
            selectors: ScrapeSelectors {
                items: ".meta".to_string(),
                title: ".list-title a".to_string(),
                score: None,
                link: Some(".list-title a".to_string()),
                snippet: Some(".mathjax".to_string()),
                comments: None,
            },
            wait_for: None,
            max_items: 10,
        },
    ]
}

fn reddit_source(name: &'static str, url: &'static str) -> SourceDef {
    SourceDef {
        platform: "reddit",
        name,
        url,
        selectors: ScrapeSelectors {
            items: "article, shreddit-post, [data-testid=post-container]".to_string(),
            title: "a[slot=title], h3, [data-testid=post-title]".to_string(),
            score: Some("[score], [data-testid=post-score], faceplate-number".to_string()),
            link: Some("a[slot=title], a[data-click-id=body]".to_string()),
            snippet: None,
            comments: Some("[data-testid=comment-count], a[data-click-id=comments]".to_string()),
        },
        wait_for: Some("article, shreddit-post"),
        max_items: 20,
    }
}

// ── Scrape All Sources ───────────────────────────────────────────

pub async fn scrape_all_sources(reach: &ReachClient) -> Vec<SourceResult> {
    let sources = source_definitions();
    let mut results = Vec::new();

    for source in &sources {
        info!("Scraping {}...", source.name);

        let req = ScrapeRequest {
            url: source.url.to_string(),
            selectors: ScrapeSelectors {
                items: source.selectors.items.clone(),
                title: source.selectors.title.clone(),
                score: source.selectors.score.clone(),
                link: source.selectors.link.clone(),
                snippet: source.selectors.snippet.clone(),
                comments: source.selectors.comments.clone(),
            },
            wait_for: source.wait_for.map(|s| s.to_string()),
            max_items: Some(source.max_items),
        };

        match reach.scrape(&req).await {
            Ok(resp) => {
                let items: Vec<TrendItem> = resp
                    .items
                    .into_iter()
                    .filter_map(|item| {
                        item.title.map(|title| TrendItem {
                            title,
                            snippet: item.snippet,
                            url: item.link,
                            score: item.score,
                            comments: item.comments,
                        })
                    })
                    .collect();

                info!("  {} → {} items", source.name, items.len());
                results.push(SourceResult {
                    platform: source.platform.to_string(),
                    source_name: source.name.to_string(),
                    items,
                });
            }
            Err(e) => {
                warn!("  {} failed: {e}", source.name);
            }
        }
    }

    results
}

// ── Build TrendContext ───────────────────────────────────────────

pub async fn build_context(
    pool: &PgPool,
    reach: &ReachClient,
) -> Result<TrendContext, AppError> {
    let sources = scrape_all_sources(reach).await;

    if sources.is_empty() {
        return Err(AppError::MercuryApi(
            "All sources failed to scrape".to_string(),
        ));
    }

    let thirty_days_ago = Utc::now() - chrono::Duration::days(30);

    // Get creator's subjects
    let subject_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT s.name FROM subjects s JOIN posts p ON p.subject_id = s.id WHERE p.analyzed_at IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    let creator_subjects: Vec<String> = subject_rows.into_iter().map(|r| r.0).collect();

    // Get creator's intents
    let intent_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT i.name FROM intents i JOIN posts p ON p.intent_id = i.id WHERE p.analyzed_at IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    let creator_intents: Vec<String> = intent_rows.into_iter().map(|r| r.0).collect();

    // Recent post subjects (last 30 days)
    let freq_rows: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT s.name, COUNT(*) AS cnt
           FROM posts p
           JOIN subjects s ON p.subject_id = s.id
           WHERE p.timestamp >= $1 AND p.analyzed_at IS NOT NULL
           GROUP BY s.name
           ORDER BY cnt DESC"#,
    )
    .bind(thirty_days_ago)
    .fetch_all(pool)
    .await?;

    let recent_post_subjects: Vec<SubjectFrequency> = freq_rows
        .into_iter()
        .map(|(name, count)| SubjectFrequency {
            name,
            post_count_30d: count,
        })
        .collect();

    let total_items: usize = sources.iter().map(|s| s.items.len()).sum();
    info!(
        "Trend context built: {} sources, {} total items",
        sources.len(),
        total_items
    );

    Ok(TrendContext {
        scanned_at: Utc::now().to_rfc3339(),
        sources,
        creator_subjects,
        creator_intents,
        recent_post_subjects,
    })
}

// ── Generate Report ──────────────────────────────────────────────

pub async fn generate_report(
    pool: &PgPool,
    mercury: &MercuryClient,
    reach: &ReachClient,
    trigger_type: &str,
) -> Result<StoredTrendReport, AppError> {
    let context = build_context(pool, reach).await?;

    info!("Generating trend report ({trigger_type})...");
    let report = mercury.generate_trends(&context).await?;

    let context_json = serde_json::to_value(&context)?;
    let report_json = serde_json::to_value(&report)?;

    let (id, generated_at): (uuid::Uuid, DateTime<Utc>) = sqlx::query_as(
        r#"INSERT INTO trend_reports (trigger_type, report, context)
           VALUES ($1, $2, $3)
           RETURNING id, generated_at"#,
    )
    .bind(trigger_type)
    .bind(&report_json)
    .bind(&context_json)
    .fetch_one(pool)
    .await?;

    info!("Trend report generated: {id}");

    Ok(StoredTrendReport {
        id: id.to_string(),
        generated_at,
        trigger_type: trigger_type.to_string(),
        report,
    })
}

// ── Get Latest Report ────────────────────────────────────────────

pub async fn get_latest_report(pool: &PgPool) -> Result<Option<StoredTrendReport>, AppError> {
    let row: Option<(uuid::Uuid, DateTime<Utc>, String, serde_json::Value)> = sqlx::query_as(
        r#"SELECT id, generated_at, trigger_type, report
           FROM trend_reports
           ORDER BY generated_at DESC
           LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await?;

    match row {
        None => Ok(None),
        Some((id, generated_at, trigger_type, report_json)) => {
            let report: TrendReport = serde_json::from_value(report_json)
                .map_err(|e| AppError::MercuryApi(format!("Corrupt trend report: {e}")))?;
            Ok(Some(StoredTrendReport {
                id: id.to_string(),
                generated_at,
                trigger_type,
                report,
            }))
        }
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: May fail because `mercury.generate_trends()` doesn't exist yet — that's Task 4. If so, comment out the `generate_report` function body temporarily, verify the types compile, then uncomment in the next task.

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/trends.rs postgraph-server/src/main.rs
git commit -m "feat(trends): add source definitions, scraping orchestration, and TrendContext"
```

---

### Task 4: Mercury Trends Prompt

**Files:**
- Modify: `postgraph-server/src/mercury.rs` (add `generate_trends` method + imports)

- [ ] **Step 1: Add imports at top of mercury.rs**

Add after the existing `use crate::insights::{InsightsContext, InsightsReport};` line:

```rust
use crate::trends::{TrendContext, TrendReport};
```

- [ ] **Step 2: Add generate_trends method**

Add this method to `impl MercuryClient`, after the `generate_insights` method:

```rust
    pub async fn generate_trends(
        &self,
        context: &TrendContext,
    ) -> Result<TrendReport, AppError> {
        let context_json = serde_json::to_string_pretty(context)
            .map_err(|e| AppError::MercuryApi(format!("Failed to serialize trend context: {e}")))?;

        let system_prompt = r#"You're a sharp content strategist helping a tech creator find their next posts. You've just scanned what's trending across Reddit, Hacker News, tech sites, arXiv, Threads, and X.

The creator's subjects, intents, and recent posting frequency are provided below. Generate 5-10 post ideas that are:
- Timely — tied to something trending RIGHT NOW
- Specific — don't say "post about AI", say what angle, what take, what format
- Voiced — match the creator's style (look at their intent distribution)
- Gap-filling — prioritize topics they haven't covered recently

For each idea, provide:
- "title": a punchy post title/hook
- "angle": 1-2 sentences on the take or approach
- "why_now": why this is timely (cite specific trending items)
- "sources": array of {"platform", "title", "url", "score"} that inspired this idea
- "suggested_intent": one of the creator's existing intents (e.g. "Hot take", "Question", "Tip")
- "suggested_subject": one of the creator's existing subjects (e.g. "AI & LLMs", "Software dev")
- "relevance": why this matters for THIS creator specifically

Also write a "headline": one punchy sentence summarizing what's dominating the trend landscape this week.

Respond with ONLY valid JSON:
{"headline": "...", "ideas": [{"title": "...", "angle": "...", "why_now": "...", "sources": [{"platform": "...", "title": "...", "url": "...", "score": "..."}], "suggested_intent": "...", "suggested_subject": "...", "relevance": "..."}]}"#;

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
                        "Here is what's trending right now, plus the creator's profile:\n\n{context_json}"
                    ),
                },
            ],
            temperature: 0.6,
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

        let report: TrendReport = serde_json::from_str(json_str).map_err(|e| {
            AppError::MercuryApi(format!(
                "Failed to parse trends response: {e}. Raw: {json_str}"
            ))
        })?;

        Ok(report)
    }
```

- [ ] **Step 3: Verify compilation**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add postgraph-server/src/mercury.rs
git commit -m "feat(trends): add Mercury generate_trends prompt"
```

---

### Task 5: AppState + API Routes + Nightly Schedule

**Files:**
- Modify: `postgraph-server/src/state.rs` (add `reach` field)
- Create: `postgraph-server/src/routes/trends.rs`
- Modify: `postgraph-server/src/routes/mod.rs` (add `pub mod trends;`)
- Modify: `postgraph-server/src/main.rs` (init ReachClient, mount routes, add 4am schedule)

- [ ] **Step 1: Add ReachClient to AppState**

In `postgraph-server/src/state.rs`, add the import and field:

```rust
use crate::reach::ReachClient;
```

Add to the `AppState` struct, after the `mercury` field:

```rust
    pub reach: Option<Arc<ReachClient>>,
```

The `Option` allows Postgraph to run without the scraper configured (it's a new dependency).

- [ ] **Step 2: Create routes/trends.rs**

Add `pub mod trends;` to `postgraph-server/src/routes/mod.rs`.

Create `postgraph-server/src/routes/trends.rs`:

```rust
use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::state::AppState;
use crate::trends;

#[derive(Serialize)]
pub struct TrendsResponse {
    pub id: String,
    pub generated_at: String,
    pub trigger_type: String,
    pub report: trends::TrendReport,
}

#[derive(Serialize)]
pub struct TrendsError {
    pub error: String,
}

pub async fn get_latest(
    State(state): State<AppState>,
) -> Result<Json<TrendsResponse>, (axum::http::StatusCode, Json<TrendsError>)> {
    let report = trends::get_latest_report(&state.pool)
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(TrendsError {
                    error: e.to_string(),
                }),
            )
        })?;

    match report {
        Some(r) => Ok(Json(TrendsResponse {
            id: r.id,
            generated_at: r.generated_at.to_rfc3339(),
            trigger_type: r.trigger_type,
            report: r.report,
        })),
        None => Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(TrendsError {
                error: "No trend report generated yet".to_string(),
            }),
        )),
    }
}

pub async fn generate(
    State(state): State<AppState>,
) -> Result<Json<TrendsResponse>, (axum::http::StatusCode, Json<TrendsError>)> {
    let reach = state.reach.as_ref().ok_or_else(|| {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(TrendsError {
                error: "Scraper service not configured (REACH_URL not set)".to_string(),
            }),
        )
    })?;

    let report = trends::generate_report(&state.pool, &state.mercury, reach, "manual")
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(TrendsError {
                    error: e.to_string(),
                }),
            )
        })?;

    Ok(Json(TrendsResponse {
        id: report.id,
        generated_at: report.generated_at.to_rfc3339(),
        trigger_type: report.trigger_type,
        report: report.report,
    }))
}
```

- [ ] **Step 3: Initialize ReachClient in main.rs**

In `postgraph-server/src/main.rs`, add after the Mercury client initialization (around line 80, after `let mercury_url = ...`):

```rust
    let reach = match std::env::var("REACH_URL") {
        Ok(url) => {
            let key = std::env::var("REACH_API_KEY").unwrap_or_default();
            info!("Reach scraper configured at {url}");
            Some(Arc::new(reach::ReachClient::new(url, key)))
        }
        Err(_) => {
            info!("REACH_URL not set — trend scanning disabled");
            None
        }
    };
```

Add to the `AppState` initialization:

```rust
        reach,
```

- [ ] **Step 4: Mount routes in main.rs**

Add to `api_routes`, before the `.layer(middleware::from_fn_with_state(` line:

```rust
        .route("/api/trends/latest", get(routes::trends::get_latest))
        .route("/api/trends/generate", post(routes::trends::generate))
```

- [ ] **Step 5: Add 4am nightly trend scan**

In `main.rs`, after the existing nightly sync `tokio::spawn` block (after the `});` that closes it, around line 245), add a new spawn block:

```rust
    // Spawn trend scanning task at 4am
    if let Some(ref reach_client) = state.reach {
        let trend_state = state.clone();
        let trend_reach = Arc::clone(reach_client);
        tokio::spawn(async move {
            loop {
                let sleep_dur = duration_until_hour(4, tz);
                info!(
                    "Trend scan scheduled in {:.1}h ({tz})",
                    sleep_dur.as_secs_f64() / 3600.0
                );
                tokio::time::sleep(sleep_dur).await;

                info!("Trend scan starting");
                match trends::generate_report(
                    &trend_state.pool,
                    &trend_state.mercury,
                    &trend_reach,
                    "nightly",
                )
                .await
                {
                    Ok(r) => info!("Trend report generated: {}", r.id),
                    Err(e) => tracing::error!("Trend scan failed: {e}"),
                }
            }
        });
    }
```

Also add a helper function `duration_until_hour` (or refactor `duration_until_2am` to accept an hour parameter). Add this near `duration_until_2am`:

```rust
fn duration_until_hour(hour: u32, tz: chrono_tz::Tz) -> Duration {
    let now = Utc::now().with_timezone(&tz);
    let today_target = now
        .date_naive()
        .and_hms_opt(hour, 0, 0)
        .unwrap()
        .and_local_timezone(tz)
        .earliest()
        .unwrap_or_else(|| {
            now.date_naive()
                .succ_opt()
                .unwrap()
                .and_hms_opt(hour, 0, 0)
                .unwrap()
                .and_local_timezone(tz)
                .earliest()
                .unwrap()
        });

    let target = if today_target <= now {
        now.date_naive()
            .succ_opt()
            .unwrap()
            .and_hms_opt(hour, 0, 0)
            .unwrap()
            .and_local_timezone(tz)
            .earliest()
            .unwrap()
    } else {
        today_target
    };

    let diff = target.signed_duration_since(now);
    Duration::from_secs(diff.num_seconds().max(0) as u64)
}
```

- [ ] **Step 6: Verify compilation**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles

- [ ] **Step 7: Commit**

```bash
git add postgraph-server/src/state.rs postgraph-server/src/routes/trends.rs postgraph-server/src/routes/mod.rs postgraph-server/src/main.rs
git commit -m "feat(trends): add AppState, API routes, and 4am nightly schedule"
```

---

### Task 6: Frontend Proxy Routes

**Files:**
- Create: `web/src/routes/api/trends/latest/+server.ts`
- Create: `web/src/routes/api/trends/generate/+server.ts`

- [ ] **Step 1: Create GET proxy**

Create `web/src/routes/api/trends/latest/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  return proxyToBackend('/api/trends/latest');
};
```

- [ ] **Step 2: Create POST proxy**

Create `web/src/routes/api/trends/generate/+server.ts`:

```typescript
import { proxyToBackend } from '$lib/server/proxy';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async () => {
  return proxyToBackend('/api/trends/generate', { method: 'POST' });
};
```

- [ ] **Step 3: Commit**

```bash
git add web/src/routes/api/trends/
git commit -m "feat(trends): add frontend proxy routes"
```

---

### Task 7: Frontend API Client

**Files:**
- Modify: `web/src/lib/api.ts`

- [ ] **Step 1: Add TypeScript types**

Add these interfaces in `web/src/lib/api.ts`, before the `export const api = {` line:

```typescript
export interface IdeaSource {
  platform: string;
  title: string;
  url: string | null;
  score: string | null;
}

export interface TrendIdea {
  title: string;
  angle: string;
  why_now: string;
  sources: IdeaSource[];
  suggested_intent: string;
  suggested_subject: string;
  relevance: string;
}

export interface TrendReport {
  headline: string;
  ideas: TrendIdea[];
}

export interface TrendsResponse {
  id: string;
  generated_at: string;
  trigger_type: string;
  report: TrendReport;
}
```

- [ ] **Step 2: Add API methods**

Add inside the `api` object, after the `generateInsights` method:

```typescript
  getTrendsLatest: () => fetchApi<TrendsResponse>('/api/trends/latest'),
  generateTrends: () => fetch('/api/trends/generate', {
    method: 'POST',
  }).then(async r => {
    if (!r.ok) {
      const body = await r.json().catch(() => ({ error: `Generate failed (${r.status})` }));
      throw new Error(body.error ?? `Generate failed (${r.status})`);
    }
    return r.json() as Promise<TrendsResponse>;
  }),
```

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/api.ts
git commit -m "feat(trends): add trend types and API client methods"
```

---

### Task 8: Trends Svelte Component

**Files:**
- Create: `web/src/lib/components/Trends.svelte`

- [ ] **Step 1: Create the component**

Create `web/src/lib/components/Trends.svelte`:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { TrendsResponse } from '$lib/api';

  let report: TrendsResponse | null = $state(null);
  let loading = $state(true);
  let generating = $state(false);
  let error = $state('');

  const PLATFORM_LABELS: Record<string, string> = {
    reddit: 'Reddit',
    hackernews: 'HN',
    techsite: 'Web',
    arxiv: 'arXiv',
    threads: 'Threads',
    x: 'X',
  };

  function timeAgo(dateStr: string): string {
    const diff = Date.now() - new Date(dateStr).getTime();
    const hours = Math.floor(diff / 3600000);
    if (hours < 1) return 'just now';
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  }

  function fetchWithTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
    return Promise.race([
      promise,
      new Promise<T>((_, reject) => setTimeout(() => reject(new Error('Timeout')), ms)),
    ]);
  }

  async function loadReport() {
    try {
      report = await fetchWithTimeout(api.getTrendsLatest(), 10000);
    } catch {
      report = null;
    }
    loading = false;
  }

  async function generate() {
    generating = true;
    error = '';
    try {
      report = await api.generateTrends();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to generate trends';
    } finally {
      generating = false;
    }
  }

  onMount(loadReport);
</script>

<div class="trends">
  {#if loading}
    <div class="header">
      <div>
        <h2>What's Trending</h2>
        <span class="subtitle">Loading...</span>
      </div>
    </div>
    <div class="ideas">
      {#each Array(3) as _}
        <div class="card skeleton"></div>
      {/each}
    </div>
  {:else if error && !report}
    <div class="header">
      <div><h2>What's Trending</h2></div>
    </div>
    <div class="empty"><p>{error}</p></div>
  {:else if !report}
    <div class="header">
      <div>
        <h2>What's Trending</h2>
        <span class="subtitle">No trends scanned yet</span>
      </div>
    </div>
    <div class="empty">
      <p>No trend report has been generated yet.</p>
      <button class="generate-btn" onclick={generate} disabled={generating}>
        {generating ? 'Scanning sources...' : 'Scan Now'}
      </button>
    </div>
  {:else}
    <div class="header">
      <div>
        <h2>What's Trending</h2>
        <span class="subtitle">
          Generated {timeAgo(report.generated_at)}
          · {report.trigger_type === 'nightly' ? 'Auto' : 'Manual'}
        </span>
      </div>
      <button class="regen-btn" onclick={generate} disabled={generating}>
        {generating ? 'Scanning...' : '↻ Regenerate'}
      </button>
    </div>

    {#if error}
      <div class="error-toast">{error}</div>
    {/if}

    <div class="headline">{report.report.headline}</div>

    <div class="ideas" class:generating>
      {#each report.report.ideas as idea, i}
        <div class="card">
          <div class="idea-number">{i + 1}</div>
          <h3 class="idea-title">{idea.title}</h3>
          <p class="idea-angle">{idea.angle}</p>
          <p class="idea-why">{idea.why_now}</p>

          <div class="sources">
            {#each idea.sources as source}
              <a
                class="source-badge"
                href={source.url ?? '#'}
                target="_blank"
                rel="noopener noreferrer"
              >
                <span class="platform">{PLATFORM_LABELS[source.platform] ?? source.platform}</span>
                {#if source.score}
                  <span class="score">{source.score}</span>
                {/if}
              </a>
            {/each}
          </div>

          <div class="idea-meta">
            <span class="tag intent">{idea.suggested_intent}</span>
            <span class="tag subject">{idea.suggested_subject}</span>
          </div>

          <p class="relevance">{idea.relevance}</p>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .trends {
    max-width: 800px;
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
  .subtitle { font-size: 13px; color: #888; }
  .regen-btn {
    background: #222;
    border: 1px solid #333;
    color: #ccc;
    padding: 8px 16px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }
  .regen-btn:hover { background: #2a2a2a; border-color: #444; }
  .regen-btn:disabled { opacity: 0.5; cursor: not-allowed; }
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
  .ideas {
    display: flex;
    flex-direction: column;
    gap: 16px;
    transition: opacity 0.3s;
  }
  .ideas.generating { opacity: 0.4; pointer-events: none; }
  .card {
    background: #111;
    border: 1px solid #222;
    border-radius: 8px;
    padding: 20px 24px;
    position: relative;
  }
  .card.skeleton {
    min-height: 150px;
    animation: pulse 1.5s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.7; }
  }
  .idea-number {
    position: absolute;
    top: 16px;
    right: 20px;
    font-size: 32px;
    font-weight: 700;
    color: #222;
  }
  .idea-title {
    font-size: 16px;
    font-weight: 600;
    color: #fff;
    margin: 0 0 8px 0;
    padding-right: 40px;
  }
  .idea-angle {
    font-size: 14px;
    color: #bbb;
    line-height: 1.6;
    margin: 0 0 8px 0;
  }
  .idea-why {
    font-size: 13px;
    color: #888;
    margin: 0 0 12px 0;
    font-style: italic;
  }
  .sources {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 12px;
  }
  .source-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: #1a1a2e;
    border: 1px solid #2a2a4a;
    border-radius: 4px;
    padding: 3px 8px;
    font-size: 11px;
    color: #8888cc;
    text-decoration: none;
  }
  .source-badge:hover { border-color: #4444aa; color: #aaaaee; }
  .source-badge .platform { font-weight: 600; }
  .source-badge .score { color: #666; }
  .idea-meta {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
  }
  .tag {
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 3px;
  }
  .tag.intent { background: #1a2a1a; color: #4ade80; border: 1px solid #2a3a2a; }
  .tag.subject { background: #1a2a3a; color: #60a5fa; border: 1px solid #2a3a4a; }
  .relevance {
    font-size: 12px;
    color: #666;
    margin: 0;
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
  .generate-btn:hover { background: #3a3a3a; }
  .generate-btn:disabled { opacity: 0.5; cursor: not-allowed; }
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

- [ ] **Step 2: Verify**

Run: `cd "/Users/dennis/programming projects/postgraph/web" && npx svelte-check 2>&1 | tail -5`
Expected: no new errors

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/components/Trends.svelte
git commit -m "feat(trends): add Trends Svelte component with idea cards"
```

---

### Task 9: Route & Navigation

**Files:**
- Create: `web/src/routes/trends/+page.svelte`
- Modify: `web/src/routes/+layout.svelte`

- [ ] **Step 1: Create page route**

Create `web/src/routes/trends/+page.svelte`:

```svelte
<script lang="ts">
  import Trends from '$lib/components/Trends.svelte';
</script>

<Trends />
```

- [ ] **Step 2: Add nav link**

In `web/src/routes/+layout.svelte`, add after the Insights link:

```svelte
      <a href="/trends" class:active={$page.url.pathname === '/trends'}>Trends</a>
```

- [ ] **Step 3: Verify frontend builds**

Run: `cd "/Users/dennis/programming projects/postgraph/web" && npx svelte-check 2>&1 | tail -5`
Expected: no new errors

- [ ] **Step 4: Verify backend compiles**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo check --workspace`
Expected: compiles

- [ ] **Step 5: Run cargo fmt and clippy**

Run:
```bash
cd "/Users/dennis/programming projects/postgraph" && cargo fmt --all && cargo clippy --workspace --all-targets 2>&1 | tail -5
```
Expected: no new warnings

- [ ] **Step 6: Commit**

```bash
git add web/src/routes/trends/ web/src/routes/+layout.svelte
git commit -m "feat(trends): add /trends route and navigation link"
```

---

### Task 10: Smoke Test

**Files:** None (verification only)

- [ ] **Step 1: Start the backend**

Run: `cd "/Users/dennis/programming projects/postgraph" && cargo run --package postgraph-server`
Expected: server starts, migration 013 runs, logs "REACH_URL not set — trend scanning disabled" (or connects if REACH_URL is set)

- [ ] **Step 2: Start the frontend**

Run: `cd "/Users/dennis/programming projects/postgraph/web" && npm run dev`

- [ ] **Step 3: Navigate to /trends**

Open http://localhost:5173/trends. Expected: "No trend report has been generated yet" with "Scan Now" button (if REACH_URL not set, clicking Scan Now shows an error about scraper not configured).

- [ ] **Step 4: Verify nav**

Check "Trends" appears in navigation and highlights when active.

- [ ] **Step 5: Final lint**

Run:
```bash
cd "/Users/dennis/programming projects/postgraph" && cargo fmt --all && cargo clippy --workspace --all-targets
cd "/Users/dennis/programming projects/postgraph/web" && npx svelte-check
```

- [ ] **Step 6: Commit any lint fixes**

```bash
git add -A && git commit -m "chore: lint fixes for trends tab"
```
