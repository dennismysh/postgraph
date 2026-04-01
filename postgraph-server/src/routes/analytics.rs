use crate::db;
use crate::state::AppState;
use axum::{Json, extract::Path, extract::Query, extract::State};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Types ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct AnalyticsData {
    pub total_posts: usize,
    pub analyzed_posts: usize,
    pub total_subjects: usize,
    pub total_intents: usize,
    pub total_views: i64,
    pub subjects: Vec<SubjectSummary>,
    pub engagement_over_time: Vec<EngagementPoint>,
}

#[derive(Serialize)]
pub struct SubjectSummary {
    pub name: String,
    pub post_count: i64,
    pub avg_engagement: f64,
}

#[derive(Serialize)]
pub struct EngagementPoint {
    pub date: String,
    pub likes: i64,
    pub replies: i64,
    pub reposts: i64,
}

#[derive(Serialize)]
pub struct ViewsPoint {
    pub date: String,
    pub views: i64,
}

#[derive(Serialize)]
pub struct CumulativeViewsPoint {
    pub date: String,
    pub cumulative_views: i64,
}

#[derive(Deserialize)]
pub struct ViewsQuery {
    pub since: Option<String>,
    pub grouping: Option<String>,
}

#[derive(Serialize)]
pub struct ViewsRangeSums {
    pub sums: HashMap<String, i64>,
}

#[derive(Serialize)]
pub struct PostEngagementPoint {
    pub date: String,
    pub views: i32,
    pub likes: i32,
    pub replies: i32,
    pub reposts: i32,
    pub quotes: i32,
}

#[derive(Deserialize)]
pub struct HeatmapQuery {
    pub range: Option<String>,
}

#[derive(Serialize)]
pub struct HeatmapDay {
    pub date: String,
    pub posts: i64,
    pub likes: i64,
    pub replies: i64,
    pub reposts: i64,
    pub views: i64,
    pub media_types: HashMap<String, i64>,
}

#[derive(Serialize)]
pub struct HeatmapResponse {
    pub days: Vec<HeatmapDay>,
}

#[derive(Serialize)]
pub struct HistogramBucket {
    pub bucket_min: i64,
    pub bucket_max: i64,
    pub label: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct HistogramResponse {
    pub engagement: Vec<HistogramBucket>,
    pub views: Vec<HistogramBucket>,
}

#[derive(Deserialize)]
pub struct HistogramQuery {
    pub since: Option<String>,
}

// ── Chart A: Daily Reach (from daily_views) ─────────────────────────

pub async fn get_views(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<ViewsPoint>>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.date_naive());

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT date::text, views
           FROM daily_views
           WHERE ($1::date IS NULL OR date >= $1)
           ORDER BY date"#,
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let points: Vec<ViewsPoint> = rows
        .into_iter()
        .map(|(date, views)| ViewsPoint { date, views })
        .collect();

    Ok(Json(points))
}

// ── Chart C: Growth Trajectory (cumulative daily_views) ─────────────

pub async fn get_views_cumulative(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<CumulativeViewsPoint>>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.date_naive());

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT date::text,
                  SUM(views) OVER (ORDER BY date)::bigint AS cumulative_views
           FROM daily_views
           WHERE ($1::date IS NULL OR date >= $1)
           ORDER BY date"#,
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let points: Vec<CumulativeViewsPoint> = rows
        .into_iter()
        .map(|(date, cumulative_views)| CumulativeViewsPoint {
            date,
            cumulative_views,
        })
        .collect();

    Ok(Json(points))
}

// ── Engagement Over Time (capture-time attribution) ─────────────────

pub async fn get_engagement(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<EngagementPoint>>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let is_hourly = query.grouping.as_deref() == Some("hourly");
    let (date_expr, date_format) = if is_hourly {
        (
            "DATE_TRUNC('hour', captured_at)",
            "TO_CHAR(DATE_TRUNC('hour', captured_at), 'YYYY-MM-DD HH24:00')",
        )
    } else {
        ("DATE(captured_at)", "DATE(captured_at)::text")
    };

    // Attribution: always use captured_at (when we observed the delta)
    let sql = format!(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.likes,
                      es.replies_count,
                      es.reposts,
                      MAX(es.likes) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_likes,
                      MAX(es.replies_count) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_replies,
                      MAX(es.reposts) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_reposts
               FROM engagement_snapshots es
           ),
           with_deltas AS (
               SELECT captured_at,
                      GREATEST(likes - COALESCE(prev_likes, 0), 0) AS like_delta,
                      GREATEST(replies_count - COALESCE(prev_replies, 0), 0) AS reply_delta,
                      GREATEST(reposts - COALESCE(prev_reposts, 0), 0) AS repost_delta
               FROM ordered_snapshots
           )
           SELECT {date_format} AS date,
                  SUM(like_delta)::bigint,
                  SUM(reply_delta)::bigint,
                  SUM(repost_delta)::bigint
           FROM with_deltas
           WHERE ($1::timestamptz IS NULL OR captured_at >= $1)
           GROUP BY {date_expr}
           ORDER BY date"#,
    );

    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(&sql)
        .bind(since)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let points: Vec<EngagementPoint> = rows
        .into_iter()
        .map(|(date, likes, replies, reposts)| EngagementPoint {
            date,
            likes,
            replies,
            reposts,
        })
        .collect();

    Ok(Json(points))
}

// ── Range Sums (single pass over daily_views) ───────────────────────

pub async fn get_views_range_sums(
    State(state): State<AppState>,
) -> Result<Json<ViewsRangeSums>, axum::http::StatusCode> {
    let now = chrono::Utc::now().date_naive();
    let b365 = now - chrono::Duration::days(365);
    let b270 = now - chrono::Duration::days(270);
    let b180 = now - chrono::Duration::days(180);
    let b90 = now - chrono::Duration::days(90);
    let b60 = now - chrono::Duration::days(60);
    let b30 = now - chrono::Duration::days(30);
    let b14 = now - chrono::Duration::days(14);
    let b7 = now - chrono::Duration::days(7);
    let b1 = now - chrono::Duration::days(1);

    let row: (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT
               COALESCE(SUM(views), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $1 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $2 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $3 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $4 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $5 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $6 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $7 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $8 THEN views END), 0)::bigint,
               COALESCE(SUM(CASE WHEN date >= $9 THEN views END), 0)::bigint
           FROM daily_views"#,
    )
    .bind(b365)
    .bind(b270)
    .bind(b180)
    .bind(b90)
    .bind(b60)
    .bind(b30)
    .bind(b14)
    .bind(b7)
    .bind(b1)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut sums = HashMap::new();
    sums.insert("all".to_string(), row.0);
    sums.insert("365d".to_string(), row.1);
    sums.insert("270d".to_string(), row.2);
    sums.insert("180d".to_string(), row.3);
    sums.insert("90d".to_string(), row.4);
    sums.insert("60d".to_string(), row.5);
    sums.insert("30d".to_string(), row.6);
    sums.insert("14d".to_string(), row.7);
    sums.insert("7d".to_string(), row.8);
    sums.insert("24h".to_string(), row.9);

    Ok(Json(ViewsRangeSums { sums }))
}

// ── Analytics Summary ───────────────────────────────────────────────

pub async fn get_analytics(
    State(state): State<AppState>,
) -> Result<Json<AnalyticsData>, axum::http::StatusCode> {
    let posts = db::get_all_posts(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let subjects = db::get_all_subjects(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let intents = db::get_all_intents(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let subject_summaries: Vec<SubjectSummary> = sqlx::query_as::<_, (String, i64, f64)>(
        r#"SELECT s.name, COUNT(p.id)::bigint AS post_count,
           COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes), 0)::float8 AS avg_engagement
           FROM subjects s
           LEFT JOIN posts p ON p.subject_id = s.id AND p.analyzed_at IS NOT NULL
           GROUP BY s.name
           ORDER BY post_count DESC"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(|(name, count, avg)| SubjectSummary {
        name,
        post_count: count,
        avg_engagement: avg,
    })
    .collect();

    // Engagement over time: capture-time attribution, no backdating
    let engagement_over_time: Vec<EngagementPoint> = sqlx::query_as::<_, (String, i64, i64, i64)>(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.likes,
                      es.replies_count,
                      es.reposts,
                      MAX(es.likes) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_likes,
                      MAX(es.replies_count) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_replies,
                      MAX(es.reposts) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_reposts
               FROM engagement_snapshots es
           ),
           with_deltas AS (
               SELECT captured_at,
                      GREATEST(likes - COALESCE(prev_likes, 0), 0) AS like_delta,
                      GREATEST(replies_count - COALESCE(prev_replies, 0), 0) AS reply_delta,
                      GREATEST(reposts - COALESCE(prev_reposts, 0), 0) AS repost_delta
               FROM ordered_snapshots
           )
           SELECT DATE(captured_at)::text AS date,
                  SUM(like_delta)::bigint,
                  SUM(reply_delta)::bigint,
                  SUM(repost_delta)::bigint
           FROM with_deltas
           GROUP BY DATE(captured_at)
           ORDER BY date"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(|(date, likes, replies, reposts)| EngagementPoint {
        date,
        likes,
        replies,
        reposts,
    })
    .collect();

    let analyzed_count = posts.iter().filter(|p| p.analyzed_at.is_some()).count();

    // Total views from the authoritative source: daily_views
    let total_views = db::get_daily_views_total(&state.pool).await.unwrap_or(0);

    Ok(Json(AnalyticsData {
        total_posts: posts.len(),
        analyzed_posts: analyzed_count,
        total_subjects: subjects.len(),
        total_intents: intents.len(),
        total_views,
        subjects: subject_summaries,
        engagement_over_time,
    }))
}

// ── Per-Post Engagement (raw snapshots, unchanged) ──────────────────

pub async fn get_post_engagement(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<Vec<PostEngagementPoint>>, axum::http::StatusCode> {
    let snapshots = db::get_engagement_history(&state.pool, &post_id)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let points: Vec<PostEngagementPoint> = snapshots
        .into_iter()
        .map(|s| PostEngagementPoint {
            date: s.captured_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            views: s.views,
            likes: s.likes,
            replies: s.replies_count,
            reposts: s.reposts,
            quotes: s.quotes,
        })
        .collect();

    Ok(Json(points))
}

// ── Heatmap A: Daily Reach (from daily_views) ───────────────────────

pub async fn get_views_heatmap(
    State(state): State<AppState>,
    Query(query): Query<HeatmapQuery>,
) -> Result<Json<HeatmapResponse>, axum::http::StatusCode> {
    let since = match query.range.as_deref() {
        Some("3m") => chrono::Utc::now().date_naive() - chrono::Duration::days(90),
        Some("6m") => chrono::Utc::now().date_naive() - chrono::Duration::days(180),
        Some("all") => chrono::NaiveDate::MIN,
        _ => chrono::Utc::now().date_naive() - chrono::Duration::days(365),
    };

    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT date::text, views FROM daily_views WHERE date >= $1 ORDER BY date"#,
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let days: Vec<HeatmapDay> = rows
        .into_iter()
        .map(|(date, views)| HeatmapDay {
            date,
            posts: 0,
            likes: 0,
            replies: 0,
            reposts: 0,
            views,
            media_types: HashMap::new(),
        })
        .collect();

    Ok(Json(HeatmapResponse { days }))
}

// ── Heatmap B: Posting Activity (by publish date, no views) ─────────

pub async fn get_heatmap(
    State(state): State<AppState>,
    Query(query): Query<HeatmapQuery>,
) -> Result<Json<HeatmapResponse>, axum::http::StatusCode> {
    let since = match query.range.as_deref() {
        Some("3m") => chrono::Utc::now() - chrono::Duration::days(90),
        Some("6m") => chrono::Utc::now() - chrono::Duration::days(180),
        Some("all") => chrono::DateTime::<chrono::Utc>::MIN_UTC,
        _ => chrono::Utc::now() - chrono::Duration::days(365),
    };

    let rows: Vec<(String, i64, i64, i64, i64, Option<String>)> = sqlx::query_as(
        r#"SELECT DATE(timestamp)::text AS date,
                  COUNT(*) AS posts,
                  SUM(likes)::bigint AS likes,
                  SUM(replies_count)::bigint AS replies,
                  SUM(reposts)::bigint AS reposts,
                  media_type
           FROM posts
           WHERE timestamp >= $1
           GROUP BY DATE(timestamp), media_type
           ORDER BY date"#,
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut day_map: std::collections::BTreeMap<String, HeatmapDay> =
        std::collections::BTreeMap::new();
    for (date, posts, likes, replies, reposts, media_type) in rows {
        let entry = day_map.entry(date.clone()).or_insert_with(|| HeatmapDay {
            date,
            posts: 0,
            likes: 0,
            replies: 0,
            reposts: 0,
            views: 0,
            media_types: HashMap::new(),
        });
        entry.posts += posts;
        entry.likes += likes;
        entry.replies += replies;
        entry.reposts += reposts;
        if let Some(mt) = media_type {
            *entry.media_types.entry(mt).or_insert(0) += posts;
        }
    }

    Ok(Json(HeatmapResponse {
        days: day_map.into_values().collect(),
    }))
}

// ── Histograms (unchanged — already correct) ────────────────────────

pub async fn get_histograms(
    State(state): State<AppState>,
    Query(query): Query<HistogramQuery>,
) -> Result<Json<HistogramResponse>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let engagement_sql = r#"
        WITH buckets(bucket_min, bucket_max, label, ord) AS (
            VALUES
                (0, 0, '0', 1),
                (1, 5, '1-5', 2),
                (6, 10, '6-10', 3),
                (11, 25, '11-25', 4),
                (26, 50, '26-50', 5),
                (51, 100, '51-100', 6),
                (101, 250, '101-250', 7),
                (251, 500, '251-500', 8),
                (501, 1000, '501-1k', 9),
                (1001, 2147483647, '1k+', 10)
        ),
        post_engagement AS (
            SELECT (likes + replies_count + reposts + quotes) AS total
            FROM posts
            WHERE ($1::timestamptz IS NULL OR timestamp >= $1)
        )
        SELECT b.bucket_min::bigint, b.bucket_max::bigint, b.label,
               COUNT(p.total)::bigint AS count
        FROM buckets b
        LEFT JOIN post_engagement p ON p.total >= b.bucket_min AND p.total <= b.bucket_max
        GROUP BY b.bucket_min, b.bucket_max, b.label, b.ord
        ORDER BY b.ord
    "#;

    let engagement_rows: Vec<(i64, i64, String, i64)> = sqlx::query_as(engagement_sql)
        .bind(since)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let views_sql = r#"
        WITH buckets(bucket_min, bucket_max, label, ord) AS (
            VALUES
                (0, 0, '0', 1),
                (1, 100, '1-100', 2),
                (101, 500, '101-500', 3),
                (501, 1000, '501-1k', 4),
                (1001, 5000, '1k-5k', 5),
                (5001, 10000, '5k-10k', 6),
                (10001, 50000, '10k-50k', 7),
                (50001, 100000, '50k-100k', 8),
                (100001, 2147483647, '100k+', 9)
        )
        SELECT b.bucket_min::bigint, b.bucket_max::bigint, b.label,
               COUNT(p.id)::bigint AS count
        FROM buckets b
        LEFT JOIN posts p ON p.views >= b.bucket_min AND p.views <= b.bucket_max
            AND ($1::timestamptz IS NULL OR p.timestamp >= $1)
        GROUP BY b.bucket_min, b.bucket_max, b.label, b.ord
        ORDER BY b.ord
    "#;

    let views_rows: Vec<(i64, i64, String, i64)> = sqlx::query_as(views_sql)
        .bind(since)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let engagement = engagement_rows
        .into_iter()
        .map(|(bucket_min, bucket_max, label, count)| HistogramBucket {
            bucket_min,
            bucket_max,
            label,
            count,
        })
        .collect();

    let views = views_rows
        .into_iter()
        .map(|(bucket_min, bucket_max, label, count)| HistogramBucket {
            bucket_min,
            bucket_max,
            label,
            count,
        })
        .collect();

    Ok(Json(HistogramResponse { engagement, views }))
}
