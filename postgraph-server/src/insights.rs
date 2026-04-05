use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;

use crate::error::AppError;
use crate::mercury::MercuryClient;

const MIN_POSTS_THRESHOLD: usize = 5;

// ── Context Types ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PostSummary {
    pub id: String,
    pub text: String,
    pub permalink: Option<String>,
    pub timestamp: String,
    pub views: i32,
    pub likes: i32,
    pub replies: i32,
    pub reposts: i32,
    pub quotes: i32,
    pub intent: Option<String>,
    pub subject: Option<String>,
    pub sentiment: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CategoryStats {
    pub name: String,
    pub recent_post_count: i64,
    pub recent_avg_views: f64,
    pub recent_avg_engagement: f64,
    pub alltime_post_count: i64,
    pub alltime_avg_views: f64,
    pub alltime_avg_engagement: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FrequencyStats {
    pub recent_posts_per_week: f64,
    pub alltime_posts_per_week: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SentimentStats {
    pub recent_avg: f64,
    pub alltime_avg: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DailyViewPoint {
    pub date: String,
    pub views: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InsightsContext {
    pub window_start: String,
    pub window_end: String,
    pub posts: Vec<PostSummary>,
    pub top_posts: Vec<PostSummary>,
    pub bottom_posts: Vec<PostSummary>,
    pub subject_stats: Vec<CategoryStats>,
    pub intent_stats: Vec<CategoryStats>,
    pub posting_frequency: FrequencyStats,
    pub sentiment: SentimentStats,
    pub daily_views: Vec<DailyViewPoint>,
}

// ── Report Types ─────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct InsightsReport {
    pub headline: String,
    pub sections: Vec<InsightsSection>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InsightsSection {
    pub key: String,
    pub title: String,
    pub summary: String,
    pub items: Vec<InsightsItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InsightsItem {
    pub observation: String,
    pub cited_posts: Vec<String>,
    pub tone: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredReport {
    pub id: String,
    pub generated_at: DateTime<Utc>,
    pub trigger_type: String,
    pub report: InsightsReport,
}

// ── compute_context ──────────────────────────────────────────────────

type PostRow = (
    String,
    Option<String>,
    Option<String>,
    String,
    i32,
    i32,
    i32,
    i32,
    i32,
    Option<String>,
    Option<String>,
    Option<f32>,
);

pub async fn compute_context(pool: &PgPool) -> Result<InsightsContext, AppError> {
    let now = Utc::now();
    let window_start = now - chrono::Duration::days(30);
    let window_start_str = window_start.format("%Y-%m-%d").to_string();
    let window_end_str = now.format("%Y-%m-%d").to_string();

    info!("Computing insights context for window {window_start_str}..{window_end_str}");

    // ── Posts in 30d window ──────────────────────────────────────────
    let post_rows: Vec<PostRow> = sqlx::query_as(
        r#"SELECT
               p.id,
               p.text,
               p.permalink,
               p.timestamp::text,
               p.views,
               p.likes,
               p.replies_count,
               p.reposts,
               p.quotes,
               i.name AS intent,
               s.name AS subject,
               p.sentiment
           FROM posts p
           LEFT JOIN intents i ON i.id = p.intent_id
           LEFT JOIN subjects s ON s.id = p.subject_id
           WHERE p.timestamp >= $1
           ORDER BY p.timestamp DESC"#,
    )
    .bind(window_start)
    .fetch_all(pool)
    .await?;

    let mut posts: Vec<PostSummary> = post_rows
        .into_iter()
        .map(
            |(
                id,
                text,
                permalink,
                timestamp,
                views,
                likes,
                replies,
                reposts,
                quotes,
                intent,
                subject,
                sentiment,
            )| {
                PostSummary {
                    id,
                    text: text.unwrap_or_default(),
                    permalink,
                    timestamp,
                    views,
                    likes,
                    replies,
                    reposts,
                    quotes,
                    intent,
                    subject,
                    sentiment,
                }
            },
        )
        .collect();

    // Sort by views descending to make top/bottom slicing easy
    posts.sort_by(|a, b| b.views.cmp(&a.views));
    let top_posts = posts.iter().take(5).cloned().collect();
    let bottom_posts = posts.iter().rev().take(5).cloned().collect();

    // ── Subject stats (recent vs all-time) ──────────────────────────
    let subject_rows: Vec<(
        String,
        i64,
        f64,
        f64,
        i64,
        f64,
        f64,
    )> = sqlx::query_as(
        r#"SELECT
               s.name,
               COUNT(*) FILTER (WHERE p.timestamp >= $1)                                                 AS recent_post_count,
               COALESCE(AVG(p.views) FILTER (WHERE p.timestamp >= $1), 0.0)                             AS recent_avg_views,
               COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes) FILTER (WHERE p.timestamp >= $1), 0.0) AS recent_avg_engagement,
               COUNT(*)                                                                                   AS alltime_post_count,
               COALESCE(AVG(p.views), 0.0)                                                               AS alltime_avg_views,
               COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes), 0.0)                     AS alltime_avg_engagement
           FROM posts p
           JOIN subjects s ON s.id = p.subject_id
           GROUP BY s.name
           ORDER BY recent_post_count DESC"#,
    )
    .bind(window_start)
    .fetch_all(pool)
    .await?;

    let subject_stats: Vec<CategoryStats> = subject_rows
        .into_iter()
        .map(
            |(
                name,
                recent_post_count,
                recent_avg_views,
                recent_avg_engagement,
                alltime_post_count,
                alltime_avg_views,
                alltime_avg_engagement,
            )| {
                CategoryStats {
                    name,
                    recent_post_count,
                    recent_avg_views,
                    recent_avg_engagement,
                    alltime_post_count,
                    alltime_avg_views,
                    alltime_avg_engagement,
                }
            },
        )
        .collect();

    // ── Intent stats (recent vs all-time) ───────────────────────────
    let intent_rows: Vec<(
        String,
        i64,
        f64,
        f64,
        i64,
        f64,
        f64,
    )> = sqlx::query_as(
        r#"SELECT
               i.name,
               COUNT(*) FILTER (WHERE p.timestamp >= $1)                                                 AS recent_post_count,
               COALESCE(AVG(p.views) FILTER (WHERE p.timestamp >= $1), 0.0)                             AS recent_avg_views,
               COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes) FILTER (WHERE p.timestamp >= $1), 0.0) AS recent_avg_engagement,
               COUNT(*)                                                                                   AS alltime_post_count,
               COALESCE(AVG(p.views), 0.0)                                                               AS alltime_avg_views,
               COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes), 0.0)                     AS alltime_avg_engagement
           FROM posts p
           JOIN intents i ON i.id = p.intent_id
           GROUP BY i.name
           ORDER BY recent_post_count DESC"#,
    )
    .bind(window_start)
    .fetch_all(pool)
    .await?;

    let intent_stats: Vec<CategoryStats> = intent_rows
        .into_iter()
        .map(
            |(
                name,
                recent_post_count,
                recent_avg_views,
                recent_avg_engagement,
                alltime_post_count,
                alltime_avg_views,
                alltime_avg_engagement,
            )| {
                CategoryStats {
                    name,
                    recent_post_count,
                    recent_avg_views,
                    recent_avg_engagement,
                    alltime_post_count,
                    alltime_avg_views,
                    alltime_avg_engagement,
                }
            },
        )
        .collect();

    // ── Posting frequency ────────────────────────────────────────────
    let freq_row: (i64, Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
        r#"SELECT
               COUNT(*),
               MIN(timestamp),
               MAX(timestamp)
           FROM posts"#,
    )
    .fetch_one(pool)
    .await?;

    let recent_count_row: (i64,) =
        sqlx::query_as(r#"SELECT COUNT(*) FROM posts WHERE timestamp >= $1"#)
            .bind(window_start)
            .fetch_one(pool)
            .await?;

    let alltime_count = freq_row.0;
    let oldest = freq_row.1;
    let newest = freq_row.2;
    let recent_count = recent_count_row.0;

    let alltime_posts_per_week = if let (Some(oldest), Some(newest)) = (oldest, newest) {
        let days = (newest - oldest).num_seconds() as f64 / 86400.0;
        let weeks = (days / 7.0).max(1.0 / 7.0);
        alltime_count as f64 / weeks
    } else {
        0.0
    };

    let recent_posts_per_week = recent_count as f64 / (30.0 / 7.0);

    let posting_frequency = FrequencyStats {
        recent_posts_per_week,
        alltime_posts_per_week,
    };

    // ── Sentiment ────────────────────────────────────────────────────
    let sentiment_row: (Option<f64>, Option<f64>) = sqlx::query_as(
        r#"SELECT
               AVG(sentiment) FILTER (WHERE timestamp >= $1),
               AVG(sentiment)
           FROM posts
           WHERE sentiment IS NOT NULL"#,
    )
    .bind(window_start)
    .fetch_one(pool)
    .await?;

    let sentiment = SentimentStats {
        recent_avg: sentiment_row.0.unwrap_or(0.0),
        alltime_avg: sentiment_row.1.unwrap_or(0.0),
    };

    // ── Daily views ──────────────────────────────────────────────────
    let view_rows: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT date::text, views
           FROM daily_views
           WHERE date >= $1
           ORDER BY date"#,
    )
    .bind(window_start.date_naive())
    .fetch_all(pool)
    .await?;

    let daily_views = view_rows
        .into_iter()
        .map(|(date, views)| DailyViewPoint { date, views })
        .collect();

    info!(
        "Insights context: {} posts in window, {} subjects, {} intents",
        posts.len(),
        subject_stats.len(),
        intent_stats.len()
    );

    Ok(InsightsContext {
        window_start: window_start_str,
        window_end: window_end_str,
        posts,
        top_posts,
        bottom_posts,
        subject_stats,
        intent_stats,
        posting_frequency,
        sentiment,
        daily_views,
    })
}

// ── generate_report ──────────────────────────────────────────────────

pub async fn generate_report(
    pool: &PgPool,
    mercury: &MercuryClient,
    trigger_type: &str,
) -> Result<StoredReport, AppError> {
    let context = compute_context(pool).await?;

    if context.posts.len() < MIN_POSTS_THRESHOLD {
        return Err(AppError::MercuryApi(format!(
            "Insufficient data: need at least {MIN_POSTS_THRESHOLD} posts in the last 30 days, found {}",
            context.posts.len()
        )));
    }

    info!(
        "Generating insights report (trigger={trigger_type}, posts={})",
        context.posts.len()
    );

    let report = mercury.generate_insights(&context).await?;

    let context_json = serde_json::to_value(&context)?;
    let report_json = serde_json::to_value(&report)?;

    let row: (uuid::Uuid, DateTime<Utc>) = sqlx::query_as(
        r#"INSERT INTO insights_reports (trigger_type, report, context)
           VALUES ($1, $2, $3)
           RETURNING id, generated_at"#,
    )
    .bind(trigger_type)
    .bind(&report_json)
    .bind(&context_json)
    .fetch_one(pool)
    .await?;

    info!("Stored insights report id={}", row.0);

    Ok(StoredReport {
        id: row.0.to_string(),
        generated_at: row.1,
        trigger_type: trigger_type.to_string(),
        report,
    })
}

// ── get_latest_report ────────────────────────────────────────────────

pub async fn get_latest_report(pool: &PgPool) -> Result<Option<StoredReport>, AppError> {
    let row: Option<(uuid::Uuid, DateTime<Utc>, String, serde_json::Value)> = sqlx::query_as(
        r#"SELECT id, generated_at, trigger_type, report
           FROM insights_reports
           ORDER BY generated_at DESC
           LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await?;

    match row {
        None => Ok(None),
        Some((id, generated_at, trigger_type, report_json)) => {
            let report: InsightsReport = serde_json::from_value(report_json)?;
            Ok(Some(StoredReport {
                id: id.to_string(),
                generated_at,
                trigger_type,
                report,
            }))
        }
    }
}
