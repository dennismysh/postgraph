# Taxonomy & Graph Visualization Redesign

## Problem

The current analysis pipeline produces 44+ granular one-off topics ("Coffee humor", "Parking preference", "Brain fog coping") grouped into 6 overly generic categories ("Creative & Insightful Thoughts"). The post-level graph is an unreadable hairball of 819 nodes with overlapping labels, no spatial meaning, and no hierarchy.

## Goals

1. **Content identity** — what themes do I post about most, and how do they connect?
2. **Performance correlation** — which types of posts perform best?
3. **Evolution tracking** — how has my content focus shifted over time?

## Design

### Two-Layer Taxonomy Model

Every post gets exactly **one intent** and **one subject**.

**Intent** (6-10 emergent, what the post is trying to do):
Seed examples: Question, Hot take, Humor, Story, Tip, Hype, Rant, Observation, Promotion.

**Subject** (15-25 emergent, what the post is about):
Seed examples: AI & LLMs, Software dev, Side projects, Social media, Productivity, Daily life, Gaming, Career, Health, Culture, Tech industry, Politics.

**Approach: Seed Examples + Emergence.** The LLM uses seed tags when they fit and creates new ones at the same granularity level when they don't. Seeds calibrate without constraining.

### LLM Analysis Prompt

```
You are analyzing social media posts for a content analytics platform.

For each post, extract:
1. **Intent** — what the post is trying to do (one per post)
2. **Subject** — what the post is about (one per post)
3. **Sentiment** — emotional tone (-1.0 to 1.0)

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

You may create new intents if a post genuinely doesn't fit any of these,
but apply the reusability test first.

## Subject (pick exactly one)
The topic domain of the post. Seed examples:
- AI & LLMs, Software dev, Side projects, Social media,
  Productivity, Daily life, Gaming, Career, Health,
  Culture, Tech industry, Politics

You may create new subjects at this same granularity level.

## Rules
1. REUSABILITY TEST: Before creating a new intent or subject, ask:
   "Would this apply to at least 10 posts from a typical creator?"
   If no, use a broader existing tag.
2. NO COMPOUND TAGS: "Coffee humor" is wrong. That's intent=Humor,
   subject=Daily life.
3. PREFER EXISTING: Always reuse an existing intent/subject before
   creating a new one.
4. SHORT NAMES: Max 3 words per tag.
5. NEVER describe a single post's specific content as a tag.
   "UNO house rules" → subject=Gaming, intent=Question.
   "Parking preference" → subject=Daily life, intent=Question.

Existing intents: {intents_list}
Existing subjects: {subjects_list}

Posts: {posts_json}

Respond with ONLY valid JSON:
{"posts": [{"post_id": "...", "intent": "...", "subject": "...",
  "sentiment": 0.5}]}
```

No separate categorization LLM call is needed. The old flow (analyze → extract topics → categorize topics into groups) is replaced by a single call that outputs intent + subject directly.

### Data Model Changes

**Current model:**
```
post → post_topics → topics → categories
```

**New model:**
```
post.intent_id → intents (id, name, description, color)
post.subject_id → subjects (id, name, description, color)
```

New tables:
- `intents` — id (UUID PK), name (TEXT UNIQUE), description (TEXT), color (TEXT)
- `subjects` — id (UUID PK), name (TEXT UNIQUE), description (TEXT), color (TEXT)

New columns on `posts`:
- `intent_id` (UUID FK → intents, nullable until analyzed)
- `subject_id` (UUID FK → subjects, nullable until analyzed)

Old tables become unused: `topics`, `post_topics`, `categories`. Drop them after migration is verified.

Old columns on `posts` to keep: `sentiment`, `analyzed_at` (repurposed for the new analysis).

**Color assignment:** Both intents and subjects get colors assigned server-side from the existing `CATEGORY_COLORS` palette when first inserted. The upsert function checks if the row already exists; if inserting a new row, it assigns the next unused color from the palette.

**Description column:** Populated from the seed example descriptions for seed tags. For LLM-created tags, set to an empty string — descriptions are not critical for functionality and can be manually edited later.

### Edge Computation

Edges connect subjects that have similar intent distributions. For each pair of subjects (A, B), compute the number of shared intent types:

```sql
-- Count how many distinct intents appear in BOTH subject A and subject B
SELECT COUNT(*) AS shared_intents
FROM (
    SELECT DISTINCT intent_id FROM posts WHERE subject_id = $1
    INTERSECT
    SELECT DISTINCT intent_id FROM posts WHERE subject_id = $2
) shared;
```

Edge weight = `shared_intents / total_intents` (Jaccard-like similarity). Two subjects are connected if they share at least 2 intent types. This means "AI & LLMs" and "Software dev" are strongly connected if both have Question, Tip, Hot take, and Hype posts, while "Gaming" and "Politics" may not connect at all if they have disjoint intent patterns.

**Storage:** A new `subject_edges` table replaces `post_edges`:
```sql
CREATE TABLE subject_edges (
    source_subject_id UUID NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    target_subject_id UUID NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    weight REAL NOT NULL,
    shared_intents INTEGER NOT NULL,
    PRIMARY KEY (source_subject_id, target_subject_id)
);
```

The old `post_edges` table is dropped in the migration.

### Graph Visualization

**Primary view: Subject Network Graph**
- 15-25 subject nodes (circles), sized by post count
- Edges between subjects based on shared intent patterns (from `subject_edges`)
- Node color from the subject's assigned color
- Each node shows: subject name, post count, average engagement
- Click a subject node → sidebar shows its posts sorted by engagement

**Intent as a visual facet:**
- Dropdown/toggle to filter graph by intent
- When filtered, node sizes update to reflect post count within that intent
- Answers: "what subjects do I joke about most?" or "what subjects do I rant about?"

**Post-level deep dive (deferred to follow-up):**
The post-level graph within a subject is explicitly out of scope for this iteration. The sidebar post list provides sufficient drill-down. A future iteration can add a post-level graph view filtered by subject if needed.

### API Endpoints

**`GET /api/graph`** — Subject network graph (replaces current post graph):
```json
{
  "nodes": [
    {
      "id": "uuid",
      "label": "AI & LLMs",
      "post_count": 142,
      "avg_engagement": 45.3,
      "color": "#4363d8"
    }
  ],
  "edges": [
    {
      "source": "uuid-a",
      "target": "uuid-b",
      "weight": 0.75,
      "shared_intents": 6
    }
  ],
  "intents": [
    { "id": "uuid", "name": "Question", "color": "#e6194b", "post_count": 89 }
  ]
}
```

Query params: `?intent=Question` — filter node sizes to only count posts with that intent.

**`GET /api/subjects/{id}/posts`** — Posts for a subject (sidebar drill-down):
```json
{
  "subject": "AI & LLMs",
  "posts": [
    {
      "id": "post-id",
      "text": "...",
      "intent": "Hot take",
      "engagement": 234,
      "views": 5000,
      "timestamp": "2026-03-10T..."
    }
  ]
}
```

Query params: `?intent=Question` — filter to only posts with that intent.

**`GET /api/analytics`** — Updated to return intent/subject breakdowns instead of topic summaries. Replace `total_topics` and `topics` array with `total_subjects`, `subjects`, `total_intents`, `intents`.

### Downstream Changes

- **`reset_all_analysis` in `db.rs`:** Update to clear `intent_id`/`subject_id` on posts, delete from `subject_edges`, and optionally clear `intents`/`subjects` tables.
- **`/api/reanalyze` route:** Remove the auto-categorization trigger (no longer needed).
- **`/api/categorize` and `/api/categories` routes:** Remove entirely.
- **`AppState`:** Remove `categorize_running`/`categorize_progress`/`categorize_total` fields.
- **Background sync and nightly sync in `main.rs`:** Update to call new analysis pipeline; remove edge computation step (edges recomputed after full reanalysis, not incrementally per-post).
- **FilterBar.svelte:** Update to show intent/subject dropdowns instead of topic/category pills.

### Migration Strategy

1. SQL migration: create `intents`, `subjects`, `subject_edges` tables; add `intent_id`, `subject_id` columns to `posts`
2. Rewrite Mercury `analyze_posts()` to use the new prompt returning intent + subject
3. Update `analysis.rs` to upsert intents/subjects and set foreign keys on posts
4. Rewrite graph API endpoints to serve subject-level graph data
5. Add `/api/subjects/{id}/posts` endpoint
6. Update analytics endpoint to return intent/subject breakdowns
7. Update frontend Graph.svelte to render subject network with intent faceting
8. Update FilterBar.svelte for intent/subject filtering
9. Remove categorization routes, AppState fields, and old analysis code
10. Run full reanalysis of all posts
11. SQL migration: drop old tables (`topics`, `post_topics`, `categories`, `post_edges`) once verified
