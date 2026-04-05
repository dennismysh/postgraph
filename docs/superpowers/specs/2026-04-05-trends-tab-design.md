# Trends Tab Design Spec

**Date:** 2026-04-05
**Status:** Draft

## Overview

A new "Trends" tab in the Postgraph dashboard for **content ideation**. A Reach-based scraping service browses Reddit, Hacker News, tech sites, arXiv, Threads, and X on a schedule. The scraped trending content is sent to Mercury alongside the creator's subject/intent taxonomy and recent posting history. Mercury generates 5-10 concrete post ideas — each tied to specific trending source material, with a suggested angle, timing rationale, and relevance to the creator's content profile.

## Architecture

```
Postgraph (Railway)              Reach Scraper (Railway)
┌──────────────────┐             ┌────────────────────────────┐
│  Rust backend    │   HTTP      │  Reach sandbox image       │
│                  │ ─────────>  │  Xvfb + Chrome + Playwright│
│  Orchestrates    │             │  + Scrapling               │
│  source scanning │  <────────  │                            │
│                  │  scraped    │  Python Flask API           │
│  Aggregates into │  content    │  /browse, /scrape, /health │
│  TrendContext    │             └────────────────────────────┘
│                  │
│  Mercury call    │
│  → post ideas    │
│                  │
│  Store in DB     │
└──────────────────┘
```

Two Railway services in the same project. They communicate over Railway's internal networking (private URLs). No Docker-in-Docker or privileged containers — the Reach sandbox image runs directly as a Railway service.

## Scraping Service

### Deployment

A new directory `reach-scraper/` in the Postgraph repo. Contains:
- `Dockerfile` — based on Reach's sandbox image, adds the Python HTTP API
- `api.py` — Flask app exposing scraping endpoints
- `requirements.txt` — Flask + any additional Python deps

Deployed as a second Railway service. Railway auto-detects the Dockerfile from the `reach-scraper/` directory.

### Endpoints

**`POST /scrape`**

Request:
```json
{
  "url": "https://www.reddit.com/r/LocalLLaMA/hot/",
  "selectors": {
    "items": "article, .Post",
    "title": "h3, .title a",
    "score": ".score, [id^=vote-arrows]",
    "link": "a[data-click-id=body]"
  },
  "wait_for": ".Post",
  "max_items": 20
}
```

Response:
```json
{
  "url": "https://www.reddit.com/r/LocalLLaMA/hot/",
  "items": [
    {
      "title": "Llama 4 Scout vs GPT-4 on code generation",
      "score": "1247",
      "link": "https://reddit.com/r/LocalLLaMA/..."
    }
  ],
  "scraped_at": "2026-04-05T04:00:00Z"
}
```

Uses Playwright to navigate, wait for content, then extract via CSS selectors. The scraper is generic — Postgraph tells it what to extract.

**`POST /browse`**

Request:
```json
{
  "url": "https://news.ycombinator.com/",
  "wait_for": ".titleline"
}
```

Response:
```json
{
  "url": "https://news.ycombinator.com/",
  "title": "Hacker News",
  "content": "... page content as text ...",
  "scraped_at": "2026-04-05T04:00:00Z"
}
```

Simpler endpoint — navigates and returns full page text. Used when CSS selectors are impractical.

**`GET /health`**

Response: `{"status": "ok"}`

**Auth:** `Authorization: Bearer {REACH_API_KEY}` header on all endpoints.

## Sources

### Reddit (9 subreddits)
| Subreddit | Maps to subjects |
|---|---|
| r/programming | Software dev |
| r/LocalLLaMA | AI & LLMs |
| r/MachineLearning | AI & LLMs |
| r/ChatGPT | AI & LLMs, Social media |
| r/SideProject | Side projects |
| r/ExperiencedDevs | Career, Software dev |
| r/cscareerquestions | Career |
| r/productivity | Productivity |
| r/technology | Tech industry |

Scrape `/hot/` for each. Extract: title, score, comment count, link. Top 20 per subreddit.

### Hacker News
- Front page (top 30): `https://news.ycombinator.com/`
- Show HN: `https://news.ycombinator.com/show`

Extract: title, points, comment count, link.

### Tech Sites (4)
| Site | URL |
|---|---|
| The Verge | `https://www.theverge.com/tech` |
| Ars Technica | `https://arstechnica.com/` |
| TechCrunch | `https://techcrunch.com/` |
| Wired | `https://www.wired.com/` |

Extract: headline, snippet/subhead, link. Top 10-15 per site.

### arXiv (3 categories)
| Category | URL |
|---|---|
| cs.AI | `https://arxiv.org/list/cs.AI/recent` |
| cs.CL | `https://arxiv.org/list/cs.CL/recent` |
| cs.LG | `https://arxiv.org/list/cs.LG/recent` |

Extract: paper title, authors, abstract snippet. Top 10 per category.

### Social (via browser)
| Platform | What to scan |
|---|---|
| Threads | Explore/trending page |
| X/Twitter | Trending topics in tech |

These are the hardest to scrape reliably. Handled via Playwright browser automation. Extract: post text, engagement metrics where visible.

**Total: ~20 scrape requests per scan.** At ~5-10 seconds per request (browser navigation + render + extract), a full scan takes roughly 2-3 minutes.

## Data Pipeline

### Step 1: Scrape Sources (Rust → Reach)

A new `trends.rs` module in the Postgraph backend. Contains:
- Source definitions: URL, selectors, subject mapping
- `scrape_all_sources(reach_url)` function that iterates sources, calls the scraper, collects results
- Handles individual source failures gracefully (log and continue)

### Step 2: Build TrendContext

```rust
struct TrendContext {
    scanned_at: String,
    sources: Vec<SourceResult>,
    creator_subjects: Vec<String>,
    creator_intents: Vec<String>,
    recent_post_subjects: Vec<SubjectFrequency>,
}

struct SourceResult {
    platform: String,      // "reddit", "hackernews", "techsite", "arxiv", "threads", "x"
    source_name: String,   // "r/LocalLLaMA", "Hacker News", "The Verge"
    items: Vec<TrendItem>,
}

struct TrendItem {
    title: String,
    snippet: Option<String>,
    url: Option<String>,
    score: Option<String>,
    comments: Option<String>,
}

struct SubjectFrequency {
    name: String,
    post_count_30d: i64,
}
```

### Step 3: Mercury Call

System prompt establishes the content strategist persona. Mercury receives `TrendContext` and returns structured post ideas.

**Mercury prompt persona:**

> "You're a sharp content strategist helping a tech creator find their next posts. You've just scanned what's trending across Reddit, Hacker News, tech sites, arXiv, Threads, and X. The creator's subjects and recent posting history are below. Generate post ideas that are timely, match the creator's voice, and fill gaps they haven't covered yet. Be specific — don't say 'post about AI', say what angle, what take, what format."

**Temperature:** 0.6 (more creative than insights at 0.5)

**Response format:**

```json
{
  "headline": "Local models and developer tooling are dominating this week",
  "ideas": [
    {
      "title": "Why local models are eating cloud APIs",
      "angle": "Hot take on cost vs convenience — r/LocalLLaMA benchmarks show Llama 4 matching GPT-4 on code tasks at zero API cost",
      "why_now": "3 posts in r/LocalLLaMA with 2.4k+ combined upvotes this week",
      "sources": [
        {"platform": "reddit", "title": "Llama 4 vs GPT-4 on code", "url": "https://...", "score": "1200"}
      ],
      "suggested_intent": "Hot take",
      "suggested_subject": "AI & LLMs",
      "relevance": "You post about AI frequently but haven't touched local vs cloud"
    }
  ]
}
```

**Target: 5-10 ideas per report.** Mercury picks the best angles from all sources, not one per source.

### Step 4: Store Report

Insert into `trend_reports` table with both the Mercury output and the `TrendContext` input.

## Database Schema

### New table: `trend_reports`

```sql
CREATE TABLE trend_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    trigger_type TEXT NOT NULL,  -- 'nightly' or 'manual'
    report JSONB NOT NULL,       -- Mercury's structured JSON response
    context JSONB NOT NULL       -- the TrendContext sent to Mercury
);
```

Same shape as `insights_reports`.

## API Routes

### `GET /api/trends/latest`

Returns the most recent `trend_reports` row. Returns 404 if none exists.

### `POST /api/trends/generate`

Triggers a full scan + Mercury generation. Returns the new report when complete. This is slow (~3-5 minutes) so the frontend shows a progress state.

## Refresh Strategy

**Nightly at 4am** (2 hours after the main sync at 2am, so they don't compete). Added as a separate scheduled task in `main.rs`.

**Manual on-demand** via the Regenerate button. Shows "Scanning sources..." → "Generating ideas..." progress.

## Frontend

### Navigation

New tab "Trends" at `/trends` in the nav bar.

### Page Layout

**Header:** "What's Trending" — date + "Generated Xh ago" + Regenerate button.

**Headline banner:** Mercury's one-liner summary of the week's trends.

**Ideas feed:** Vertical list of cards. Each card contains:
- **Title** — the proposed post idea (bold, prominent)
- **Angle** — paragraph explaining the take/approach
- **Why now** — one line with timing rationale
- **Source badges** — platform icons with titles and scores, linking to source URLs
- **Tags** — suggested intent + subject from the creator's taxonomy
- **Relevance** — one line explaining why this matters for the creator

**States:**
- **Loading:** Skeleton cards
- **Empty:** "No trends scanned yet" + "Scan Now" button
- **Generating:** Progress text ("Scanning sources... Generating ideas...")
- **Error:** Error message with retry button

### Responsive

Cards go full-width on mobile (single column). Source badges wrap.

## Environment Variables

New variables for the Postgraph backend:
- `REACH_URL` — internal Railway URL of the Reach scraper service
- `REACH_API_KEY` — shared auth key for scraper requests

New variables for the Reach scraper service:
- `REACH_API_KEY` — same key, validated on incoming requests
- `PORT` — server port (Railway sets automatically)

## Error Handling

- **Scraper unreachable:** Log error, skip trend generation. Frontend shows last successful report.
- **Individual source fails:** Log and continue with remaining sources. Mercury works with partial data.
- **Mercury fails:** Generation fails. Frontend shows error toast, last report remains.
- **All sources fail:** Don't call Mercury. Return error to frontend.

## Scope Boundaries

**In scope:**
- Reach scraper service (Dockerfile + Python API)
- Source definitions and selector configs in Rust
- TrendContext computation and Mercury prompt
- `trend_reports` table + migration
- Two new API endpoints
- Svelte Trends page with idea cards
- Nightly scan schedule (4am)
- Manual regeneration

**Out of scope:**
- Historical trend browsing (only latest report)
- User-configurable source list (hardcoded for now)
- Trend tracking over time (no diff between reports)
- Automatic posting or drafting
- Source-specific error recovery/retry logic beyond simple skip
