# Topic Categories Design Spec

Add a category hierarchy to the existing flat topic system. Mercury LLM discovers broad categories by grouping existing topics, providing stable meaningful coloring for both graphs, category-based filtering, and reduced visual noise.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Category discovery | Mercury LLM (second-pass) | Fully automated, LLM sees all topics at once for coherent groupings |
| Number of categories | LLM-determined | Let natural groupings emerge from the data rather than forcing a fixed count |
| Category assignment | Separate categorization pass | Clean separation from post analysis; categories see the full topic landscape |
| Graph display | Topics visible, colored by category | Upgrades Louvain's arbitrary colors with stable, labeled groupings |
| Timing | Incremental + manual recategorize | New topics assigned to existing categories during analysis; full regrouping on demand |
| Scope | Both graphs + category filter | Post graph colored by dominant category, tag graph colored by category, FilterBar gets category dropdown |

## Database Schema

### New table: `categories`

```sql
CREATE TABLE categories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    color TEXT  -- hex color, assigned server-side from a predefined palette
);
```

### Modified table: `topics`

```sql
ALTER TABLE topics ADD COLUMN category_id UUID REFERENCES categories(id) ON DELETE SET NULL;
```

- Each topic belongs to at most one category (nullable FK)
- `ON DELETE SET NULL` — if a category is deleted, topics revert to uncategorized
- Uncategorized topics render in neutral gray on the frontend
- No separate join table — one topic, one category

### Migration file: `005_add_categories.sql`

Contains the `CREATE TABLE categories` and `ALTER TABLE topics` statements above. Runs automatically on startup via `sqlx::migrate!()`.

### Color palette

The frontend's existing `COLORS` array (in `TagGraph.svelte` and `Graph.svelte`) moves server-side, expanded to 15 visually distinct hex colors:

```
#e6194b, #3cb44b, #4363d8, #f58231, #911eb4,
#42d4f4, #f032e6, #bfef45, #fabed4, #469990,
#dcbeff, #9A6324, #800000, #aaffc3, #808000
```

When categories are created, colors are assigned in order from the palette. On recategorization, existing categories that survive (matched by name) keep their existing color from the DB; only genuinely new categories get the next unused color from the palette. The frontend reads colors from the API response instead of using its hardcoded array.

## Mercury LLM Integration

### Full categorization (`categorize_topics`)

New function in `mercury.rs`. Sends all topic names and descriptions to Mercury with a prompt instructing it to group them into broad categories. Mercury returns JSON:

```json
{
  "categories": [
    {
      "name": "Technology",
      "description": "Software, AI, programming, and tech industry",
      "topics": ["AI", "Rust", "Open Source", "Web Development"]
    }
  ]
}
```

The server then:
1. Upserts categories (creates new, updates description if name matches)
2. Assigns colors from the palette
3. Updates each topic's `category_id` based on the mapping
4. Orphaned categories (no topics after regrouping) are deleted

### Incremental assignment (`assign_topic_category`)

When post analysis creates a new topic, a lightweight Mercury call sends the new topic name + the list of existing category names and descriptions. The prompt:

> "Given the topic '{topic_name}' and these existing categories: {categories_json}, which category does this topic belong to? Return JSON: {\"category\": \"<category_name>\"}"

Mercury returns:
```json
{"category": "Technology"}
```

The topic's `category_id` is set to the matching category. If Mercury returns a category name that doesn't match any existing category, the topic remains uncategorized (`category_id = NULL`) — it will be assigned on the next full recategorization.

If no categories exist yet (first run), incremental assignment is skipped — a full categorization runs instead.

### Triggering

- **Manual:** "Recategorize" button in the UI triggers `POST /api/categorize`
- **Auto on first analysis:** If no categories exist after post analysis completes, a full categorization runs automatically
- **Incremental:** During `run_analysis()`, after upserting a new topic, `assign_topic_category()` is called
- **After reanalysis:** `reset_all_analysis()` deletes all topics (and cascading deletes clear `post_topics`). Since the topics rows are deleted entirely, the categories table is unaffected — categories survive with no associated topics. After reanalysis creates new topics (with `category_id = NULL` by default), a full recategorization runs automatically to reassign them.

### Concurrency

`POST /api/categorize` uses a `categorize_running: Arc<AtomicBool>` guard on `AppState` (same pattern as `analysis_running`). If analysis is currently running when categorization is triggered, the endpoint returns immediately with `{"error": "analysis in progress, try again after it completes"}` and HTTP 409. The frontend can check `GET /api/analyze/status` and retry. Incremental assignment during analysis is fine since it only reads existing categories.

The auto-triggered categorization after reanalysis is an internal call within the reanalysis background task (sequenced after analysis completes), so it bypasses the `categorize_running` HTTP guard — it sets the flag directly to prevent concurrent manual triggers.

### Route file

New file: `routes/categorize.rs` — contains handlers for `POST /api/categorize`, `GET /api/categorize/status`, and `GET /api/categories`.

## API Changes

### New endpoints

- `POST /api/categorize` — triggers full categorization of all topics. Runs in background with progress tracking. Protected by API key auth. Requires new `AppState` fields: `categorize_running: Arc<AtomicBool>`, `categorize_progress: Arc<AtomicU32>`, `categorize_total: Arc<AtomicU32>`.
- `GET /api/categorize/status` — returns categorization progress: `{"running": bool, "progress": u32, "total": u32}`. Same pattern as `GET /api/analyze/status`.
- `GET /api/categories` — returns all categories with their assigned topic names, descriptions, and colors:
  ```json
  {
    "categories": [
      {
        "id": "uuid",
        "name": "Technology",
        "description": "Software, AI, programming, and tech industry",
        "color": "#4363d8",
        "topics": ["AI", "Rust", "Open Source", "Web Development"]
      }
    ]
  }
  ```

### Modified endpoints

- `GET /api/graph` — post graph nodes now include `category` object (`name`, `color`) derived from the post's dominant category (the category with the highest total topic weight for that post). Accepts optional `?category=<name>` query param to filter posts by category.
- `GET /api/graph/tags` — tag graph nodes now include `category_id`, `category_name`, and `category_color` fields. Frontend uses these for coloring instead of Louvain community detection. Category filtering on the tag graph is done client-side — each node already has its `category_name`, so the frontend's `nodeReducer` can hide/fade nodes outside the selected category without a server round-trip.

### Unchanged endpoints

`/api/posts`, `/api/analytics`, `/api/sync` — categories are a graph/visualization concern and don't affect these endpoints.

## Frontend Changes

### Tag graph (`TagGraph.svelte`)

- **Coloring:** Replace Louvain community detection (`louvain.assign(graph)`) with category-based coloring. Each node's color comes from its `category_color` field in the API response.
- **Legend:** Render a category legend below the graph — a row of colored dots with category names. Clicking a legend item acts as a quick filter.
- **Uncategorized:** Topics without a category render in neutral gray (`#888`).
- **ForceAtlas2 layout stays** — same-category topics naturally cluster because they share more co-occurrence edges.

### Post graph (`Graph.svelte`)

- **Coloring:** Replace Louvain community detection with category-based coloring. Each post node's color comes from its dominant category.
- **Legend:** Same category legend as the tag graph.
- **Category filter:** When a category is selected (via FilterBar or legend click), `nodeReducer` hides posts outside that category.

### FilterBar

- **Category dropdown:** New dropdown filter listing all categories. Adds a `category: string | null` field to the `Filters` store (initialized to `null`, reset to `null` by `resetFilters()`). The category filter is independent of the existing topic filter — both can be active simultaneously. Selecting a category does not auto-populate the topic filter; they compose with AND logic (e.g., category="Technology" AND topic="Rust" shows only posts matching both). `Graph.svelte`'s `nodeMatchesFilters` function updated to check the category field.
- **"Recategorize" button:** Next to the existing "Reanalyze" button. Triggers `POST /api/categorize`. Shows the same progress bar pattern used by sync and analyze.

### Dashboard analytics

- **Top Topics chart:** Bar colors match the topic's category color (instead of a single color).

### Uncategorized state

If no categories exist (topics haven't been categorized yet):
- Graphs render with neutral gray nodes (no Louvain fallback)
- A subtle prompt appears near the Recategorize button: "Run categorization to group topics"

## Dominant category computation

A post's dominant category is computed server-side when building the graph response:

```
For each post:
  1. Get all (topic, weight) pairs from post_topics
  2. Group by category_id (via topic's FK)
  3. Sum weights per category
  4. The category with the highest total weight is the dominant category
  5. If no topics have categories, the post is uncategorized
```

This is computed on-the-fly — not stored in the database. It's derived data that changes when topics get recategorized. Implementation: the existing `get_graph()` query already fetches all `post_topics` into a `topic_map: HashMap<String, Vec<TopicWeight>>`. Augment this by joining `topics.category_id -> categories` in the query, then compute dominant category in Rust by grouping the topic weights by category and picking the max.

## Data flow

```
Existing flow (unchanged):
  Threads API -> sync -> posts table
  posts -> Mercury analysis -> topics + post_topics
  post_topics -> edge computation -> post_edges

New categorization flow:
  topics (all) -> Mercury categorize_topics -> categories table + topics.category_id
  new topic (during analysis) -> Mercury assign_topic_category -> topics.category_id

Graph serving (modified):
  GET /api/graph/tags -> nodes include category_name, category_color
  GET /api/graph -> nodes include dominant category (computed from post_topics + topics.category_id)
```

## Error handling

- If Mercury categorization fails (malformed JSON, API error), the operation fails gracefully — existing categories are preserved, no topics are modified. The error is reported to the frontend via the progress endpoint.
- If incremental assignment fails for a new topic, the topic is created without a category (`category_id = NULL`). It will be assigned on the next full recategorization.
- If a topic name from Mercury's response doesn't match any existing topic, it's silently skipped (Mercury may hallucinate topic names).
