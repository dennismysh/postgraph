use crate::db;
use crate::state::AppState;
use axum::{Json, extract::Path, extract::Query, extract::State};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct AnalyticsData {
    pub total_posts: usize,
    pub analyzed_posts: usize,
    pub total_subjects: usize,
    pub total_intents: usize,
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
    // For hourly grouping, use snapshot deltas (Threads API only provides daily)
    if query.grouping.as_deref() == Some("hourly") {
        return get_views_from_snapshots(&state.pool, &query).await;
    }

    // For daily data, use Threads user-level insights API (authoritative source)
    let until = chrono::Utc::now().timestamp();
    let since = if let Some(ref since_str) = query.since {
        chrono::DateTime::parse_from_rfc3339(since_str)
            .map(|dt| dt.timestamp())
            .unwrap_or(1712991600)
    } else {
        // Threads API earliest allowed: April 13, 2024
        1712991600
    };

    match state.threads.get_user_views(since, until).await {
        Ok(rows) => {
            let points: Vec<ViewsPoint> = rows
                .into_iter()
                .map(|(date, views)| ViewsPoint { date, views })
                .collect();
            Ok(Json(points))
        }
        Err(e) => {
            tracing::warn!("User insights API failed, falling back to snapshots: {e}");
            get_views_from_snapshots(&state.pool, &query).await
        }
    }
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
               WHERE es.views > 0
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

    Ok(Json(AnalyticsData {
        total_posts: posts.len(),
        analyzed_posts: analyzed_count,
        total_subjects: subjects.len(),
        total_intents: intents.len(),
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
