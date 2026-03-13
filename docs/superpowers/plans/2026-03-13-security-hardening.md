# Security Hardening Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the postgraph dashboard by moving API keys server-side, adding a password login gate, and locking down CORS.

**Architecture:** SvelteKit server routes proxy all API calls to the Rust backend, keeping the API key server-side. A password-protected login page with signed HttpOnly cookies guards all routes via a `hooks.server.ts` auth guard. The Rust backend restricts CORS to the frontend origin.

**Tech Stack:** SvelteKit (server routes, hooks, form actions), axum (CORS config), Shuttle Secrets

**Spec:** `docs/superpowers/specs/2026-03-13-security-hardening-design.md`

**Important:** This project uses Svelte 5 with `runes: true` in `svelte.config.js`. All components must use runes syntax: `$state()`, `$derived()`, `$effect()`, `$props()`, `onclick` (not `on:click`), `{@render children()}` (not `<slot />`).

---

## Chunk 1: Server-Side API Proxy + Client Update

### Task 1: Create SvelteKit Server Route Proxy for Graph

**Files:**
- Create: `web/src/routes/api/graph/+server.ts`

- [ ] **Step 1: Create the server route**

```typescript
import { API_URL, API_KEY } from '$env/static/private';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  const res = await fetch(`${API_URL}/api/graph`, {
    headers: { 'Authorization': `Bearer ${API_KEY}` },
  });
  const data = await res.json();
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status: res.status,
  });
};
```

- [ ] **Step 2: Commit**

```bash
git add web/src/routes/api/graph/+server.ts
git commit -m "feat: add server-side proxy for /api/graph"
```

---

### Task 2: Create Server Route Proxies for Posts, Analytics, Sync

**Files:**
- Create: `web/src/routes/api/posts/+server.ts`
- Create: `web/src/routes/api/analytics/+server.ts`
- Create: `web/src/routes/api/sync/+server.ts`

- [ ] **Step 1: Create posts proxy**

```typescript
import { API_URL, API_KEY } from '$env/static/private';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  const res = await fetch(`${API_URL}/api/posts`, {
    headers: { 'Authorization': `Bearer ${API_KEY}` },
  });
  const data = await res.json();
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status: res.status,
  });
};
```

- [ ] **Step 2: Create analytics proxy**

```typescript
import { API_URL, API_KEY } from '$env/static/private';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async () => {
  const res = await fetch(`${API_URL}/api/analytics`, {
    headers: { 'Authorization': `Bearer ${API_KEY}` },
  });
  const data = await res.json();
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status: res.status,
  });
};
```

- [ ] **Step 3: Create sync proxy (POST method)**

```typescript
import { API_URL, API_KEY } from '$env/static/private';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async () => {
  const res = await fetch(`${API_URL}/api/sync`, {
    method: 'POST',
    headers: { 'Authorization': `Bearer ${API_KEY}` },
  });
  const data = await res.json();
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status: res.status,
  });
};
```

- [ ] **Step 4: Commit**

```bash
git add web/src/routes/api/posts/+server.ts web/src/routes/api/analytics/+server.ts web/src/routes/api/sync/+server.ts
git commit -m "feat: add server-side proxies for posts, analytics, sync"
```

---

### Task 3: Update Frontend API Client

**Files:**
- Modify: `web/src/lib/api.ts`

- [ ] **Step 1: Rewrite api.ts to use same-origin requests with no auth headers**

Replace the entire file with:

```typescript
async function fetchApi<T>(path: string): Promise<T> {
  const res = await fetch(path, {
    headers: { 'Content-Type': 'application/json' },
  });
  if (!res.ok) throw new Error(`API error: ${res.status}`);
  return res.json();
}

export interface GraphNode {
  id: string;
  label: string;
  size: number;
  sentiment: number | null;
  topics: string[];
}

export interface GraphEdge {
  source: string;
  target: string;
  weight: number;
  edge_type: string;
}

export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface AnalyticsData {
  total_posts: number;
  analyzed_posts: number;
  total_topics: number;
  topics: TopicSummary[];
  engagement_over_time: EngagementPoint[];
}

export interface TopicSummary {
  name: string;
  post_count: number;
  avg_engagement: number;
}

export interface EngagementPoint {
  date: string;
  likes: number;
  replies: number;
  reposts: number;
}

export interface Post {
  id: string;
  text: string | null;
  timestamp: string;
  likes: number;
  replies_count: number;
  reposts: number;
  quotes: number;
  sentiment: number | null;
}

export interface SyncResult {
  posts_synced: number;
  posts_analyzed: number;
  edges_computed: number;
}

export const api = {
  getGraph: () => fetchApi<GraphData>('/api/graph'),
  getPosts: () => fetchApi<Post[]>('/api/posts'),
  getAnalytics: () => fetchApi<AnalyticsData>('/api/analytics'),
  triggerSync: () => fetch('/api/sync', {
    method: 'POST',
  }).then(r => r.json() as Promise<SyncResult>),
};
```

Key changes: removed `$env/dynamic/public` import, removed `API_URL` and `API_KEY` constants, removed `Authorization` header from all calls, `fetchApi` uses relative paths (same-origin), `triggerSync` also uses relative path with no auth header.

- [ ] **Step 2: Update web/.env.example**

Replace contents with:

```
API_URL=http://localhost:8000
API_KEY=your_api_key_here
DASHBOARD_PASSWORD=your_password_here
SESSION_SECRET=generate_a_random_64_char_string_here
```

- [ ] **Step 3: Verify it builds**

Run: `cd web && npm run build`
Expected: Builds successfully (the proxy routes won't work without a backend, but the build should complete)

- [ ] **Step 4: Commit**

```bash
git add web/src/lib/api.ts web/.env.example
git commit -m "feat: update API client to use same-origin server proxies"
```

---

## Chunk 2: Login Gate + Session Management

### Task 4: Create Auth Guard (hooks.server.ts)

**Files:**
- Create: `web/src/hooks.server.ts`

- [ ] **Step 1: Create hooks.server.ts**

```typescript
import { redirect, type Handle } from '@sveltejs/kit';
import { SESSION_SECRET } from '$env/static/private';
import * as crypto from 'node:crypto';

function verifySession(cookieValue: string): boolean {
  try {
    const [payload, signature] = cookieValue.split('.');
    if (!payload || !signature) return false;

    const expectedSig = crypto
      .createHmac('sha256', SESSION_SECRET)
      .update(payload)
      .digest('base64url');

    if (!crypto.timingSafeEqual(Buffer.from(signature), Buffer.from(expectedSig))) {
      return false;
    }

    const data = JSON.parse(Buffer.from(payload, 'base64url').toString());
    if (!data.authenticated) return false;
    if (data.expires && Date.now() > data.expires) return false;

    return true;
  } catch {
    return false;
  }
}

export const handle: Handle = async ({ event, resolve }) => {
  const path = event.url.pathname;

  // Allow login page through without auth
  if (path === '/login' || path.startsWith('/login/')) {
    return resolve(event);
  }

  const session = event.cookies.get('session');
  if (!session || !verifySession(session)) {
    throw redirect(303, '/login');
  }

  return resolve(event);
};
```

- [ ] **Step 2: Verify it builds**

Run: `cd web && npm run build`
Expected: Builds successfully

- [ ] **Step 3: Commit**

```bash
git add web/src/hooks.server.ts
git commit -m "feat: add auth guard hook for session validation"
```

---

### Task 5: Create Login Page

**Files:**
- Create: `web/src/routes/login/+page.svelte`
- Create: `web/src/routes/login/+page.server.ts`

- [ ] **Step 1: Create login form action**

```typescript
import { fail, redirect } from '@sveltejs/kit';
import { DASHBOARD_PASSWORD, SESSION_SECRET } from '$env/static/private';
import * as crypto from 'node:crypto';
import type { Actions } from './$types';

function createSession(): string {
  const payload = Buffer.from(JSON.stringify({
    authenticated: true,
    expires: Date.now() + 7 * 24 * 60 * 60 * 1000, // 7 days
  })).toString('base64url');

  const signature = crypto
    .createHmac('sha256', SESSION_SECRET)
    .update(payload)
    .digest('base64url');

  return `${payload}.${signature}`;
}

export const actions: Actions = {
  default: async ({ request, cookies }) => {
    const form = await request.formData();
    const password = form.get('password') as string;

    if (!password) {
      return fail(400, { error: 'Password is required' });
    }

    // Constant-time comparison
    const expected = Buffer.from(DASHBOARD_PASSWORD);
    const received = Buffer.from(password);

    if (expected.length !== received.length ||
        !crypto.timingSafeEqual(expected, received)) {
      return fail(401, { error: 'Invalid password' });
    }

    cookies.set('session', createSession(), {
      path: '/',
      httpOnly: true,
      secure: process.env.NODE_ENV === 'production',
      sameSite: 'strict',
      maxAge: 7 * 24 * 60 * 60, // 7 days in seconds
    });

    throw redirect(303, '/');
  },
};
```

- [ ] **Step 2: Create login page component**

```svelte
<script lang="ts">
  import { enhance } from '$app/forms';

  let { form } = $props();
</script>

<div class="login-container">
  <div class="login-card">
    <h1>postgraph</h1>
    <form method="POST" use:enhance>
      {#if form?.error}
        <p class="error">{form.error}</p>
      {/if}
      <input
        type="password"
        name="password"
        placeholder="Password"
        autocomplete="current-password"
        required
      />
      <button type="submit">Sign in</button>
    </form>
  </div>
</div>

<style>
  .login-container {
    display: flex;
    justify-content: center;
    align-items: center;
    height: 100vh;
    background: #0a0a0a;
  }
  .login-card {
    background: #111;
    border: 1px solid #333;
    border-radius: 8px;
    padding: 2rem;
    width: 320px;
    text-align: center;
  }
  h1 {
    margin: 0 0 1.5rem;
    color: #eee;
    font-size: 1.5rem;
  }
  input {
    width: 100%;
    padding: 0.6rem;
    background: #1a1a1a;
    border: 1px solid #444;
    color: #eee;
    border-radius: 4px;
    font-size: 1rem;
    margin-bottom: 1rem;
    box-sizing: border-box;
  }
  button {
    width: 100%;
    padding: 0.6rem;
    background: #4363d8;
    border: none;
    color: white;
    border-radius: 4px;
    font-size: 1rem;
    cursor: pointer;
  }
  button:hover { background: #3751b5; }
  .error {
    color: #e6194b;
    font-size: 0.85rem;
    margin: 0 0 1rem;
  }
</style>
```

- [ ] **Step 3: Verify it builds**

Run: `cd web && npm run build`
Expected: Builds successfully

- [ ] **Step 4: Commit**

```bash
git add web/src/routes/login/
git commit -m "feat: add password login page with session cookie auth"
```

---

### Task 6: Create Logout Endpoint + Update Layout

**Files:**
- Create: `web/src/routes/logout/+server.ts`
- Modify: `web/src/routes/+layout.svelte`

- [ ] **Step 1: Create logout server route**

```typescript
import { redirect } from '@sveltejs/kit';
import type { RequestHandler } from './$types';

export const GET: RequestHandler = async ({ cookies }) => {
  cookies.delete('session', { path: '/' });
  throw redirect(303, '/login');
};
```

- [ ] **Step 2: Update +layout.svelte to add logout link**

Replace the entire file with:

```svelte
<script lang="ts">
  import { page } from '$app/stores';

  let { children } = $props();
</script>

<div class="layout">
  <nav>
    <div class="nav-links">
      <a href="/" class:active={$page.url.pathname === '/'}>Graph</a>
      <a href="/analytics" class:active={$page.url.pathname === '/analytics'}>Analytics</a>
    </div>
    <a href="/logout" class="logout">Logout</a>
  </nav>
  <div class="content">
    {@render children()}
  </div>
</div>

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    background: #0a0a0a;
    color: #eee;
  }
  .layout { display: flex; flex-direction: column; height: 100vh; }
  nav {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid #333;
  }
  .nav-links {
    display: flex;
    gap: 1rem;
  }
  nav a {
    color: #888;
    text-decoration: none;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
  }
  nav a.active { color: #fff; background: #333; }
  .logout { color: #888; font-size: 0.85rem; }
  .logout:hover { color: #e6194b; }
  .content { flex: 1; overflow: hidden; }
</style>
```

- [ ] **Step 3: Verify it builds**

Run: `cd web && npm run build`
Expected: Builds successfully

- [ ] **Step 4: Commit**

```bash
git add web/src/routes/logout/+server.ts web/src/routes/+layout.svelte
git commit -m "feat: add logout endpoint and logout link in nav"
```

---

## Chunk 3: CORS Lockdown + Environment Files

### Task 7: Lock Down CORS on Rust Backend

**Files:**
- Modify: `postgraph-server/src/main.rs`

- [ ] **Step 1: Update CORS configuration**

In `postgraph-server/src/main.rs`, replace the CORS setup. Change the import:

Replace the CORS construction (lines 70-73):

Replace:
```rust
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
```

With:
```rust
    let frontend_origin = std::env::var("FRONTEND_ORIGIN")
        .unwrap_or_else(|_| "http://localhost:5173".to_string());
    let cors = CorsLayer::new()
        .allow_origin(
            frontend_origin
                .parse::<axum::http::HeaderValue>()
                .expect("FRONTEND_ORIGIN must be a valid origin"),
        )
        .allow_methods(Any)
        .allow_headers(Any);
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add postgraph-server/src/main.rs
git commit -m "feat: restrict CORS to configured frontend origin"
```

---

### Task 8: Update Environment Files + Shuttle Secrets

**Files:**
- Modify: `.env.example`
- Modify: `.gitignore`
- Create: `Secrets.toml.example`

- [ ] **Step 1: Update root .env.example**

Replace contents with:

```
THREADS_ACCESS_TOKEN=your_threads_token_here
MERCURY_API_KEY=your_mercury_api_key_here
MERCURY_API_URL=https://api.inceptionlabs.ai/v1
POSTGRAPH_API_KEY=your_dashboard_api_key_here
FRONTEND_ORIGIN=http://localhost:5173
```

- [ ] **Step 2: Create Secrets.toml.example**

```toml
THREADS_ACCESS_TOKEN = ""
MERCURY_API_KEY = ""
MERCURY_API_URL = "https://api.inceptionlabs.ai/v1"
POSTGRAPH_API_KEY = ""
FRONTEND_ORIGIN = "https://your-app.netlify.app"
```

- [ ] **Step 3: Add Secrets.toml to .gitignore**

Add `Secrets.toml` to the end of `.gitignore`.

- [ ] **Step 4: Update CLAUDE.md**

Add the following to the "Environment Variables" section of `CLAUDE.md`:

```markdown
### Shuttle Secrets (production)

Use `Secrets.toml` (git-ignored) instead of `.env` for Shuttle deployments. See `Secrets.toml.example`.

### Frontend Auth

The frontend uses server-side session auth. See `web/.env.example` for:
- `API_URL` / `API_KEY` — server-to-server auth with Rust backend (never exposed to browser)
- `DASHBOARD_PASSWORD` — login password
- `SESSION_SECRET` — cookie signing key
```

- [ ] **Step 5: Commit**

```bash
git add .env.example Secrets.toml.example .gitignore CLAUDE.md
git commit -m "feat: add Shuttle secrets template, update env files and docs"
```

---

### Task 9: Final Verification

- [ ] **Step 1: Full backend compile check**

Run: `cargo check --workspace`
Expected: Compiles with no errors

- [ ] **Step 2: Full frontend build**

Run: `cd web && npm run build`
Expected: Builds successfully

- [ ] **Step 3: Format and lint**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets`
Expected: No errors (dead_code warnings are OK)

- [ ] **Step 4: Final commit if any formatting changes**

```bash
git add -A && git commit -m "chore: format and lint cleanup"
```
