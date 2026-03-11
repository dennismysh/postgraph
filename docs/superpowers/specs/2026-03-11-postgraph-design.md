# postgraph Design Spec

A Threads analytics platform that caches posts, analyzes them with an LLM, and visualizes a node graph of how posts connect by topic, theme, and relevance — alongside a full-featured analytics dashboard.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Backend language | Rust | Matches ndl reference project, strong async ecosystem |
| Backend framework | Axum (via Shuttle) | Shuttle provides ergonomic Rust deployment, first-class axum support |
| Frontend framework | Svelte (SvelteKit) | Lighter weight than React, clean d3/Sigma.js integration via direct DOM access |
| Frontend hosting | Netlify | Existing familiarity, good static/SSR hosting |
| Database | Shuttle Postgres | Built-in provisioning via `#[shuttle_shared_db::Postgres]`, zero external setup. Migrate to Supabase when multi-user needs arise |
| Analytics charts | Chart.js | Svelte-compatible, Canvas-based, good for time-series and bar/pie charts |
| Graph visualization | Sigma.js + Graphology | WebGL rendering handles 20k-50k nodes, ForceAtlas2 in WebWorker, filtering/search built-in |
| LLM for analysis | Mercury (Inception Labs) | Diffusion-based LLM, OpenAI API-compatible, fast inference for batch analysis |
| Auth (v1) | API key (env var) | Single-user for now; multi-user OAuth deferred |

## Architecture

Three layers:

1. **Rust API server** (Shuttle + axum) — Threads API sync, Mercury LLM analysis, graph edge computation, REST API for frontend
2. **Shuttle Postgres** — post cache, LLM analysis results, graph edges, engagement time-series
3. **Svelte frontend** (Netlify) — Sigma.js node graph, analytics charts, power-user filters

Data flow:

```
Threads API -> Rust backend (sync + cache) -> Shuttle Postgres
                    |
            Mercury LLM (analyze posts -> topics, themes, sentiment)
                    |
            Graph computation (build edges with weights)
                    |
            Shuttle Postgres (store graph + analytics)
                    |
            Svelte dashboard <- Rust API (REST, polled)
                    |
            Sigma.js node graph + Chart.js analytics
```

The Rust backend is the single source of truth for all processing. The frontend is purely a presentation layer that consumes pre-computed data via the Rust REST API (no direct Supabase connection in v1 — simplifies auth and avoids dual data paths).

## Threads API Details

Base URL: `https://graph.threads.net/v1.0/`

**Two-phase data retrieval:** Post content and engagement metrics come from separate endpoints:
- Phase 1: `GET /me/threads` — fetches post content (text, media, timestamp, permalink)
- Phase 2: `GET /{thread-media-id}/insights` — fetches engagement metrics per post (requires `threads_manage_insights` permission)

This means each sync requires 1 + N API calls (1 for the post list, N for insights per post).

**Token lifecycle:** Threads uses OAuth 2.0. Long-lived tokens last 60 days and must be refreshed before expiry. The backend stores the token in the database and refreshes it proactively (e.g. when < 7 days remain). Initial token obtained via the same hosted OAuth flow used by ndl.

**Rate limits:** Threads API has undocumented but real read rate limits. Strategy:
- Exponential backoff with jitter on 429 responses
- Insights calls are throttled (max ~5 concurrent, configurable)
- Sync runs are idempotent — if interrupted mid-batch, the next run picks up where it left off using `sync_state`

## Data Model

### posts
Cached Threads data.
- `id` (Threads post ID), `text`, `media_type`, `media_url`, `timestamp`, `permalink`
- `likes`, `replies_count`, `reposts`, `quotes` — engagement (updated from insights API)
- `sentiment` — float (-1.0 to 1.0), set by Mercury analysis
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
- `post_id`, `timestamp`, `likes`, `replies_count`, `reposts`, `quotes`

### sync_state
Bookkeeping for incremental Threads API pagination.
- `last_sync_cursor`, `last_sync_at`

## Sync & Analysis Pipeline

### Scheduling
Sync runs via a tokio background task (`tokio::time::interval`) spawned at server startup. The `POST /sync/trigger` endpoint also allows manual sync from the dashboard.

### Incremental sync (every 15 minutes)
1. Phase 1: Fetch new posts using `sync_state.last_sync_cursor` via `GET /me/threads`
2. Insert new posts with `analyzed_at = null`
3. Phase 2: Fetch insights for each post via `GET /{id}/insights` (throttled, with backoff)
4. Update engagement metrics on posts + append `engagement_snapshots` row
5. Update `sync_state` cursor
6. If interrupted, `sync_state` tracks progress — next run resumes from last cursor

### Analysis (runs after sync)
1. Query `posts WHERE analyzed_at IS NULL`
2. Batch to Mercury (10-20 posts per LLM call) for topic/theme/sentiment extraction
3. Mercury prompt includes the existing topic list — it maps posts to existing topics first, only creates new topics when no match exists. This is the normalization strategy: the LLM is the normalizer, with the existing topic catalog as context.
4. If Mercury returns malformed JSON or errors, the post is skipped and retried on the next analysis run (stays `analyzed_at = null`)
5. Upsert into `topics` and `post_topics`, set `sentiment` on post
6. Set `analyzed_at = now()` on successfully processed posts

### Edge computation (runs after analysis)
1. For each newly analyzed post, find related posts via SQL join on `post_topics` (shared topics)
2. Edge weight = sum of (topic_weight_a * topic_weight_b) for shared topics, normalized
3. Prune weak edges below a configurable threshold (default 0.1) to prevent quadratic edge growth
4. Upsert into `post_edges` — only for newly analyzed posts, not the entire graph
5. Indexes on `post_topics(post_id)`, `post_topics(topic_id)`, `post_edges(source_post_id)`, `post_edges(target_post_id)` for query performance

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
- Types live in `postgraph-server` for now — extract a `postgraph-core` crate when a second consumer (Android client) materializes
- `threads.rs` borrows ndl's reqwest-based API client patterns (access_token query param, error types)
- `mercury.rs` is an OpenAI-compatible client (POST to /v1/chat/completions)
- Routes split by domain, not HTTP method
- Svelte app in `web/`, outside Cargo workspace, own build pipeline
- CORS middleware on axum required (Svelte on Netlify -> Rust on Shuttle is cross-origin)
- Database migrations via `sqlx migrate`

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
- Chart.js (analytics charts)

## Reference

- ndl project (github.com/pgray/ndl) — reference for reqwest-based Threads API client and Rust workspace structure
- Threads API: base URL `https://graph.threads.net/v1.0/`, read endpoints: GET /me, GET /me/threads, GET /{id}/replies, GET /{id}/insights
- Mercury API: OpenAI-compatible, POST to /v1/chat/completions
- Edition 2024 Rust, following ndl's code conventions

### Graph endpoint payload
For large accounts (20k+ posts), `GET /graph` should support pagination or level-of-detail: return cluster summaries by default, expand to individual posts on zoom. This avoids multi-megabyte payloads.

## Future

- Multi-user: migrate to Supabase (Auth + RLS + real-time), Threads OAuth
- Android client: extract postgraph-core crate, share types via UniFFI, native Compose UI
- Supabase migration: real-time subscriptions, Auth, row-level security when multi-user justifies the complexity
- Additional platforms beyond Threads
