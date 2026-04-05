# Trends Tab Part 1: Reach Scraper Service Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone scraping service based on Reach's Docker image that exposes `/scrape`, `/browse`, and `/health` HTTP endpoints for Postgraph to call.

**Architecture:** A Python Flask API running inside Reach's sandbox Docker image (Ubuntu 24.04 + Chrome + Playwright). The API receives a URL + CSS selectors, uses Playwright to navigate and extract content, and returns structured JSON. Deployed as a separate Railway service.

**Tech Stack:** Python 3, Flask, Playwright (already in Reach image), Docker

**Spec:** `docs/superpowers/specs/2026-04-05-trends-tab-design.md` (Scraping Service section)

---

### Task 1: Create Scraper Directory and Dockerfile

**Files:**
- Create: `reach-scraper/Dockerfile`
- Create: `reach-scraper/requirements.txt`

- [ ] **Step 1: Create the requirements file**

Create `reach-scraper/requirements.txt`:

```
flask==3.1.1
gunicorn==23.0.0
```

- [ ] **Step 2: Create the Dockerfile**

Create `reach-scraper/Dockerfile`:

```dockerfile
FROM ghcr.io/todie/reach:latest

USER root

# Install Flask and gunicorn
COPY requirements.txt /app/requirements.txt
RUN pip install --break-system-packages -r /app/requirements.txt

# Copy the API code
COPY api.py /app/api.py

# The reach image runs as 'sandbox' user
USER sandbox

ENV DISPLAY=:99

# Start Xvfb + the Flask API
# Xvfb must run for Playwright to work (it needs a display)
CMD Xvfb :99 -screen 0 1280x720x24 -nolisten tcp & \
    sleep 1 && \
    gunicorn --bind 0.0.0.0:${PORT:-8080} --timeout 120 --workers 1 app:app --chdir /app
```

Note: We only start Xvfb (for the virtual display) — we skip openbox, x11vnc, noVNC since we don't need the desktop UI. Single gunicorn worker because Playwright isn't thread-safe with a shared browser.

- [ ] **Step 3: Verify directory structure**

Run: `ls -la reach-scraper/`
Expected: `Dockerfile`, `requirements.txt`

- [ ] **Step 4: Commit**

```bash
git add reach-scraper/Dockerfile reach-scraper/requirements.txt
git commit -m "feat(scraper): add Dockerfile and requirements for reach-scraper service"
```

---

### Task 2: Create the Flask API

**Files:**
- Create: `reach-scraper/api.py`

- [ ] **Step 1: Create api.py with all three endpoints**

Create `reach-scraper/api.py`:

```python
import os
import json
from datetime import datetime, timezone
from flask import Flask, request, jsonify
from functools import wraps
from playwright.sync_api import sync_playwright

app = Flask(__name__)

API_KEY = os.environ.get("REACH_API_KEY", "")

# ── Auth ──────────────────────────────────────────────────────────

def require_auth(f):
    @wraps(f)
    def decorated(*args, **kwargs):
        auth = request.headers.get("Authorization", "")
        token = auth.removeprefix("Bearer ").strip()
        if not API_KEY or token != API_KEY:
            return jsonify({"error": "Unauthorized"}), 401
        return f(*args, **kwargs)
    return decorated

# ── Browser singleton ─────────────────────────────────────────────

_playwright = None
_browser = None

def get_browser():
    global _playwright, _browser
    if _browser is None or not _browser.is_connected():
        _playwright = sync_playwright().start()
        _browser = _playwright.chromium.launch(
            headless=False,  # Uses Xvfb display
            args=[
                "--no-sandbox",
                "--disable-dev-shm-usage",
                "--disable-gpu",
                "--disable-extensions",
            ],
        )
    return _browser

# ── Endpoints ─────────────────────────────────────────────────────

@app.route("/health", methods=["GET"])
def health():
    return jsonify({"status": "ok"})


@app.route("/scrape", methods=["POST"])
@require_auth
def scrape():
    data = request.get_json()
    if not data or "url" not in data:
        return jsonify({"error": "url is required"}), 400

    url = data["url"]
    selectors = data.get("selectors", {})
    wait_for = data.get("wait_for")
    max_items = data.get("max_items", 20)

    browser = get_browser()
    page = browser.new_page()

    try:
        page.goto(url, wait_until="domcontentloaded", timeout=30000)

        if wait_for:
            try:
                page.wait_for_selector(wait_for, timeout=10000)
            except Exception:
                pass  # Continue even if selector doesn't appear

        # Allow dynamic content to render
        page.wait_for_timeout(2000)

        items_selector = selectors.get("items", "article")
        title_selector = selectors.get("title", "h2, h3")
        score_selector = selectors.get("score")
        link_selector = selectors.get("link", "a")
        snippet_selector = selectors.get("snippet")
        comments_selector = selectors.get("comments")

        items = []
        elements = page.query_selector_all(items_selector)

        for el in elements[:max_items]:
            item = {}

            title_el = el.query_selector(title_selector)
            if title_el:
                item["title"] = title_el.inner_text().strip()

            if score_selector:
                score_el = el.query_selector(score_selector)
                if score_el:
                    item["score"] = score_el.inner_text().strip()

            link_el = el.query_selector(link_selector)
            if link_el:
                href = link_el.get_attribute("href")
                if href:
                    if href.startswith("/"):
                        from urllib.parse import urljoin
                        href = urljoin(url, href)
                    item["link"] = href

            if snippet_selector:
                snippet_el = el.query_selector(snippet_selector)
                if snippet_el:
                    item["snippet"] = snippet_el.inner_text().strip()[:300]

            if comments_selector:
                comments_el = el.query_selector(comments_selector)
                if comments_el:
                    item["comments"] = comments_el.inner_text().strip()

            if item.get("title"):
                items.append(item)

        return jsonify({
            "url": url,
            "items": items,
            "scraped_at": datetime.now(timezone.utc).isoformat(),
        })

    except Exception as e:
        return jsonify({
            "url": url,
            "error": str(e),
            "items": [],
            "scraped_at": datetime.now(timezone.utc).isoformat(),
        }), 500

    finally:
        page.close()


@app.route("/browse", methods=["POST"])
@require_auth
def browse():
    data = request.get_json()
    if not data or "url" not in data:
        return jsonify({"error": "url is required"}), 400

    url = data["url"]
    wait_for = data.get("wait_for")

    browser = get_browser()
    page = browser.new_page()

    try:
        page.goto(url, wait_until="domcontentloaded", timeout=30000)

        if wait_for:
            try:
                page.wait_for_selector(wait_for, timeout=10000)
            except Exception:
                pass

        page.wait_for_timeout(2000)

        title = page.title()
        content = page.inner_text("body")

        # Truncate to avoid massive responses
        if len(content) > 50000:
            content = content[:50000] + "\n... (truncated)"

        return jsonify({
            "url": url,
            "title": title,
            "content": content,
            "scraped_at": datetime.now(timezone.utc).isoformat(),
        })

    except Exception as e:
        return jsonify({
            "url": url,
            "error": str(e),
            "title": "",
            "content": "",
            "scraped_at": datetime.now(timezone.utc).isoformat(),
        }), 500

    finally:
        page.close()
```

- [ ] **Step 2: Verify file exists**

Run: `cat reach-scraper/api.py | head -5`
Expected: `import os` etc.

- [ ] **Step 3: Commit**

```bash
git add reach-scraper/api.py
git commit -m "feat(scraper): add Flask API with /scrape, /browse, /health endpoints"
```

---

### Task 3: Build and Test Locally

**Files:** None (verification only)

- [ ] **Step 1: Build the Docker image**

Run:
```bash
cd reach-scraper && docker build -t reach-scraper . 2>&1 | tail -5
```
Expected: `Successfully tagged reach-scraper:latest` (or similar success message). This will take a few minutes on first build since the Reach base image is large.

- [ ] **Step 2: Run the container**

Run:
```bash
docker run -d --name reach-scraper-test \
  -p 8080:8080 \
  -e REACH_API_KEY=test-key \
  -e PORT=8080 \
  --shm-size=2g \
  reach-scraper
```

Note: `--shm-size=2g` is important for Chrome — it uses shared memory and will crash without enough.

- [ ] **Step 3: Wait for startup and test health**

Run:
```bash
sleep 5 && curl -s http://localhost:8080/health
```
Expected: `{"status": "ok"}`

- [ ] **Step 4: Test the /scrape endpoint with Hacker News**

Run:
```bash
curl -s -X POST http://localhost:8080/scrape \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer test-key" \
  -d '{
    "url": "https://news.ycombinator.com/",
    "selectors": {
      "items": ".athing",
      "title": ".titleline > a",
      "score": "+ tr .score",
      "link": ".titleline > a"
    },
    "wait_for": ".titleline",
    "max_items": 5
  }' | python3 -m json.tool | head -20
```
Expected: JSON with 5 items containing titles and links from HN front page.

- [ ] **Step 5: Test the /browse endpoint**

Run:
```bash
curl -s -X POST http://localhost:8080/browse \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer test-key" \
  -d '{
    "url": "https://news.ycombinator.com/",
    "wait_for": ".titleline"
  }' | python3 -c "import json,sys; d=json.load(sys.stdin); print(f'Title: {d[\"title\"]}'); print(f'Content length: {len(d[\"content\"])} chars')"
```
Expected: `Title: Hacker News` and content length > 0.

- [ ] **Step 6: Test auth rejection**

Run:
```bash
curl -s -o /dev/null -w "%{http_code}" -X POST http://localhost:8080/scrape \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}'
```
Expected: `401`

- [ ] **Step 7: Clean up**

Run:
```bash
docker stop reach-scraper-test && docker rm reach-scraper-test
```

- [ ] **Step 8: Commit (no code changes, but note test results)**

No commit needed — this was verification only.

---

### Task 4: Railway Deployment Configuration

**Files:**
- Create: `reach-scraper/.dockerignore`

- [ ] **Step 1: Create .dockerignore**

Create `reach-scraper/.dockerignore`:

```
__pycache__
*.pyc
.env
```

- [ ] **Step 2: Commit**

```bash
git add reach-scraper/.dockerignore
git commit -m "feat(scraper): add .dockerignore for Railway deployment"
```

The Railway service is configured via the Railway dashboard:
- Create a new service in the Postgraph Railway project
- Set root directory to `reach-scraper/`
- Set environment variables: `REACH_API_KEY`, `PORT` (Railway auto-sets PORT)
- Set memory to 2GB, add `--shm-size=2g` equivalent (Railway supports this in service settings)
