use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;

use crate::error::AppError;
use crate::mercury::MercuryClient;

pub const EMOTIONS: &[&str] = &[
    "vulnerable",
    "curious",
    "playful",
    "confident",
    "reflective",
    "frustrated",
    "provocative",
];

#[derive(Debug, Serialize, Deserialize)]
pub struct EmotionStat {
    pub name: String,
    pub post_count: i64,
    pub percentage: f64,
    pub avg_views: f64,
    pub avg_likes: f64,
    pub avg_replies: f64,
    pub avg_reposts: f64,
    pub top_post_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmotionsSummary {
    pub window_start: String,
    pub window_end: String,
    pub total_posts: i64,
    pub emotions: Vec<EmotionStat>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmotionObservation {
    pub text: String,
    pub cited_posts: Vec<String>,
    pub emotion: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmotionNarrative {
    pub headline: String,
    pub observations: Vec<EmotionObservation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredNarrative {
    pub id: String,
    pub generated_at: chrono::DateTime<Utc>,
    pub trigger_type: String,
    pub narrative: EmotionNarrative,
}

pub async fn compute_summary(pool: &PgPool) -> Result<EmotionsSummary, AppError> {
    let now = Utc::now();
    let window_start = now - chrono::Duration::days(30);

    let total_row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM posts WHERE emotion IS NOT NULL AND timestamp >= $1")
            .bind(window_start)
            .fetch_one(pool)
            .await?;
    let total_posts = total_row.0;

    let rows: Vec<(String, i64, f64, f64, f64, f64)> = sqlx::query_as(
        r#"SELECT
               emotion,
               COUNT(*) AS post_count,
               COALESCE(AVG(views::float8), 0.0) AS avg_views,
               COALESCE(AVG(likes::float8), 0.0) AS avg_likes,
               COALESCE(AVG(replies_count::float8), 0.0) AS avg_replies,
               COALESCE(AVG(reposts::float8), 0.0) AS avg_reposts
           FROM posts
           WHERE emotion IS NOT NULL AND timestamp >= $1
           GROUP BY emotion
           ORDER BY post_count DESC"#,
    )
    .bind(window_start)
    .fetch_all(pool)
    .await?;

    let mut emotions = Vec::new();
    for (name, post_count, avg_views, avg_likes, avg_replies, avg_reposts) in rows {
        let top_post: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM posts WHERE emotion = $1 AND timestamp >= $2 ORDER BY views DESC LIMIT 1",
        )
        .bind(&name)
        .bind(window_start)
        .fetch_optional(pool)
        .await?;

        let percentage = if total_posts > 0 {
            (post_count as f64 / total_posts as f64) * 100.0
        } else {
            0.0
        };

        emotions.push(EmotionStat {
            name,
            post_count,
            percentage,
            avg_views,
            avg_likes,
            avg_replies,
            avg_reposts,
            top_post_id: top_post.map(|(id,)| id),
        });
    }

    Ok(EmotionsSummary {
        window_start: window_start.format("%Y-%m-%d").to_string(),
        window_end: now.format("%Y-%m-%d").to_string(),
        total_posts,
        emotions,
    })
}

pub async fn generate_narrative(
    pool: &PgPool,
    mercury: &MercuryClient,
    trigger_type: &str,
) -> Result<StoredNarrative, AppError> {
    let summary = compute_summary(pool).await?;

    if summary.total_posts < 5 {
        return Err(AppError::MercuryApi(format!(
            "Insufficient data: need at least 5 posts with emotions in the last 30 days, found {}",
            summary.total_posts
        )));
    }

    info!(
        "Generating emotion narrative (trigger={trigger_type}, posts={})",
        summary.total_posts
    );

    let narrative = mercury.generate_emotion_narrative(&summary).await?;

    let context_json = serde_json::to_value(&summary)?;
    let narrative_json = serde_json::to_value(&narrative)?;

    let row: (uuid::Uuid, chrono::DateTime<Utc>) = sqlx::query_as(
        r#"INSERT INTO emotion_narratives (trigger_type, narrative, context)
           VALUES ($1, $2, $3)
           RETURNING id, generated_at"#,
    )
    .bind(trigger_type)
    .bind(&narrative_json)
    .bind(&context_json)
    .fetch_one(pool)
    .await?;

    info!("Stored emotion narrative id={}", row.0);

    Ok(StoredNarrative {
        id: row.0.to_string(),
        generated_at: row.1,
        trigger_type: trigger_type.to_string(),
        narrative,
    })
}

fn backfill_batch_size() -> i64 {
    std::env::var("ANALYSIS_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16)
}

pub async fn backfill_emotions(pool: &PgPool, mercury: &MercuryClient) -> Result<u32, AppError> {
    let batch_size = backfill_batch_size();
    let mut total_classified: u32 = 0;

    loop {
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            r#"SELECT id, text FROM posts
               WHERE analyzed_at IS NOT NULL AND emotion IS NULL AND text IS NOT NULL
               ORDER BY timestamp DESC
               LIMIT $1"#,
        )
        .bind(batch_size)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            break;
        }

        let posts_for_llm: Vec<(String, String)> = rows
            .into_iter()
            .filter_map(|(id, text)| text.map(|t| (id, t)))
            .collect();

        if posts_for_llm.is_empty() {
            break;
        }

        info!("Backfilling emotions for {} posts", posts_for_llm.len());

        let results = mercury.classify_emotions(&posts_for_llm).await?;

        for result in &results {
            let emotion = result.emotion.to_lowercase();
            if EMOTIONS.contains(&emotion.as_str()) {
                sqlx::query("UPDATE posts SET emotion = $1 WHERE id = $2")
                    .bind(&emotion)
                    .bind(&result.post_id)
                    .execute(pool)
                    .await?;
                total_classified += 1;
            } else {
                tracing::warn!(
                    "Mercury returned unknown emotion '{}' for post {}, skipping",
                    result.emotion,
                    result.post_id
                );
            }
        }

        info!(
            "Backfill batch complete: {} classified so far",
            total_classified
        );
    }

    info!(
        "Emotion backfill complete: {} total posts classified",
        total_classified
    );
    Ok(total_classified)
}

pub async fn get_latest_narrative(pool: &PgPool) -> Result<Option<StoredNarrative>, AppError> {
    let row: Option<(uuid::Uuid, chrono::DateTime<Utc>, String, serde_json::Value)> =
        sqlx::query_as(
            r#"SELECT id, generated_at, trigger_type, narrative
               FROM emotion_narratives
               ORDER BY generated_at DESC
               LIMIT 1"#,
        )
        .fetch_optional(pool)
        .await?;

    match row {
        None => Ok(None),
        Some((id, generated_at, trigger_type, narrative_json)) => {
            let narrative: EmotionNarrative = serde_json::from_value(narrative_json)?;
            Ok(Some(StoredNarrative {
                id: id.to_string(),
                generated_at,
                trigger_type,
                narrative,
            }))
        }
    }
}
