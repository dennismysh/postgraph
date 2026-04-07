# Compose Tab — Content Calendar & Scheduled Publishing

**Date:** 2026-04-07
**Status:** Approved

## Overview

A new Compose tab in the postgraph dashboard for creating, scheduling, and publishing Threads posts. Features a content calendar with weekly, 2-week, and monthly views, a compose modal for writing posts, and a backend scheduler that publishes posts at their scheduled time via the Threads API.

Text-only in V1. No media attachments, no reply management.

## Data Model

### `scheduled_posts` table

| Column | Type | Notes |
|--------|------|-------|
| `id` | UUID (PK) | Generated server-side |
| `text` | TEXT NOT NULL | Post content (500 char Threads limit) |
| `status` | TEXT NOT NULL | `draft`, `scheduled`, `publishing`, `published`, `failed`, `cancelled` |
| `scheduled_at` | TIMESTAMPTZ | NULL for drafts, required for scheduled |
| `published_at` | TIMESTAMPTZ | Set when successfully published |
| `threads_post_id` | TEXT | Threads API post ID after publish, links to `posts.id` |
| `error_message` | TEXT | Last failure reason if status = `failed` |
| `created_at` | TIMESTAMPTZ | Default NOW() |
| `updated_at` | TIMESTAMPTZ | Default NOW(), updated on every write |

Separate from the synced `posts` table. The `posts` table remains the source of truth for analytics — it stores data pulled from Threads. `scheduled_posts` is the authoring pipeline. After a post publishes and the next sync picks it up, `threads_post_id` links the two.

### Status lifecycle

```
draft --> scheduled --> publishing --> published
  |          |              |
  v          v              v
cancelled  cancelled      failed --> scheduled (retry)
```

- `publishing` is a brief transitional state set right before the API call to prevent double-publish.
- `failed` posts can be retried (resets to `scheduled` with `scheduled_at = NOW()`).

## Backend

### Publish module (`publish.rs`)

New methods on `ThreadsClient` for the Threads two-step publish flow:

1. **Create container:** `POST /{user_id}/threads` with `text` param + `media_type=TEXT` — returns a container ID
2. **Publish container:** `POST /{user_id}/threads_publish` with `creation_id` = container ID — returns the published post ID

### API routes (`routes/compose.rs`)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/compose` | GET | List scheduled posts (filterable by status, date range) |
| `/api/compose` | POST | Create a new draft or scheduled post |
| `/api/compose/{id}` | GET | Get a single scheduled post |
| `/api/compose/{id}` | PUT | Update text, status, or scheduled_at |
| `/api/compose/{id}` | DELETE | Cancel/delete a scheduled post |
| `/api/compose/{id}/publish` | POST | Publish immediately ("Post Now") |

### Scheduler loop (in `main.rs`)

A new `tokio::spawn` block alongside the existing sync and nightly loops:

- Runs every 60 seconds via `tokio::time::interval`
- Queries: `SELECT * FROM scheduled_posts WHERE status = 'scheduled' AND scheduled_at <= NOW()`
- For each post:
  1. Re-check status (catch cancellations in the race window)
  2. Set status to `publishing`
  3. Call two-step Threads API
  4. On success: set status to `published`, store `threads_post_id` and `published_at`
  5. On failure: set status to `failed`, store `error_message`
- On server startup: recover stuck `publishing` rows (where `updated_at` is older than 5 minutes) by resetting to `scheduled`

## Frontend

### New route: `/compose`

Added to the nav alongside Dashboard, Analytics V2, Insights, etc.

### Page layout

The page is the calendar itself. Top bar contains:
- View mode toggle: **Weekly** | **2-Week** | **Monthly**
- Navigation arrows (prev/next) and a "Today" button
- **New Post** button (top-right)

### Calendar views

- **Weekly** — 7 columns (Mon-Sun), rows are time slots (morning/afternoon/evening). Most detail per post.
- **2-Week** — 14 columns, compact cards, less text visible per post.
- **Monthly** — Traditional grid (rows = weeks, cols = days). Posts shown as small pills/chips on their day.

### Post cards on the calendar

Each scheduled post appears as a card showing:
- Truncated text preview (first ~50 chars)
- Status badge (color-coded: draft = gray, scheduled = blue, published = green, failed = red)
- Scheduled time

### Interactions

- **Click a post card** — opens compose modal pre-filled with that post's data for editing
- **Click an empty time slot** — opens compose modal with date/time pre-filled
- **New Post button** — opens compose modal with empty form, manual date/time selection

### Compose modal

A drawer/modal overlay on the calendar containing:

- **Text area** with live character count (500 limit)
- **Date/time picker** (pre-filled from calendar click, or manual)
- **Action buttons:**
  - **Post Now** — publishes immediately via `/api/compose/{id}/publish`
  - **Schedule** — saves with status `scheduled` and the chosen date/time
  - **Save as Draft** — saves with status `draft`, no scheduled_at
- **For existing posts:**
  - **Cancel Post** (if scheduled) — sets status to `cancelled`
  - **Delete Draft** (if draft) — deletes the row
  - **Retry** (if failed) — resets to `scheduled` with `scheduled_at = NOW()`

## Error Handling & Edge Cases

### Publish failures
If the Threads API returns an error during either step (container creation or publish), set status to `failed` with error message stored. Failed posts show a red badge on the calendar. User can retry from the modal.

### Character limit
Frontend enforces 500 char limit with a live counter. Backend also validates before saving — returns 400 if exceeded.

### Race condition on cancel
The publish loop re-checks status right before the API call. If the user cancelled between the SELECT and the publish attempt, the re-check catches it and skips.

### Stuck `publishing` recovery
On server startup, any `publishing` rows with `updated_at` older than 5 minutes get reset to `scheduled`.

### Scheduling in the past
If a user sets a time that's already passed, the poll loop picks it up immediately since `scheduled_at <= NOW()` — effectively treated as "post now."

### Rate limiting
If the Threads API returns 429, mark the post as `failed` with a rate-limit error message. User can retry manually later. No automatic retry loop.

## Out of Scope (V1)

- Media attachments (images, video)
- Reply management
- AI-assisted draft generation
- Drag-and-drop rescheduling on the calendar
- Published post display on the calendar (calendar shows only scheduled/draft/failed posts)
