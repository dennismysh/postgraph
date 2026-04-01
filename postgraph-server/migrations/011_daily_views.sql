-- Create authoritative daily views table (user-level insights API)
CREATE TABLE daily_views (
    date DATE PRIMARY KEY,
    views BIGINT NOT NULL,
    source TEXT NOT NULL DEFAULT 'user_insights',
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Drop the single-row user_insights table (replaced by daily_views)
DROP TABLE IF EXISTS user_insights;
