-- Remove remaining reposts that slipped through migration 006.
-- Some reposts were inserted with NULL media_type, or were synced
-- after migration 006 ran but before the sync filter was deployed.
-- Posts with no text and zero engagement across all metrics are reposts.
-- Cascading deletes handle post_topics, post_edges, and engagement_snapshots.
DELETE FROM posts
WHERE media_type = 'REPOST_FACADE'
   OR (text IS NULL AND views = 0 AND likes = 0 AND replies_count = 0 AND reposts = 0 AND quotes = 0);
