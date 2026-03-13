# Security Hardening Design Spec

**Goal:** Harden the postgraph dashboard for single-user deployment by removing client-side API key exposure, adding a password-protected login gate, and locking down CORS.

**Scope:** Single-user hardening only. Multi-tenant changes (user tables, per-user data isolation, OAuth) are deferred.

**Existing Architecture:** Rust backend (Shuttle + axum) serves REST API. SvelteKit frontend (Netlify) calls API directly from browser with `PUBLIC_API_KEY` baked into JS bundle.

---

## 1. Server-Side API Proxy

### Problem

`PUBLIC_API_KEY` is a public SvelteKit env var — it's bundled into the client JS and visible to anyone who loads the page or inspects the network tab. Anyone with the key can call the Rust backend directly.

### Solution

Move all API calls to SvelteKit server routes. The API key becomes a private server-side env var that never reaches the browser.

### Changes

**New files:**
- `web/src/routes/api/graph/+server.ts` — GET handler, proxies to Rust backend `/api/graph`
- `web/src/routes/api/posts/+server.ts` — GET handler, proxies to `/api/posts`
- `web/src/routes/api/analytics/+server.ts` — GET handler, proxies to `/api/analytics`
- `web/src/routes/api/sync/+server.ts` — POST handler, proxies to `/api/sync`

Each server route:
1. Reads `API_URL` and `API_KEY` from `$env/static/private`
2. Forwards the request to the Rust backend with `Authorization: Bearer ${API_KEY}`
3. Returns the response to the client

**Modified files:**
- `web/src/lib/api.ts` — Remove `Authorization` header. Change base URL to empty string (calls go to same-origin SvelteKit server routes). Remove `PUBLIC_API_KEY` / `PUBLIC_API_URL` imports.

**Removed env vars:**
- `PUBLIC_API_URL` — no longer needed (same-origin requests)
- `PUBLIC_API_KEY` — replaced by private `API_KEY`

**New private env vars (web/.env):**
- `API_URL` — Rust backend URL (e.g., `http://localhost:8000`)
- `API_KEY` — shared secret for backend auth

### Data Flow

```
Browser (no secrets) → SvelteKit server route (has API_KEY) → Rust backend
```

---

## 2. Login Gate + Session Management

### Problem

The dashboard is publicly accessible — anyone with the URL can view Threads analytics data.

### Solution

Add a password-protected login page. A single `DASHBOARD_PASSWORD` env var controls access. Authenticated sessions use a signed HttpOnly cookie.

### Changes

**New files:**
- `web/src/routes/login/+page.svelte` — password form (dark theme, matches app style)
- `web/src/routes/login/+page.server.ts` — form action that validates password, sets session cookie
- `web/src/routes/logout/+server.ts` — clears session cookie, redirects to `/login`
- `web/src/hooks.server.ts` — auth guard that runs on every request

**Modified files:**
- `web/src/routes/+layout.svelte` — add a "Logout" link in the nav bar (visible when authenticated)

**New env vars (web/.env):**
- `DASHBOARD_PASSWORD` — the login password
- `SESSION_SECRET` — random string used to sign session cookies (minimum 32 chars)

### Session Cookie Details

- **Name:** `session`
- **Value:** Signed token containing `authenticated=true` and expiry timestamp. Signed using `SESSION_SECRET` with HMAC-SHA256 (via SvelteKit's built-in cookie signing).
- **Attributes:** `HttpOnly`, `Secure` (in production), `SameSite=Strict`, `Path=/`
- **Expiry:** 7 days
- **No session store needed** — single-user, so the signed cookie itself is the session

### Auth Guard (`hooks.server.ts`)

Runs on every request:
1. If path is `/login` or starts with `/login/`, allow through
2. Otherwise, check for valid `session` cookie
3. If missing or invalid signature or expired, redirect to `/login`
4. If valid, proceed to route handler

### Login Flow

1. User visits any page → redirected to `/login`
2. User enters password → POST to `/login`
3. Server compares password to `DASHBOARD_PASSWORD` using constant-time comparison
4. On success: set session cookie, redirect to `/`
5. On failure: re-render login page with error message

### Logout

GET `/logout` → clear session cookie → redirect to `/login`

---

## 3. CORS Lockdown

### Problem

The Rust backend currently uses `CorsLayer::new().allow_origin(Any)` — any website can make requests to the API.

### Solution

Restrict CORS to the frontend's origin only.

### Changes

**Modified files:**
- `postgraph-server/src/main.rs` — Replace `allow_origin(Any)` with `allow_origin(frontend_origin.parse().unwrap())` where `frontend_origin` comes from `FRONTEND_ORIGIN` env var

**New env var (root .env):**
- `FRONTEND_ORIGIN` — e.g., `http://localhost:5173` for dev, `https://your-app.netlify.app` for production

**Fallback:** If `FRONTEND_ORIGIN` is not set, default to `http://localhost:5173` for local dev convenience.

---

## 4. Shuttle Secrets

### Problem

Production secrets are managed via env vars, but Shuttle has a dedicated secrets system (`Secrets.toml`) that's more appropriate for deployment.

### Solution

Add `Secrets.toml` to `.gitignore` and document its use. No code changes needed — Shuttle injects `Secrets.toml` values as environment variables automatically.

**New files:**
- `Secrets.toml.example` — template showing required keys

```toml
THREADS_ACCESS_TOKEN = ""
MERCURY_API_KEY = ""
MERCURY_API_URL = "https://api.inceptionlabs.ai/v1"
POSTGRAPH_API_KEY = ""
FRONTEND_ORIGIN = "https://your-app.netlify.app"
```

**Modified files:**
- `.gitignore` — add `Secrets.toml`

---

## 5. Updated Environment Files

### `web/.env.example`

```
API_URL=http://localhost:8000
API_KEY=your_api_key_here
DASHBOARD_PASSWORD=your_password_here
SESSION_SECRET=generate_a_random_64_char_string_here
```

### Root `.env.example`

Add `FRONTEND_ORIGIN`:

```
THREADS_ACCESS_TOKEN=your_threads_token_here
MERCURY_API_KEY=your_mercury_api_key_here
MERCURY_API_URL=https://api.inceptionlabs.ai/v1
POSTGRAPH_API_KEY=your_dashboard_api_key_here
FRONTEND_ORIGIN=http://localhost:5173
```

---

## Summary of Security Improvements

| Before | After |
|--------|-------|
| API key in browser JS bundle | API key server-side only |
| No login required | Password-protected login gate |
| CORS allows any origin | CORS restricted to frontend origin |
| Env vars for prod secrets | Shuttle Secrets for production |
| Session: none | HttpOnly signed cookie, 7-day expiry |

## Deferred

- Multi-tenant (user accounts, per-user data, OAuth with Threads)
- Rate limiting on login attempts
- CSRF token on login form (SvelteKit's form actions have built-in CSRF protection via origin checking)
- Password hashing (single env var comparison is sufficient for single-user; hashing becomes relevant with a user table)
