-- Remove reposted content (REPOST_FACADE) from the database.
-- These are other users' posts that were reposted to our feed.
-- Cascading deletes handle post_topics, post_edges, and engagement_snapshots.
DELETE FROM posts WHERE media_type = 'REPOST_FACADE';
