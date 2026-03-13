-- Add views column to posts table (Threads API insights metric)
ALTER TABLE posts ADD COLUMN views INTEGER NOT NULL DEFAULT 0;
