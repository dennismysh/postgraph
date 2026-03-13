-- Posts cached from Threads API
CREATE TABLE posts (
    id TEXT PRIMARY KEY,
    text TEXT,
    media_type TEXT,
    media_url TEXT,
    timestamp TIMESTAMPTZ NOT NULL,
    permalink TEXT,
    likes INTEGER NOT NULL DEFAULT 0,
    replies_count INTEGER NOT NULL DEFAULT 0,
    reposts INTEGER NOT NULL DEFAULT 0,
    quotes INTEGER NOT NULL DEFAULT 0,
    sentiment REAL,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    analyzed_at TIMESTAMPTZ
);

-- LLM-extracted topics
CREATE TABLE topics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT
);

-- Post-to-topic many-to-many with relevance weight
CREATE TABLE post_topics (
    post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    topic_id UUID NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
    weight REAL NOT NULL DEFAULT 1.0,
    PRIMARY KEY (post_id, topic_id)
);
CREATE INDEX idx_post_topics_topic_id ON post_topics(topic_id);

-- Pre-computed graph edges between posts
CREATE TABLE post_edges (
    source_post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    target_post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    edge_type TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 0.0,
    PRIMARY KEY (source_post_id, target_post_id, edge_type)
);
CREATE INDEX idx_post_edges_target ON post_edges(target_post_id);

-- Engagement time-series
CREATE TABLE engagement_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    captured_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    likes INTEGER NOT NULL DEFAULT 0,
    replies_count INTEGER NOT NULL DEFAULT 0,
    reposts INTEGER NOT NULL DEFAULT 0,
    quotes INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_engagement_post_id ON engagement_snapshots(post_id);
CREATE INDEX idx_engagement_captured_at ON engagement_snapshots(captured_at);

-- Sync bookkeeping
CREATE TABLE sync_state (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    last_sync_cursor TEXT,
    last_sync_at TIMESTAMPTZ
);
INSERT INTO sync_state (id) VALUES (1);

-- Threads API token storage
CREATE TABLE api_tokens (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    access_token TEXT NOT NULL,
    expires_at TIMESTAMPTZ,
    refreshed_at TIMESTAMPTZ DEFAULT NOW()
);
