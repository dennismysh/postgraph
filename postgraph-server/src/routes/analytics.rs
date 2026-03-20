use crate::db;
use crate::state::AppState;
use axum::{Json, extract::Path, extract::Query, extract::State};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

#[derive(Deserialize)]
pub struct ViewsQuery {
    pub since: Option<String>,
    pub grouping: Option<String>,
}

pub async fn get_engagement(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<EngagementPoint>>, axum::http::StatusCode> {
    let since_clause = if let Some(ref since) = query.since {
        format!(
            "WHERE effective_date >= '{}'::timestamptz",
            since.replace('\'', "")
        )
    } else {
        String::new()
    };

    let is_hourly = query.grouping.as_deref() == Some("hourly");
    let (date_expr, date_format) = if is_hourly {
        (
            "DATE_TRUNC('hour', effective_date)",
            "TO_CHAR(DATE_TRUNC('hour', effective_date), 'YYYY-MM-DD HH24:00')",
        )
    } else {
        ("DATE(effective_date)", "DATE(effective_date)::text")
    };

    let sql = format!(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.post_id,
                      es.likes,
                      es.replies_count,
                      es.reposts,
                      MAX(es.likes) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_likes,
                      MAX(es.replies_count) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_replies,
                      MAX(es.reposts) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_reposts,
                      p.timestamp AS post_timestamp
               FROM engagement_snapshots es
               JOIN posts p ON p.id = es.post_id
           ),
           with_deltas AS (
               SELECT CASE
                          WHEN prev_likes IS NULL THEN post_timestamp
                          ELSE captured_at
                      END AS effective_date,
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
           {since_clause}
           GROUP BY {date_expr}
           ORDER BY date"#,
        date_format = date_format,
        since_clause = since_clause,
        date_expr = date_expr,
    );

    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(&sql)
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

pub async fn get_views(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<ViewsPoint>>, axum::http::StatusCode> {
    // Per-post snapshot deltas provide the time distribution.
    // When user-level insights are available and exceed the delta total,
    // we scale the chart proportionally so the sum matches the app total.
    let mut points = get_views_from_snapshots(&state.pool, &query).await?;

    let user_level = db::get_user_insights_total(&state.pool).await.unwrap_or(0);

    if user_level > 0 {
        let delta_total: i64 = points.iter().map(|p| p.views).sum();
        if delta_total > 0 && user_level > delta_total {
            let scale = user_level as f64 / delta_total as f64;
            for point in points.iter_mut() {
                point.views = (point.views as f64 * scale) as i64;
            }
        }
    }

    Ok(points)
}

/// Fallback: compute views from engagement snapshot deltas.
async fn get_views_from_snapshots(
    pool: &sqlx::PgPool,
    query: &ViewsQuery,
) -> Result<Json<Vec<ViewsPoint>>, axum::http::StatusCode> {
    let since_clause = if let Some(ref since) = query.since {
        format!(
            "WHERE effective_date >= '{}'::timestamptz",
            since.replace('\'', "")
        )
    } else {
        String::new()
    };

    let is_hourly = query.grouping.as_deref() == Some("hourly");
    let (date_expr, date_format) = if is_hourly {
        (
            "DATE_TRUNC('hour', effective_date)",
            "TO_CHAR(DATE_TRUNC('hour', effective_date), 'YYYY-MM-DD HH24:00')",
        )
    } else {
        ("DATE(effective_date)", "DATE(effective_date)::text")
    };

    // For the first snapshot of each post (prev_views IS NULL or prev_views = 0),
    // spread the initial delta evenly across the days from post creation to capture.
    // This prevents the chart from showing a huge spike on the first-sync date
    // (March 13) when migration 004 backfilled old snapshots with views=0.
    let sql = format!(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.post_id,
                      es.views,
                      MAX(es.views) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_views,
                      p.timestamp AS post_timestamp
               FROM engagement_snapshots es
               JOIN posts p ON p.id = es.post_id
           ),
           first_snapshot_spread AS (
               -- First snapshot (or zero-prev from migration 004): spread delta
               -- evenly across days from post creation to snapshot capture.
               SELECT gs.day::timestamptz AS effective_date,
                      (GREATEST(os.views - COALESCE(os.prev_views, 0), 0)::float8
                       / GREATEST(EXTRACT(DAY FROM os.captured_at - os.post_timestamp) + 1, 1))::bigint AS view_delta
               FROM ordered_snapshots os,
               LATERAL generate_series(
                   os.post_timestamp::date,
                   os.captured_at::date,
                   '1 day'::interval
               ) AS gs(day)
               WHERE (os.prev_views IS NULL OR os.prev_views = 0)
                 AND os.views > COALESCE(os.prev_views, 0)
           ),
           subsequent_deltas AS (
               SELECT captured_at AS effective_date,
                      GREATEST(views - COALESCE(prev_views, 0), 0) AS view_delta
               FROM ordered_snapshots
               WHERE prev_views IS NOT NULL AND prev_views > 0
           ),
           all_deltas AS (
               SELECT * FROM first_snapshot_spread
               UNION ALL
               SELECT * FROM subsequent_deltas
           )
           SELECT {date_format} AS date,
                  SUM(view_delta)::bigint AS total_views
           FROM all_deltas
           WHERE view_delta > 0
           {since_clause_and}
           GROUP BY {date_expr}
           ORDER BY date"#,
        date_format = date_format.replace("effective_date", "effective_date"),
        since_clause_and = if since_clause.is_empty() {
            String::new()
        } else {
            since_clause.replace("WHERE", "AND")
        },
        date_expr = date_expr,
    );

    let rows: Vec<(String, i64)> = sqlx::query_as(&sql)
        .fetch_all(pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let points: Vec<ViewsPoint> = rows
        .into_iter()
        .map(|(date, views)| ViewsPoint { date, views })
        .collect();

    Ok(Json(points))
}

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

    let engagement_over_time: Vec<EngagementPoint> = sqlx::query_as::<_, (String, i64, i64, i64)>(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.post_id,
                      es.likes,
                      es.replies_count,
                      es.reposts,
                      MAX(es.likes) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_likes,
                      MAX(es.replies_count) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_replies,
                      MAX(es.reposts) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_reposts,
                      p.timestamp AS post_timestamp
               FROM engagement_snapshots es
               JOIN posts p ON p.id = es.post_id
           ),
           with_deltas AS (
               SELECT CASE
                          WHEN prev_likes IS NULL THEN post_timestamp
                          ELSE captured_at
                      END AS effective_date,
                      GREATEST(likes - COALESCE(prev_likes, 0), 0) AS like_delta,
                      GREATEST(replies_count - COALESCE(prev_replies, 0), 0) AS reply_delta,
                      GREATEST(reposts - COALESCE(prev_reposts, 0), 0) AS repost_delta
               FROM ordered_snapshots
           )
           SELECT DATE(effective_date)::text AS date,
                  SUM(like_delta)::bigint,
                  SUM(reply_delta)::bigint,
                  SUM(repost_delta)::bigint
           FROM with_deltas
           GROUP BY DATE(effective_date)
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

    // Use the greatest of three sources for total views:
    // 1. User-level insights from the Threads API (authoritative, matches the app)
    // 2. SUM(posts.views) — protected by GREATEST going forward
    // 3. Delta-based sum from engagement snapshots
    let (post_and_delta_max,): (i64,) = sqlx::query_as(
        r#"SELECT GREATEST(
               (SELECT COALESCE(SUM(views), 0) FROM posts),
               (SELECT COALESCE(SUM(GREATEST(views - COALESCE(prev_views, 0), 0)), 0)
                FROM (SELECT views,
                             MAX(views) OVER (PARTITION BY post_id ORDER BY captured_at
                                 ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_views
                      FROM engagement_snapshots) s)
           )::bigint"#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let user_level = db::get_user_insights_total(&state.pool).await.unwrap_or(0);
    let total_views = post_and_delta_max.max(user_level);

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

#[derive(Serialize)]
pub struct PostEngagementPoint {
    pub date: String,
    pub views: i32,
    pub likes: i32,
    pub replies: i32,
    pub reposts: i32,
    pub quotes: i32,
}

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

pub async fn get_histograms(
    State(state): State<AppState>,
    Query(query): Query<HistogramQuery>,
) -> Result<Json<HistogramResponse>, axum::http::StatusCode> {
    let since = query
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    // Fixed bucket boundaries for engagement (likes + replies + reposts + quotes)
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

    // Fixed bucket boundaries for views
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

pub async fn get_heatmap(
    State(state): State<AppState>,
    Query(query): Query<HeatmapQuery>,
) -> Result<Json<HeatmapResponse>, axum::http::StatusCode> {
    let since = match query.range.as_deref() {
        Some("3m") => chrono::Utc::now() - chrono::Duration::days(90),
        Some("6m") => chrono::Utc::now() - chrono::Duration::days(180),
        Some("all") => chrono::DateTime::<chrono::Utc>::MIN_UTC,
        _ => chrono::Utc::now() - chrono::Duration::days(365), // default 1y
    };

    let rows: Vec<(String, i64, i64, i64, i64, i64, Option<String>)> = sqlx::query_as(
        r#"SELECT DATE(timestamp)::text AS date,
                  COUNT(*) AS posts,
                  SUM(likes)::bigint AS likes,
                  SUM(replies_count)::bigint AS replies,
                  SUM(reposts)::bigint AS reposts,
                  SUM(views)::bigint AS views,
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

    // Aggregate rows by date (multiple rows per date due to media_type grouping)
    let mut day_map: std::collections::BTreeMap<String, HeatmapDay> =
        std::collections::BTreeMap::new();
    for (date, posts, likes, replies, reposts, views, media_type) in rows {
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
        entry.views += views;
        if let Some(mt) = media_type {
            *entry.media_types.entry(mt).or_insert(0) += posts;
        }
    }

    Ok(Json(HeatmapResponse {
        days: day_map.into_values().collect(),
    }))
}

#[derive(Serialize)]
pub struct ViewsRangeSums {
    pub sums: HashMap<String, i64>,
}

pub async fn get_views_range_sums(
    State(state): State<AppState>,
) -> Result<Json<ViewsRangeSums>, axum::http::StatusCode> {
    let now = chrono::Utc::now();
    let b365 = now - chrono::Duration::days(365);
    let b270 = now - chrono::Duration::days(270);
    let b180 = now - chrono::Duration::days(180);
    let b90 = now - chrono::Duration::days(90);
    let b60 = now - chrono::Duration::days(60);
    let b30 = now - chrono::Duration::days(30);
    let b14 = now - chrono::Duration::days(14);
    let b7 = now - chrono::Duration::days(7);
    let b1 = now - chrono::Duration::days(1);

    // Delta-based approach with first-snapshot spreading (matching chart query).
    // Spreads initial view deltas across post lifetimes to avoid attribution spikes.
    let row: (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.post_id,
                      es.views,
                      MAX(es.views) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_views,
                      p.timestamp AS post_timestamp
               FROM engagement_snapshots es
               JOIN posts p ON p.id = es.post_id
           ),
           first_snapshot_spread AS (
               SELECT gs.day::timestamptz AS effective_date,
                      (GREATEST(os.views - COALESCE(os.prev_views, 0), 0)::float8
                       / GREATEST(EXTRACT(DAY FROM os.captured_at - os.post_timestamp) + 1, 1))::bigint AS view_delta
               FROM ordered_snapshots os,
               LATERAL generate_series(
                   os.post_timestamp::date,
                   os.captured_at::date,
                   '1 day'::interval
               ) AS gs(day)
               WHERE (os.prev_views IS NULL OR os.prev_views = 0)
                 AND os.views > COALESCE(os.prev_views, 0)
           ),
           subsequent_deltas AS (
               SELECT captured_at AS effective_date,
                      GREATEST(views - COALESCE(prev_views, 0), 0) AS view_delta
               FROM ordered_snapshots
               WHERE prev_views IS NOT NULL AND prev_views > 0
           ),
           all_deltas AS (
               SELECT * FROM first_snapshot_spread
               UNION ALL
               SELECT * FROM subsequent_deltas
           )
           SELECT
               COALESCE(SUM(view_delta), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $1 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $2 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $3 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $4 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $5 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $6 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $7 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $8 THEN view_delta END), 0)::bigint,
               COALESCE(SUM(CASE WHEN effective_date >= $9 THEN view_delta END), 0)::bigint
           FROM all_deltas
           WHERE view_delta > 0"#,
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

    tracing::info!(
        all = row.0,
        d365 = row.1,
        d270 = row.2,
        d180 = row.3,
        d90 = row.4,
        d60 = row.5,
        d30 = row.6,
        d14 = row.7,
        d7 = row.8,
        d24h = row.9,
        "views range sums computed"
    );

    // Guard "all" against undercounting: use the greatest of delta total,
    // posts.views sum, and user-level insights (authoritative).
    let (posts_sum,): (i64,) = sqlx::query_as("SELECT COALESCE(SUM(views), 0)::bigint FROM posts")
        .fetch_one(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let user_level = db::get_user_insights_total(&state.pool).await.unwrap_or(0);

    let delta_all = row.0;
    let best_all = delta_all.max(posts_sum).max(user_level);

    // Scale range sums proportionally when user-level total exceeds per-post sum.
    // This distributes the "missing" views (from pre-GREATEST data corruption)
    // proportionally across all time ranges.
    let scale = if delta_all > 0 && best_all > delta_all {
        best_all as f64 / delta_all as f64
    } else {
        1.0
    };

    let mut sums = HashMap::new();
    sums.insert("all".to_string(), best_all);
    sums.insert("365d".to_string(), (row.1 as f64 * scale) as i64);
    sums.insert("270d".to_string(), (row.2 as f64 * scale) as i64);
    sums.insert("180d".to_string(), (row.3 as f64 * scale) as i64);
    sums.insert("90d".to_string(), (row.4 as f64 * scale) as i64);
    sums.insert("60d".to_string(), (row.5 as f64 * scale) as i64);
    sums.insert("30d".to_string(), (row.6 as f64 * scale) as i64);
    sums.insert("14d".to_string(), (row.7 as f64 * scale) as i64);
    sums.insert("7d".to_string(), (row.8 as f64 * scale) as i64);
    sums.insert("24h".to_string(), (row.9 as f64 * scale) as i64);

    if scale > 1.01 {
        tracing::info!(
            scale = format!("{:.2}", scale),
            user_level,
            delta_all,
            posts_sum,
            "Scaling range sums to match user-level total"
        );
    }

    Ok(Json(ViewsRangeSums { sums }))
}

/// Diagnostic endpoint: reveals post timestamp distribution and snapshot data
/// to help debug why range sums might all show the same value.
pub async fn get_views_debug(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    // 1. Post timestamp distribution (count per year-month)
    let ts_dist: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT TO_CHAR(timestamp, 'YYYY-MM') AS month, COUNT(*)::bigint
           FROM posts GROUP BY month ORDER BY month"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // 2. Five oldest posts
    let oldest: Vec<(String, String)> =
        sqlx::query_as(r#"SELECT id, timestamp::text FROM posts ORDER BY timestamp LIMIT 5"#)
            .fetch_all(&state.pool)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // 3. Snapshot stats
    let (snap_count, snap_min, snap_max): (i64, Option<String>, Option<String>) = sqlx::query_as(
        r#"SELECT COUNT(*)::bigint,
                  MIN(captured_at)::text,
                  MAX(captured_at)::text
           FROM engagement_snapshots"#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // 4. Delta total vs posts.views total
    let (delta_total,): (i64,) = sqlx::query_as(
        r#"WITH ordered_snapshots AS (
               SELECT es.views,
                      MAX(es.views) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_views
               FROM engagement_snapshots es
           )
           SELECT COALESCE(SUM(GREATEST(views - COALESCE(prev_views, 0), 0)), 0)::bigint
           FROM ordered_snapshots"#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let (posts_total,): (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(views), 0)::bigint FROM posts")
            .fetch_one(&state.pool)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // 5. Sample of effective_date distribution from the delta CTE
    let eff_dist: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at, es.post_id, es.views,
                      MAX(es.views) OVER (PARTITION BY es.post_id ORDER BY es.captured_at ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS prev_views,
                      p.timestamp AS post_timestamp
               FROM engagement_snapshots es
               JOIN posts p ON p.id = es.post_id
           ),
           with_deltas AS (
               SELECT CASE
                          WHEN prev_views IS NULL THEN post_timestamp
                          ELSE captured_at
                      END AS effective_date,
                      GREATEST(views - COALESCE(prev_views, 0), 0) AS view_delta
               FROM ordered_snapshots
           )
           SELECT TO_CHAR(effective_date, 'YYYY-MM') AS month,
                  COUNT(*)::bigint AS num_deltas,
                  COALESCE(SUM(view_delta), 0)::bigint AS total_delta
           FROM with_deltas
           GROUP BY month
           ORDER BY month"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // 6. User-level insights (authoritative total from Threads API)
    let user_level = db::get_user_insights_total(&state.pool).await.unwrap_or(0);

    // 7. Top 10 posts by views
    let top_posts: Vec<(String, i32, Option<String>)> = sqlx::query_as(
        r#"SELECT id, views, LEFT(text, 60) AS preview FROM posts ORDER BY views DESC LIMIT 10"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // 8. Posts with zero views
    let (zero_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*)::bigint FROM posts WHERE views = 0")
            .fetch_one(&state.pool)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let scale = if delta_total > 0 && user_level > delta_total {
        user_level as f64 / delta_total as f64
    } else {
        1.0
    };

    let result = serde_json::json!({
        "post_timestamp_distribution": ts_dist.into_iter().map(|(m, c)| serde_json::json!({"month": m, "count": c})).collect::<Vec<_>>(),
        "oldest_posts": oldest.into_iter().map(|(id, ts)| serde_json::json!({"id": id, "timestamp": ts})).collect::<Vec<_>>(),
        "snapshot_stats": {
            "count": snap_count,
            "earliest": snap_min,
            "latest": snap_max,
        },
        "totals": {
            "delta_based": delta_total,
            "posts_sum": posts_total,
            "user_level_insights": user_level,
            "scaling_factor": format!("{:.2}", scale),
            "gap": user_level - posts_total,
            "gap_pct": format!("{:.1}%", if posts_total > 0 { (user_level - posts_total) as f64 / posts_total as f64 * 100.0 } else { 0.0 }),
        },
        "zero_views_posts": zero_count,
        "top_posts": top_posts.into_iter().map(|(id, views, preview)| serde_json::json!({"id": id, "views": views, "preview": preview})).collect::<Vec<_>>(),
        "effective_date_distribution": eff_dist.into_iter().map(|(m, n, d)| serde_json::json!({"month": m, "num_deltas": n, "total_delta": d})).collect::<Vec<_>>(),
    });

    Ok(Json(result))
}
