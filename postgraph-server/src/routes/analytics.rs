use crate::db;
use crate::state::AppState;
use axum::{Json, extract::Query, extract::State};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct AnalyticsData {
    pub total_posts: usize,
    pub analyzed_posts: usize,
    pub total_topics: usize,
    pub topics: Vec<TopicSummary>,
    pub engagement_over_time: Vec<EngagementPoint>,
}

#[derive(Serialize)]
pub struct TopicSummary {
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

pub async fn get_views(
    State(state): State<AppState>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Vec<ViewsPoint>>, axum::http::StatusCode> {
    // Compute view *deltas* between consecutive engagement snapshots so that
    // views are attributed to when they were received, not when the post was
    // published.  The CTE computes LAG over ALL snapshots (no date filter) so
    // that the first delta inside the requested period is accurate, then the
    // outer query filters by captured_at.
    let since_clause = if let Some(ref since) = query.since {
        format!(
            "WHERE captured_at >= '{}'::timestamptz",
            since.replace('\'', "")
        )
    } else {
        String::new()
    };

    let is_hourly = query.grouping.as_deref() == Some("hourly");
    let (date_expr, date_format) = if is_hourly {
        (
            "DATE_TRUNC('hour', captured_at)",
            "TO_CHAR(DATE_TRUNC('hour', captured_at), 'YYYY-MM-DD HH24:00')",
        )
    } else {
        ("DATE(captured_at)", "DATE(captured_at)::text")
    };

    let sql = format!(
        r#"WITH ordered_snapshots AS (
               SELECT captured_at,
                      views,
                      LAG(views) OVER (PARTITION BY post_id ORDER BY captured_at) AS prev_views
               FROM engagement_snapshots
               WHERE views IS NOT NULL
           )
           SELECT {date_format} AS date,
                  SUM(GREATEST(views - COALESCE(prev_views, 0), 0))::bigint AS total_views
           FROM ordered_snapshots
           {since_clause}
           GROUP BY {date_expr}
           ORDER BY date"#,
        date_format = date_format,
        since_clause = since_clause,
        date_expr = date_expr,
    );

    let rows: Vec<(String, i64)> = sqlx::query_as(&sql)
        .fetch_all(&state.pool)
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

    let topics = db::get_all_topics(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let topic_summaries: Vec<TopicSummary> = sqlx::query_as::<_, (String, i64, f64)>(
        r#"SELECT t.name, COUNT(pt.post_id) as post_count,
           AVG(p.likes + p.replies_count + p.reposts + p.quotes)::float8 as avg_engagement
           FROM topics t
           JOIN post_topics pt ON t.id = pt.topic_id
           JOIN posts p ON pt.post_id = p.id
           GROUP BY t.name
           ORDER BY post_count DESC"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(|(name, count, avg)| TopicSummary {
        name,
        post_count: count,
        avg_engagement: avg,
    })
    .collect();

    let engagement_over_time: Vec<EngagementPoint> = sqlx::query_as::<_, (String, i64, i64, i64)>(
        r#"SELECT DATE(captured_at)::text as date,
           SUM(likes)::bigint, SUM(replies_count)::bigint, SUM(reposts)::bigint
           FROM engagement_snapshots
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

    Ok(Json(AnalyticsData {
        total_posts: posts.len(),
        analyzed_posts: analyzed_count,
        total_topics: topics.len(),
        topics: topic_summaries,
        engagement_over_time,
    }))
}
