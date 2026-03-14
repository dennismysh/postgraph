# postgraph

A Threads analytics platform that syncs your posts, analyzes them with an LLM, and visualizes the relationships between them as an interactive graph.

## What it does

postgraph connects to your [Threads](https://www.threads.net) account and continuously syncs your posts, then uses [Mercury](https://inceptionlabs.ai/) (Inception Labs' diffusion LLM) to extract topics, sentiment, and thematic relationships. The result is an interactive graph where each node is a post and edges represent shared topics — letting you see how your content connects at a glance.

**Key features:**

- **Automatic sync** — pulls new posts and engagement metrics every 15 minutes
- **LLM-powered analysis** — extracts topics, sentiment, and categories from post text
- **Topic graph** — computes weighted edges between posts based on shared topics
- **Interactive visualization** — explore your post graph with WebGL-powered rendering
- **Engagement tracking** — time-series snapshots of views, likes, replies, reposts, quotes, and shares
- **Analytics dashboard** — charts and aggregate metrics for your content performance

## Architecture

```
┌─────────────┐      ┌──────────────────┐      ┌────────────┐
│  Threads API │◄────│  Rust Backend     │────►│  PostgreSQL │
│  (data sync) │     │  (axum + tokio)   │     │  (Railway)  │
└─────────────┘      │                    │     └────────────┘
                      │  Background sync  │
┌─────────────┐      │  every 15 min     │
│  Mercury LLM │◄────│                    │
│  (analysis)  │     └────────┬───────────┘
└─────────────┘               │ REST API
                      ┌───────┴───────────┐
                      │  Svelte Frontend   │
                      │  (SvelteKit)       │
                      │  Sigma.js + Charts │
                      └───────────────────┘
```

- **`postgraph-server/`** — Rust API server (axum, sqlx, tokio), deployed to Railway
- **`web/`** — Svelte frontend (SvelteKit, Sigma.js, Chart.js), deployed to Netlify
- **PostgreSQL** — provisioned by Railway

## Tech stack

| Layer | Technology |
|-------|-----------|
| Backend | Rust, axum, tokio, sqlx, reqwest |
| Frontend | Svelte 5, SvelteKit, TypeScript |
| Graph visualization | Sigma.js 3, Graphology, ForceAtlas2 |
| Charts | Chart.js |
| Database | PostgreSQL |
| LLM | Mercury 2 (Inception Labs, OpenAI-compatible API) |
| Deployment | Railway (backend + DB), Netlify (frontend) |

## How it works

### Data pipeline

1. **Sync** — The backend fetches posts from the Threads API using paginated cursors and collects engagement metrics (views, likes, replies, reposts, quotes, shares) for each post. Engagement snapshots are stored for time-series tracking.

2. **Analysis** — Unanalyzed posts are sent to Mercury in batches. The LLM extracts topics (with descriptions and relevance weights) and sentiment scores. Existing topic names are included in the prompt so the LLM reuses them rather than creating duplicates.

3. **Edge computation** — For each analyzed post, the backend finds other posts that share topics and computes edge weights as the sum of shared topic weight products. Edges above a threshold are stored in the database.

4. **Visualization** — The frontend fetches pre-computed graph data and renders it with Sigma.js (WebGL). Nodes are sized by engagement and colored by dominant topic category. ForceAtlas2 layout runs in a WebWorker.

### Background orchestration

A background task runs on a 15-minute interval:
- Refreshes the Threads OAuth token if it expires within 7 days
- Syncs new posts and refreshes metrics for all existing posts
- Runs LLM analysis in batches (with retry logic for failures)
- Computes edges for recently analyzed posts

## Getting started

### Prerequisites

- Rust (edition 2024)
- Node.js
- PostgreSQL
- A [Threads API](https://developers.facebook.com/docs/threads) access token
- A [Mercury API](https://inceptionlabs.ai/) key (Inception Labs)

### Setup

1. **Clone the repo**
   ```bash
   git clone https://github.com/dennismysh/postgraph.git
   cd postgraph
   ```

2. **Configure environment**
   ```bash
   cp .env.example .env
   # Edit .env with your credentials

   cp web/.env.example web/.env
   # Edit web/.env with your credentials
   ```

3. **Start the backend**
   ```bash
   cargo run --package postgraph-server
   ```
   Migrations run automatically on startup.

4. **Start the frontend** (separate terminal)
   ```bash
   cd web && npm install && npm run dev
   ```

5. Open `http://localhost:5173` and log in with your configured dashboard password.

### Environment variables

**Backend (`.env`):**

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | Postgres connection string | — |
| `THREADS_ACCESS_TOKEN` | Threads API long-lived token | — |
| `MERCURY_API_KEY` | Inception Labs API key | — |
| `MERCURY_API_URL` | Mercury endpoint | `https://api.inceptionlabs.ai/v1` |
| `POSTGRAPH_API_KEY` | API key for frontend-to-backend auth | — |
| `FRONTEND_ORIGIN` | Allowed CORS origin | `http://localhost:5173` |
| `PORT` | Server port | `8000` |

**Frontend (`web/.env`):**

| Variable | Description |
|----------|-------------|
| `API_URL` | Rust backend URL |
| `API_KEY` | Same as `POSTGRAPH_API_KEY` |
| `DASHBOARD_PASSWORD` | Login password |
| `SESSION_SECRET` | 64-char random key for session signing |

## Development

```bash
# Backend
cargo check --workspace              # Quick compile check
cargo fmt --all                       # Format
cargo clippy --workspace --all-targets # Lint
cargo test --workspace                # Tests

# Frontend
cd web && npm run dev                 # Dev server
cd web && npm run build               # Production build
cd web && npx svelte-check            # Type check
```

## Database

Schema is managed via SQL migrations in `postgraph-server/migrations/`. They run automatically on startup via `sqlx::migrate!()`.

**Core tables:** `posts`, `topics`, `categories`, `post_topics`, `post_edges`, `engagement_snapshots`, `sync_state`, `api_tokens`

## API endpoints

All endpoints require `Authorization: Bearer <API_KEY>`.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/posts` | List posts (with filters) |
| GET | `/api/posts/:id` | Post detail |
| GET | `/api/graph` | Post graph (nodes + edges for Sigma.js) |
| GET | `/api/graph/tags` | Topic graph (topic nodes, co-occurrence edges) |
| GET | `/api/analytics` | Aggregate analytics |
| GET | `/api/analytics/views` | Time-series engagement data |
| GET | `/api/categories` | List topic categories |
| POST | `/api/sync` | Trigger manual sync |
| GET | `/api/sync/status` | Sync progress |
| POST | `/api/analyze` | Trigger LLM analysis |
| GET | `/api/analyze/status` | Analysis progress |
| POST | `/api/categorize` | Group topics into categories |
| POST | `/api/reanalyze` | Reset analysis for re-processing |
| GET | `/health` | Health check |

## License

[MIT](LICENSE)
