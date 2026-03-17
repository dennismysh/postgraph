use crate::state::AppState;
use axum::{Json, extract::Query, extract::State};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct SubjectNode {
    pub id: String,
    pub label: String,
    pub post_count: i64,
    pub avg_engagement: f64,
    pub color: String,
}

#[derive(Serialize)]
pub struct SubjectGraphEdge {
    pub source: String,
    pub target: String,
    pub weight: f32,
    pub shared_intents: i32,
}

#[derive(Serialize)]
pub struct IntentInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    pub post_count: i64,
}

#[derive(Serialize)]
pub struct SubjectGraphData {
    pub nodes: Vec<SubjectNode>,
    pub edges: Vec<SubjectGraphEdge>,
    pub intents: Vec<IntentInfo>,
}

#[derive(Deserialize)]
pub struct GraphQuery {
    pub intent: Option<String>,
    pub days: Option<i32>,
}

pub async fn get_graph(
    State(state): State<AppState>,
    Query(query): Query<GraphQuery>,
) -> Result<Json<SubjectGraphData>, axum::http::StatusCode> {
    let cutoff: Option<DateTime<Utc>> = query
        .days
        .map(|d| Utc::now() - Duration::days(i64::from(d)));

    let nodes = if let Some(ref intent_name) = query.intent {
        let intent_row: Option<(uuid::Uuid,)> =
            sqlx::query_as("SELECT id FROM intents WHERE name = $1")
                .bind(intent_name)
                .fetch_optional(&state.pool)
                .await
                .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some((intent_id,)) = intent_row {
            let rows: Vec<(uuid::Uuid, String, i64, f64, String)> = sqlx::query_as(
                r#"SELECT s.id, s.name,
                          COUNT(p.id)::bigint AS post_count,
                          COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes), 0)::float8 AS avg_engagement,
                          s.color
                   FROM subjects s
                   LEFT JOIN posts p ON p.subject_id = s.id AND p.analyzed_at IS NOT NULL
                     AND p.intent_id = $1
                     AND ($2::timestamptz IS NULL OR p.timestamp >= $2)
                   GROUP BY s.id, s.name, s.color
                   ORDER BY post_count DESC"#,
            )
            .bind(intent_id)
            .bind(cutoff)
            .fetch_all(&state.pool)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

            rows.into_iter()
                .map(
                    |(id, name, post_count, avg_engagement, color)| SubjectNode {
                        id: id.to_string(),
                        label: name,
                        post_count,
                        avg_engagement,
                        color,
                    },
                )
                .collect()
        } else {
            vec![]
        }
    } else {
        let rows: Vec<(uuid::Uuid, String, i64, f64, String)> = sqlx::query_as(
            r#"SELECT s.id, s.name,
                      COUNT(p.id)::bigint AS post_count,
                      COALESCE(AVG(p.likes + p.replies_count + p.reposts + p.quotes), 0)::float8 AS avg_engagement,
                      s.color
               FROM subjects s
               LEFT JOIN posts p ON p.subject_id = s.id AND p.analyzed_at IS NOT NULL
                 AND ($1::timestamptz IS NULL OR p.timestamp >= $1)
               GROUP BY s.id, s.name, s.color
               ORDER BY post_count DESC"#,
        )
        .bind(cutoff)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

        rows.into_iter()
            .map(
                |(id, name, post_count, avg_engagement, color)| SubjectNode {
                    id: id.to_string(),
                    label: name,
                    post_count,
                    avg_engagement,
                    color,
                },
            )
            .collect()
    };

    let edge_rows: Vec<(uuid::Uuid, uuid::Uuid, f32, i32)> = sqlx::query_as(
        "SELECT source_subject_id, target_subject_id, weight, shared_intents FROM subject_edges",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let edges: Vec<SubjectGraphEdge> = edge_rows
        .into_iter()
        .map(
            |(source, target, weight, shared_intents)| SubjectGraphEdge {
                source: source.to_string(),
                target: target.to_string(),
                weight,
                shared_intents,
            },
        )
        .collect();

    let intent_rows: Vec<(uuid::Uuid, String, String, i64)> = sqlx::query_as(
        r#"SELECT i.id, i.name, i.color, COUNT(p.id)::bigint AS post_count
           FROM intents i
           LEFT JOIN posts p ON p.intent_id = i.id AND p.analyzed_at IS NOT NULL
             AND ($1::timestamptz IS NULL OR p.timestamp >= $1)
           GROUP BY i.id, i.name, i.color
           ORDER BY post_count DESC"#,
    )
    .bind(cutoff)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let intents: Vec<IntentInfo> = intent_rows
        .into_iter()
        .map(|(id, name, color, post_count)| IntentInfo {
            id: id.to_string(),
            name,
            color,
            post_count,
        })
        .collect();

    Ok(Json(SubjectGraphData {
        nodes,
        edges,
        intents,
    }))
}
