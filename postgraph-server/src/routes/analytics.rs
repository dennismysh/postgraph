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
    // Show views distributed by post publication date (posts.timestamp),
    // using the latest snapshot views for each post.
    let since_clause = if let Some(ref since) = query.since {
        format!(
            "WHERE p.timestamp >= '{}'::timestamptz",
            since.replace('\'', "")
        )
    } else {
        String::new()
    };

    let is_hourly = query.grouping.as_deref() == Some("hourly");
    let (date_expr, date_format) = if is_hourly {
        (
            "DATE_TRUNC('hour', p.timestamp)",
            "TO_CHAR(DATE_TRUNC('hour', p.timestamp), 'YYYY-MM-DD HH24:00')",
        )
    } else {
        ("DATE(p.timestamp)", "DATE(p.timestamp)::text")
    };

    let sql = format!(
        r#"SELECT {date_format} as date, SUM(COALESCE(latest.views, p.views))::bigint as total_views
           FROM posts p
           LEFT JOIN LATERAL (
               SELECT views FROM engagement_snapshots es
               WHERE es.post_id = p.id AND es.views IS NOT NULL
               ORDER BY es.captured_at DESC
               LIMIT 1
           ) latest ON true
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
