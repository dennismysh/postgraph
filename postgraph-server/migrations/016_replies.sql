CREATE TABLE replies (
    id TEXT PRIMARY KEY,
    parent_post_id TEXT NOT NULL,
    username TEXT,
    text TEXT,
    timestamp TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'unreplied',
    replied_at TIMESTAMPTZ,
    our_reply_id TEXT,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_replies_status ON replies (status) WHERE status = 'unreplied';
CREATE INDEX idx_replies_parent ON replies (parent_post_id);
