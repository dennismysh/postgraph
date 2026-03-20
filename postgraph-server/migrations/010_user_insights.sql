-- Store user-level insights from the Threads API as the authoritative
-- source for total views (per-post sums were corrupted by pre-GREATEST syncs).
CREATE TABLE IF NOT EXISTS user_insights (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    total_views BIGINT NOT NULL DEFAULT 0,
    captured_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
INSERT INTO user_insights (id) VALUES (1) ON CONFLICT DO NOTHING;
