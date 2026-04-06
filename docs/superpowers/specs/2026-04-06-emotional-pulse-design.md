# Emotional Pulse — Design Spec

**Date:** 2026-04-06
**Status:** Approved

## Overview

Add an "Emotional Pulse" section to the Insights page that classifies each post with a creator-oriented emotion, visualizes the 30-day emotional profile as a radar chart, and generates Mercury-powered narrative insights correlating emotion with audience engagement.

## Emotion Taxonomy

Fixed set of 7 creator-oriented emotions (one per post, mutually exclusive):

| Emotion | Description |
|---|---|
| Vulnerable | Openness, personal sharing, admitting uncertainty |
| Curious | Questions, exploration, wonder |
| Playful | Humor, wit, lightheartedness |
| Confident | Strong opinions, assertions, expertise |
| Reflective | Introspection, lessons learned, looking back |
| Frustrated | Venting, complaints, friction |
| Provocative | Hot takes, challenging norms, debate-starting |

## Classification

Extend the existing `analyze_posts` Mercury prompt to add a 4th field alongside intent, subject, and sentiment:

```json
{"post_id": "...", "intent": "...", "subject": "...", "sentiment": 0.5, "emotion": "curious"}
```

The prompt gets a new "Emotion" section listing the 7 emotions with descriptions, same format as the existing intent/subject sections. Mercury picks exactly one per post. The emotion is stored in a new `emotion TEXT` column on the `posts` table (nullable).

## Backend API

### `GET /api/emotions/summary`

Pure SQL aggregation — `GROUP BY emotion` with engagement metrics, filtered to the 30-day window. No Mercury call.

Response:

```json
{
  "window_start": "2026-03-07",
  "window_end": "2026-04-06",
  "total_posts": 42,
  "emotions": [
    {
      "name": "curious",
      "post_count": 12,
      "percentage": 28.6,
      "avg_views": 1450.0,
      "avg_likes": 8.3,
      "avg_replies": 3.1,
      "avg_reposts": 1.2,
      "top_post_id": "abc123"
    }
  ]
}
```

### `GET /api/emotions/narrative`

Returns the latest stored emotion narrative report.

### `POST /api/emotions/narrative/generate`

Triggers Mercury to generate a creator-focused emotion narrative. Receives the aggregated emotion x engagement data as context. Mercury prompt focuses on:

- Which emotions the audience responds to most
- Which emotions get reach but not engagement (or vice versa)
- Emotional range assessment — one-note or diverse?
- Actionable observations grounded in the data

Response structure:

```json
{
  "id": "...",
  "generated_at": "...",
  "trigger_type": "manual",
  "narrative": {
    "headline": "Your audience loves your curiosity but ignores your confidence",
    "observations": [
      {
        "text": "Curious posts average 1,450 views — 2x your overall average",
        "cited_posts": ["abc123"],
        "emotion": "curious"
      }
    ]
  }
}
```

### `POST /api/emotions/backfill`

One-time backfill for existing analyzed posts that lack an emotion tag. Uses a lightweight dedicated Mercury call (just post texts + 7-emotion list, returns `[{post_id, emotion}]`). Runs in batches of 16 matching existing `ANALYSIS_BATCH_SIZE`.

## Database Changes

### Migration: Add emotion column to posts

```sql
ALTER TABLE posts ADD COLUMN emotion TEXT;
```

### Migration: Create emotion_narratives table

```sql
CREATE TABLE emotion_narratives (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    trigger_type TEXT NOT NULL,
    narrative JSONB NOT NULL,
    context JSONB NOT NULL
);
```

## Frontend

### Location

Below the existing 4-card insights grid on the Insights page. Independent loading — does not depend on the insights report.

### Radar Chart

Chart.js radar type (already a project dependency). 7 axes, one per emotion. Values are the percentage of posts in each emotion. Dark theme styling consistent with existing cards — dark fill with subtle colored border, axis labels in muted text.

### Narrative Card

Full-width card below the radar chart. Displays:

- Headline: one sentence summarizing the most important emotion x engagement finding
- 3-5 observations, each grounded in emotion x engagement data (Mercury prompt enforces this range)
- Cited posts where relevant (same `-> post text . 1,234 views` pattern as existing insights)

### Loading & Empty States

- Independent skeleton state while loading (separate from insights report)
- If no emotion data exists yet (pre-backfill), shows empty state with "Generate" button
- Separate "Regenerate" button for the emotion narrative only

## Backfill & Migration Strategy

- New `emotion TEXT` column on `posts` (nullable)
- New `emotion_narratives` table (same pattern as `insights_reports`)
- Backfill via `POST /api/emotions/backfill` — lightweight Mercury call for posts with `analyzed_at IS NOT NULL AND emotion IS NULL`, batched at 16
- New posts get emotion during normal analysis pass (extended prompt)
- Nightly: emotion narrative generates after insights report, sequentially (avoid Mercury rate limits)
