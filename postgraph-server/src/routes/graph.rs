use axum::{extract::State, Json};
use serde::Serialize;
use crate::db;
use crate::state::AppState;

#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub size: f32,
    pub sentiment: Option<f32>,
    pub topics: Vec<String>,
}

#[derive(Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub weight: f32,
    pub edge_type: String,
}

#[derive(Serialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub async fn get_graph(State(state): State<AppState>) -> Result<Json<GraphData>, axum::http::StatusCode> {
    let posts = db::get_all_posts(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let edges = db::get_all_edges(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch topics for each post
    let all_post_topics: Vec<(String, String)> = sqlx::query_as::<_, (String, String)>(
        "SELECT pt.post_id, t.name FROM post_topics pt JOIN topics t ON pt.topic_id = t.id",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let nodes: Vec<GraphNode> = posts
        .iter()
        .filter(|p| p.analyzed_at.is_some())
        .map(|p| {
            let topics: Vec<String> = all_post_topics
                .iter()
                .filter(|(pid, _)| pid == &p.id)
                .map(|(_, name)| name.clone())
                .collect();

            let engagement = (p.likes + p.replies_count + p.reposts + p.quotes) as f32;
            let size = (engagement + 1.0).ln().max(0.0) + 1.0;

            GraphNode {
                id: p.id.clone(),
                label: p.text.as_deref().unwrap_or("").chars().take(80).collect(),
                size,
                sentiment: p.sentiment,
                topics,
            }
        })
        .collect();

    let graph_edges: Vec<GraphEdge> = edges
        .iter()
        .map(|e| GraphEdge {
            source: e.source_post_id.clone(),
            target: e.target_post_id.clone(),
            weight: e.weight,
            edge_type: e.edge_type.clone(),
        })
        .collect();

    Ok(Json(GraphData {
        nodes,
        edges: graph_edges,
    }))
}
