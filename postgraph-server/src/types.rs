use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Post {
    pub id: String,
    pub text: Option<String>,
    pub media_type: Option<String>,
    pub media_url: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub permalink: Option<String>,
    pub likes: i32,
    pub replies_count: i32,
    pub reposts: i32,
    pub quotes: i32,
    pub sentiment: Option<f32>,
    pub synced_at: DateTime<Utc>,
    pub analyzed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Topic {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PostTopic {
    pub post_id: String,
    pub topic_id: Uuid,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PostEdge {
    pub source_post_id: String,
    pub target_post_id: String,
    pub edge_type: String,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EngagementSnapshot {
    pub id: Uuid,
    pub post_id: String,
    pub captured_at: DateTime<Utc>,
    pub likes: i32,
    pub replies_count: i32,
    pub reposts: i32,
    pub quotes: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SyncState {
    pub id: i32,
    pub last_sync_cursor: Option<String>,
    pub last_sync_at: Option<DateTime<Utc>>,
}
