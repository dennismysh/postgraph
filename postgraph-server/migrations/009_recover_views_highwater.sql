-- Recover posts.views from the high-water mark in engagement_snapshots.
-- Migration 004 backfilled snapshots with views=0 and subsequent syncs
-- may have overwritten posts.views with lower API values. This restores
-- each post's views to the maximum value ever recorded in any snapshot.
UPDATE posts p SET views = sub.max_views
FROM (
    SELECT post_id, MAX(views) AS max_views
    FROM engagement_snapshots
    GROUP BY post_id
) sub
WHERE sub.post_id = p.id AND sub.max_views > p.views;
