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

### Edge Computation

Edges connect subjects that share posts with the same intent patterns. Computed as:

For each pair of subjects (A, B), count posts where A and B both have posts with the same intent. Subjects frequently co-occurring via the same intent types are more related (e.g., "AI & LLMs" and "Software dev" are connected because both have many "Tip" and "Hot take" posts).

The `post_edges` table is repurposed to store subject-level edges instead of post-level edges.

### Graph Visualization

**Primary view: Subject Network Graph**
- 15-25 subject nodes (circles), sized by post count
- Edges between subjects based on shared intent patterns
- Node color from the subject's assigned color
- Each node shows: subject name, post count, average engagement
- Click a subject node → sidebar shows its posts sorted by engagement

**Intent as a visual facet:**
- Dropdown/toggle to filter graph by intent
- When filtered, node sizes update to reflect post count within that intent
- Answers: "what subjects do I joke about most?" or "what subjects do I rant about?"

**Post-level deep dive (secondary tab):**
- Only shows posts for a selected subject (not all 819)
- Nodes colored by intent (since subject is already filtered)
- Labels hidden by default, shown on hover
- Higher ForceAtlas2 gravity for tighter clusters

### Migration Strategy

1. Add new tables (`intents`, `subjects`) and columns (`intent_id`, `subject_id`) via SQL migration
2. Rewrite Mercury `analyze_posts()` to use the new prompt returning intent + subject
3. Update `analysis.rs` to upsert intents/subjects and set foreign keys on posts
4. Rewrite graph API endpoints to serve subject-level graph data
5. Update frontend Graph.svelte to render subject network with intent faceting
6. Run full reanalysis of all 819 posts
7. Drop old tables (`topics`, `post_topics`, `categories`) once verified
