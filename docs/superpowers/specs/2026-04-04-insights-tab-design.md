# Insights Tab Design Spec

**Date:** 2026-04-04
**Status:** Draft

## Overview

A new "Insights" tab in the Postgraph dashboard that uses Mercury LLM to generate expert-level summarization and analysis of the user's last 30 days of Threads activity. Mercury acts as a candid, analytically sharp friend — direct and informal, but backing everything with data. The analysis is organized into four sections that cover performance, problems, consistency, and behavioral shifts.

## Architecture: Pre-Compute + Narrate

Mercury is not good at arithmetic. Rust is. The pipeline splits the work accordingly:

1. **Rust pre-computes** a structured analytics context from the last 30 days of data
2. **Mercury narrates** that context into four sections with post citations
3. **Reports are stored** in a new `insights_reports` table for quick retrieval

This keeps Mercury's prompt small, its output grounded in real numbers, and post citations accurate (since Rust provides the ranked lists).

## Data Pipeline

### Step 1: Compute InsightsContext (Rust)

A new function queries existing tables and produces a structured context containing:

- **Post list** (last 30 days): id, text (truncated ~200 chars), permalink, timestamp, views, likes, replies, reposts, quotes, intent name, subject name, sentiment
- **Per-subject stats** (30-day vs. all-time): avg views, avg engagement, post count — enables "AI & LLMs averages 800 views this month vs your all-time 500"
- **Per-intent stats**: same structure as per-subject
- **Top 5 / bottom 5 posts** by views in the 30-day window
- **Posting frequency**: posts per week this month vs. historical weekly average
- **Sentiment distribution**: avg sentiment this month vs. all-time avg
- **Daily view trend**: total daily views for the 30-day window (momentum/trajectory)

### Step 2: Mercury Call

**System prompt** establishes the persona:

> You're a sharp social media analyst reviewing a creator's last 30 days of Threads activity. You're direct, a little informal, and you don't sugarcoat. But you back everything up with data. When you cite a post, reference it by its ID. Organize your analysis into exactly four sections.

**Temperature:** 0.5 (more creative than taxonomy analysis at 0.3, but still grounded)

**User message** contains the serialized InsightsContext as JSON.

**Response format** — structured JSON:

```json
{
  "headline": "Strong month for AI content, but your career posts need a rethink.",
  "sections": [
    {
      "key": "working",
      "title": "What's Working",
      "summary": "Your AI content is on fire this month...",
      "items": [
        {
          "observation": "AI & LLMs posts averaged 847 views vs your overall 362.",
          "cited_posts": ["post_id_1", "post_id_2"],
          "tone": "positive"
        }
      ]
    },
    {
      "key": "not_working",
      "title": "What's Not Working",
      "summary": "Career advice posts are landing flat...",
      "items": [...]
    },
    {
      "key": "on_brand",
      "title": "On Brand",
      "summary": "Posting cadence and topic mix are consistent...",
      "items": [...]
    },
    {
      "key": "off_pattern",
      "title": "Off Pattern",
      "summary": "A few notable deviations from your usual...",
      "items": [...]
    }
  ]
}
```

Each section contains:
- `summary`: A narrative paragraph in the candid friend voice
- `items`: 2-4 specific observations, each citing 1-2 post IDs
- `tone`: "positive", "negative", or "neutral" (for potential UI color coding)

### Step 3: Store Report

Reports are stored in `insights_reports` with both the Mercury output and the input context (for auditability).

## Database Schema

### New table: `insights_reports`

```sql
CREATE TABLE insights_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    trigger_type TEXT NOT NULL,  -- 'nightly' or 'manual'
    report JSONB NOT NULL,       -- Mercury's structured JSON response
    context JSONB NOT NULL       -- the InsightsContext sent to Mercury
);
```

No foreign keys to `posts` — cited post IDs are informational references. Posts could theoretically be deleted without breaking stored reports.

## API Routes

### `GET /api/insights/latest`

Returns the most recent `insights_reports` row. This is what the frontend loads on page visit.

**Response:**
```json
{
  "id": "uuid",
  "generated_at": "2026-04-04T02:00:00Z",
  "trigger_type": "nightly",
  "report": { ... }
}
```

Returns 404 if no report exists yet.

### `POST /api/insights/generate`

Triggers a new report generation. Computes InsightsContext, calls Mercury, stores the result, and returns the new report.

**Response:** Same shape as `GET /api/insights/latest`.

Returns an error if fewer than 5 analyzed posts exist in the last 30 days.

## Refresh Strategy

**Hybrid: nightly auto + manual on-demand.**

- **Nightly (automatic):** After the existing 2am sync → analysis pipeline completes, generate a fresh insights report. This ensures there's always a recent report available.
- **Manual (on-demand):** The frontend "Regenerate" button triggers `POST /api/insights/generate`. Used when the user wants a fresh take after a busy posting day.

Generation is added as a fourth step in the nightly sync task in `main.rs`, after: sync posts → sync metrics → sync daily views → **generate insights**.

## Frontend

### Navigation

New tab "Insights" in the nav bar at `/insights`.

### Page Layout

**Header row:**
- Title: "Monthly Insights"
- Subtitle: date range + "Generated Xh ago"
- Regenerate button (right-aligned)

**Headline banner:**
- Full-width card with Mercury's one-liner headline summary

**Four-section 2x2 grid:**

| What's Working (green) | What's Not Working (red) |
|---|---|
| **On Brand** (blue) | **Off Pattern** (yellow) |

Each section card contains:
- Color-coded icon + title
- Narrative summary paragraph
- Divider
- 1-2 cited posts with text preview + view count, linking to Threads permalink

**States:**
- **Loading:** Skeleton cards while fetching latest report
- **Empty:** "No insights yet" message with a "Generate Now" button (shown when no report exists)
- **Insufficient data:** "Not enough posts in the last 30 days for meaningful insights" (fewer than 5 analyzed posts)
- **Regenerating:** Loading overlay on the grid while Mercury is working (~5-15 seconds)

### Cited Post Links

Each cited post shows:
- Truncated post text (~80 chars)
- View count
- Links to the Threads permalink (opens in new tab)

The `GET /api/insights/latest` response includes the full report JSON. The frontend already fetches posts via `getPosts()` on other pages. On the Insights page, it fetches the post list alongside the report, then resolves cited post IDs to get text previews and permalinks. No new endpoint needed for this — just a client-side join.

## Error Handling

- **Mercury down or bad JSON:** Generation fails. Frontend continues showing the last successful report. The regenerate button shows an error toast.
- **Insufficient data:** If fewer than 5 analyzed posts in the last 30 days, skip nightly generation. Manual generation returns an error with a clear message.
- **Stale report:** No automatic expiration. Reports are replaced by newer ones. The "Generated Xh ago" timestamp lets the user judge freshness.

## Minimum Post Threshold

5 analyzed posts in the last 30 days. Below this, insights generation is skipped (nightly) or returns an error (manual). This prevents Mercury from generating meaningless analysis from too little data.

## Scope Boundaries

**In scope:**
- InsightsContext computation (new Rust function with SQL queries)
- New Mercury prompt for narrative analysis
- `insights_reports` table + migration
- Two new API endpoints
- Svelte page with 2x2 grid layout
- Nightly generation integration
- Frontend loading/empty/error states

**Out of scope:**
- Historical report browsing (only latest is shown)
- Section-level regeneration
- Custom date ranges (fixed at 30 days)
- Export or sharing of insights
