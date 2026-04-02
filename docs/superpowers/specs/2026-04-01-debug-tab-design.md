# Debug Tab Design

**Date:** 2026-04-01
**Status:** Approved

## Problem

No way to inspect the data pipeline from the dashboard. When views appear attributed to the wrong day or metrics look off, the only option is to query the database directly. Need a tab that surfaces pipeline metadata alongside post data for live debugging.

## Design

### New Backend Endpoint

**`GET /api/posts/debug?since=YYYY-MM-DDTHH:MM:SSZ`**

Returns posts with pipeline metadata. Single query joining `posts` with latest `engagement_snapshots`, plus intent/subject names:

```sql
SELECT p.id, LEFT(p.text, 120) AS text_preview,
       p.timestamp, p.views, p.likes, p.replies_count, p.reposts, p.quotes,
       p.synced_at,
       p.sentiment,
       i.name AS intent,
       s.name AS subject,
       es.captured_at AS last_captured_at
FROM posts p
LEFT JOIN intents i ON p.intent_id = i.id
LEFT JOIN subjects s ON p.subject_id = s.id
LEFT JOIN LATERAL (
    SELECT captured_at FROM engagement_snapshots
    WHERE post_id = p.id ORDER BY captured_at DESC LIMIT 1
) es ON true
WHERE p.timestamp >= $1
ORDER BY p.timestamp DESC
```

Response struct: `DebugPost` with all fields above.

### Frontend — Debug Page

**Route:** `/debug` — added to nav bar alongside Graph, Analytics, Fourier.

**Layout:**
1. **Time range selector** — buttons: 24h (default), 7d, 30d, All
2. **Summary bar** — post count in range, latest sync time, latest snapshot capture time
3. **Post table** — one row per post:

| Column | Value | Purpose |
|--------|-------|---------|
| Text | First ~80 chars | Identify the post |
| Posted | Timestamp in ET (America/New_York) | When published |
| API Bucket | Computed daily_views date | Shows attribution |
| Views | Current value | Performance |
| Likes / Replies / Reposts | Current values | Performance |
| Intent / Subject | From LLM analysis | Pipeline status |
| Last Synced | `synced_at` in ET | When last pulled from API |
| Last Captured | `captured_at` in ET | When engagement last recorded |

**API Bucket computation** (client-side):
```typescript
function getApiBucket(utcTimestamp: string): string {
  const dt = new Date(utcTimestamp);
  // API day boundary is 08:00 UTC — if before that, views go to previous day
  if (dt.getUTCHours() < 8) {
    dt.setUTCDate(dt.getUTCDate() - 1);
  }
  return dt.toISOString().slice(0, 10);
}
```

When the API bucket date differs from the post's local date, show a subtle visual indicator (color/icon) to make the offset immediately visible.

All timestamps displayed in America/New_York timezone.

### File Changes

**Backend (Rust):**
- `routes/posts.rs` — Add `DebugPost` struct + `get_debug_posts` handler
- `main.rs` — Register `GET /api/posts/debug`

**Frontend (Svelte):**
- `web/src/routes/debug/+page.svelte` — New page (thin wrapper)
- `web/src/lib/components/Debug.svelte` — Table component with time range selector, summary bar, post rows, bucket computation
- `web/src/lib/api.ts` — Add `DebugPost` interface + `getDebugPosts(since)` function
- `web/src/routes/api/posts/debug/+server.ts` — SvelteKit proxy route
- `web/src/routes/+layout.svelte` — Add "Debug" to nav bar

**No changes:** No migrations, no new tables, no changes to sync or analytics.
