# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**postgraph** is a Threads analytics platform. Rust backend syncs posts from the Threads API, analyzes them with Mercury LLM (Inception Labs), computes a topic-based relationship graph, and serves data to a Svelte dashboard with Sigma.js graph visualization.

## Architecture

- **postgraph-server/** — Rust API server (axum + sqlx + tokio), deployed to Railway
- **web/** — Svelte frontend (SvelteKit, deployed to Netlify)
- **Postgres** — provisioned by Railway

## Build Commands

### Rust backend
```bash
cargo check --workspace              # Quick compile check
cargo build --workspace              # Full build
cargo fmt --all                       # Format
cargo clippy --workspace --all-targets # Lint
cargo test --workspace                # Tests
```

### Svelte frontend
```bash
cd web && npm install                 # Install deps
cd web && npm run dev                 # Dev server
cd web && npm run build               # Production build
cd web && npx svelte-check            # Type check
```

### Running locally
```bash
# Backend (requires DATABASE_URL set in .env)
cargo run --package postgraph-server

# Frontend (separate terminal)
cd web && npm run dev
```

## Environment Variables

See `.env.example`. Required:
- `DATABASE_URL` — Postgres connection string
- `THREADS_ACCESS_TOKEN` — Threads API long-lived token
- `MERCURY_API_KEY` — Inception Labs API key
- `MERCURY_API_URL` — Mercury endpoint (default: https://api.inceptionlabs.ai/v1)
- `POSTGRAPH_API_KEY` — API key for frontend-to-backend auth
- `FRONTEND_ORIGIN` — Allowed CORS origin (default: http://localhost:5173)
- `PORT` — Server port (default: 8000, Railway sets this automatically)

### Frontend Auth

The frontend uses server-side session auth. See `web/.env.example` for:
- `API_URL` / `API_KEY` — server-to-server auth with Rust backend (never exposed to browser)
- `DASHBOARD_PASSWORD` — login password
- `SESSION_SECRET` — cookie signing key

## Key Patterns

- Threads API client in `threads.rs` — follows ndl's reqwest patterns, access_token as query param
- Mercury client in `mercury.rs` — OpenAI-compatible API (POST /chat/completions)
- Background sync runs every 15 min via tokio::time::interval in main.rs
- Edges are pre-computed server-side, not in the frontend
- Routes split by domain: `routes/graph.rs`, `routes/posts.rs`, `routes/analytics.rs`, `routes/sync.rs`

## Pre-commit Checklist

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo check --workspace
cd web && npx svelte-check
```

## Code Conventions

- Edition 2024 Rust
- thiserror for custom error types
- Async throughout (tokio runtime)
- No unwrap in production paths
- Modules are single files (no mod/ directories, except routes/)

## Database

Migrations in `postgraph-server/migrations/`. Run automatically on startup via `sqlx::migrate!()`.

## Reference

- Design spec: `docs/superpowers/specs/2026-03-11-postgraph-design.md`
- ndl reference project: github.com/pgray/ndl (Threads API patterns)
- Threads API base: `https://graph.threads.net/v1.0/`
- Mercury API: OpenAI-compatible at configured endpoint
