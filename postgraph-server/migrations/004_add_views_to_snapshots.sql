-- Add views column to engagement_snapshots for time-series views tracking
ALTER TABLE engagement_snapshots ADD COLUMN views INTEGER NOT NULL DEFAULT 0;
