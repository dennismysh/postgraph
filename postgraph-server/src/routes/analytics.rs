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
                      LAG(es.likes) OVER (PARTITION BY es.post_id ORDER BY es.captured_at) AS prev_likes,
                      LAG(es.replies_count) OVER (PARTITION BY es.post_id ORDER BY es.captured_at) AS prev_replies,
                      LAG(es.reposts) OVER (PARTITION BY es.post_id ORDER BY es.captured_at) AS prev_reposts,
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
    // Use per-post snapshot deltas — the Threads user-level insights API
    // undercounts views (~50% of what the Threads app reports), while
    // summing per-post view deltas matches the app exactly.
    get_views_from_snapshots(&state.pool, &query).await
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

    let sql = format!(
        r#"WITH ordered_snapshots AS (
               SELECT es.captured_at,
                      es.post_id,
                      es.views,
                      LAG(es.views) OVER (PARTITION BY es.post_id ORDER BY es.captured_at) AS prev_views,
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
           SELECT {date_format} AS date,
                  SUM(view_delta)::bigint AS total_views
           FROM with_deltas
           {since_clause}
           GROUP BY {date_expr}
           ORDER BY date"#,
        date_format = date_format,
        since_clause = since_clause,
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
                      LAG(es.likes) OVER (PARTITION BY es.post_id ORDER BY es.captured_at) AS prev_likes,
                      LAG(es.replies_count) OVER (PARTITION BY es.post_id ORDER BY es.captured_at) AS prev_replies,
                      LAG(es.reposts) OVER (PARTITION BY es.post_id ORDER BY es.captured_at) AS prev_reposts,
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

    let (total_views,): (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(views), 0)::bigint FROM posts")
            .fetch_one(&state.pool)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

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
    let ranges: &[(&str, Option<i64>)] = &[
        ("24h", Some(1)),
        ("7d", Some(7)),
        ("14d", Some(14)),
        ("30d", Some(30)),
        ("60d", Some(60)),
        ("90d", Some(90)),
        ("180d", Some(180)),
        ("270d", Some(270)),
        ("365d", Some(365)),
        ("all", None),
    ];

    let mut sums = HashMap::new();

    for &(key, days) in ranges {
        let total: i64 = match days {
            None => {
                let (v,): (i64,) =
                    sqlx::query_as("SELECT COALESCE(SUM(views), 0)::bigint FROM posts")
                        .fetch_one(&state.pool)
                        .await
                        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
                v
            }
            Some(d) => {
                let boundary = chrono::Utc::now() - chrono::Duration::days(d);
                let (v,): (i64,) = sqlx::query_as(
                    r#"SELECT COALESCE(SUM(
                        CASE
                            WHEN p.timestamp >= $1 THEN p.views
                            ELSE GREATEST(p.views - COALESCE(boundary.views, 0), 0)
                        END
                    ), 0)::bigint
                    FROM posts p
                    LEFT JOIN LATERAL (
                        SELECT es.views FROM engagement_snapshots es
                        WHERE es.post_id = p.id AND es.captured_at <= $1
                        ORDER BY es.captured_at DESC LIMIT 1
                    ) boundary ON TRUE"#,
                )
                .bind(boundary)
                .fetch_one(&state.pool)
                .await
                .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
                v
            }
        };
        sums.insert(key.to_string(), total);
    }

    Ok(Json(ViewsRangeSums { sums }))
}
