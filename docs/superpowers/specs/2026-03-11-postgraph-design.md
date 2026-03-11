# postgraph Design Spec

A Threads analytics platform that caches posts, analyzes them with an LLM, and visualizes a node graph of how posts connect by topic, theme, and relevance — alongside a full-featured analytics dashboard.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Backend language | Rust | Matches ndl reference project, strong async ecosystem |
| Backend framework | Axum (via Shuttle) | Shuttle provides ergonomic Rust deployment, first-class axum support |
| Frontend framework | Svelte (SvelteKit) | Lighter weight than React, clean d3/Sigma.js integration via direct DOM access |
| Frontend hosting | Netlify | Existing familiarity, good static/SSR hosting |
| Database | Supabase (Postgres) | Direct sqlx connection from Rust, real-time subscriptions for frontend, built-in auth for future multi-user |
| Graph visualization | Sigma.js + Graphology | WebGL rendering handles 20k-50k nodes, ForceAtlas2 in WebWorker, filtering/search built-in |
| LLM for analysis | Mercury (Inception Labs) | Diffusion-based LLM, OpenAI API-compatible, fast inference for batch analysis |
| Auth (v1) | API key (env var) | Single-user for now; multi-user OAuth deferred |

## Architecture

Three layers:

1. **Rust API server** (Shuttle + axum) — Threads API sync, Mercury LLM analysis, graph edge computation, REST API for frontend
2. **Supabase Postgres** — post cache, LLM analysis results, graph edges, engagement time-series
3. **Svelte frontend** (Netlify) — Sigma.js node graph, analytics charts, power-user filters

Data flow:

```
Threads API -> Rust backend (sync + cache) -> Supabase Postgres
                    |
            Mercury LLM (analyze posts -> topics, themes, sentiment)
                    |
            Graph computation (build edges with weights)
                    |
            Supabase Postgres (store graph + analytics)
                    |
            Svelte dashboard <- Rust API (REST) + Supabase (real-time)
                    |
            Sigma.js node graph + analytics charts
```

The Rust backend is the single source of truth for all processing. The frontend is purely a presentation layer that consumes pre-computed data.

## Data Model

### posts
Cached Threads data.
- `id` (Threads post ID), `text`, `media_type`, `media_url`, `timestamp`, `permalink`
- `likes`, `replies_count`, `reposts`, `quotes` — engagement snapshots
- `synced_at` — when last pulled from Threads API
- `analyzed_at` — when Mercury last analyzed this post (null = pending analysis)

### topics
LLM-extracted topics/themes, normalized (e.g. "Rust programming" and "writing Rust code" collapse into one).
- `id`, `name`, `description`

### post_topics
Many-to-many with relevance weight.
- `post_id`, `topic_id`, `weight` (0.0-1.0)

### post_edges
Pre-computed graph connections.
- `source_post_id`, `target_post_id`, `edge_type` (topic_overlap, theme, reply_chain, temporal_proximity), `weight`

### engagement_snapshots
Time-series for analytics charts. Captured on each sync.
- `post_id`, `timestamp`, `likes`, `replies`, `reposts`, `quotes`

### sync_state
Bookkeeping for incremental Threads API pagination.
- `last_sync_cursor`, `last_sync_at`

## Sync & Analysis Pipeline

### Incremental sync (scheduled, e.g. every 15 minutes)
1. Fetch only new posts using `sync_state.last_sync_cursor`
2. Insert new posts with `analyzed_at = null`
3. Update engagement metrics on existing posts + append `engagement_snapshots` row
4. Update `sync_state` cursor

### Analysis (runs after sync)
1. Query `posts WHERE analyzed_at IS NULL`
2. Batch to Mercury (10-20 posts per LLM call) for topic/theme/sentiment extraction
3. Upsert into `topics` and `post_topics` — new topics created, existing matched
4. Set `analyzed_at = now()` on processed posts

### Edge computation (runs after analysis)
1. For each newly analyzed post, compute edges to all other posts based on shared topics and weights
2. Calculate overlap scores, upsert into `post_edges`
3. Only recomputes for newly analyzed posts, not entire graph

### Re-analysis
Not automatic. Post text doesn't change on Threads. Manual "re-analyze all" nulls `analyzed_at` across the board (for prompt improvements, model changes).

### Post lifecycle
```
New post from Threads API
  -> synced (engagement tracked)
    -> analyzed by Mercury (topics extracted)
      -> edges computed (graph connections built)
        -> available in dashboard
```

## Project Structure

```
postgraph/
├── Cargo.toml                 # Workspace manifest
├── postgraph-server/          # Axum API server (Shuttle)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs            # Shuttle entry point, axum router
│       ├── routes/
│       │   ├── mod.rs
│       │   ├── graph.rs       # GET /graph (nodes + edges for Sigma)
│       │   ├── posts.rs       # GET /posts, filters, sorting
│       │   ├── analytics.rs   # GET /analytics (aggregated metrics)
│       │   └── sync.rs        # POST /sync/trigger (manual sync)
│       ├── threads.rs         # Threads API client (modeled after ndl's api.rs)
│       ├── mercury.rs         # Mercury LLM client (OpenAI-compatible HTTP)
│       ├── analysis.rs        # Post analysis orchestration + topic extraction
│       ├── graph.rs           # Edge computation logic
│       └── db.rs              # sqlx queries, migrations
├── postgraph-core/            # Shared types (future Android client reuse)
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs             # Post, Topic, Edge, GraphData structs
└── web/                       # Svelte frontend (outside Cargo workspace)
    ├── package.json
    ├── src/
    │   ├── lib/
    │   │   ├── components/
    │   │   │   ├── Graph.svelte       # Sigma.js wrapper
    │   │   │   ├── Dashboard.svelte   # Analytics charts
    │   │   │   └── FilterBar.svelte   # Power-user filters/sorting
    │   │   ├── stores/
    │   │   │   └── graph.ts           # Graphology store + API fetching
    │   │   └── api.ts                 # Rust backend client
    │   └── routes/                    # SvelteKit pages
    └── netlify.toml
```

Key structural decisions:
- `postgraph-core` shares types for future Android client (via UniFFI or similar)
- `threads.rs` mirrors ndl's `api.rs` patterns (reqwest + access_token query param)
- `mercury.rs` is an OpenAI-compatible client (POST to /v1/chat/completions)
- Routes split by domain, not HTTP method
- Svelte app in `web/`, outside Cargo workspace, own build pipeline

## Frontend Dashboard

### Graph View (centerpiece)
- Sigma.js rendering a Graphology graph — each node is a post, edges are topic/theme connections
- Node size = engagement score (weighted likes + replies + reposts)
- Node color = primary topic cluster (auto-colored by Graphology community detection)
- Edge thickness = connection weight
- Click node -> sidebar with post detail, topics, engagement stats, connected posts
- FilterBar: filter by topic, date range, engagement threshold, edge type (updates graph in real-time)
- Search: find posts by text, highlight matching nodes
- Zoom into clusters to explore topic neighborhoods

### Analytics View
- Time-series charts: engagement over time, posting frequency, likes/replies/reposts trends
- Topic breakdown: bar/pie charts showing topic distribution and engagement per topic
- Best/worst performing posts with sortable columns
- Sentiment trends over time
- All charts respect the same global filters as the graph view

### Shared power-user features
- Global filter bar persists across view switches
- URL state: filters and view state encoded in URL params (bookmarkable views)
- Keyboard shortcuts (topic cycling, quick filters)
- Export: graph data as JSON, analytics as CSV

### Auth (v1)
API key in environment variable. Svelte app sends as Bearer token. Multi-user OAuth deferred.

## Key Technologies

### Rust backend
- axum (HTTP framework, via Shuttle)
- sqlx (Postgres, compile-time checked queries)
- reqwest + rustls (HTTP client for Threads API and Mercury)
- serde + serde_json (serialization)
- tokio (async runtime)
- thiserror (error types)

### Svelte frontend
- SvelteKit
- Sigma.js + Graphology (graph visualization)
- graphology-layout-forceatlas2 (WebWorker-based layout)
- Chart.js or visx (analytics charts)
- Supabase JS client (real-time subscriptions)

## Reference

- ndl project (github.com/pgray/ndl) for Threads API patterns and Rust workspace structure
- Threads API: base URL `https://graph.threads.net`, endpoints: GET /me, GET /me/threads, GET /{id}/replies, POST /me/threads, POST /me/threads_publish
- Mercury API: OpenAI-compatible, POST to /v1/chat/completions
- Edition 2024 Rust, following ndl's code conventions

## Future

- Multi-user: Supabase Auth + Threads OAuth, row-level security
- Android client: share postgraph-core types, native Compose UI
- Additional platforms beyond Threads
