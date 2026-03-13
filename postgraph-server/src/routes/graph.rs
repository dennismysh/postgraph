use crate::db;
use crate::state::AppState;
use axum::{Json, extract::State};
use serde::Serialize;

#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub size: f32,
    pub sentiment: Option<f32>,
    pub topics: Vec<String>,
    pub timestamp: Option<String>,
    pub engagement: i32,
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

pub async fn get_graph(
    State(state): State<AppState>,
) -> Result<Json<GraphData>, axum::http::StatusCode> {
    // Run all three queries in parallel
    let (posts_result, edges_result, topics_result) = tokio::join!(
        db::get_posts_for_graph(&state.pool),
        db::get_all_edges(&state.pool),
        sqlx::query_as::<_, (String, String)>(
            "SELECT pt.post_id, t.name FROM post_topics pt JOIN topics t ON pt.topic_id = t.id",
        )
        .fetch_all(&state.pool),
    );

    let posts = posts_result.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let edges = edges_result.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let all_post_topics =
        topics_result.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build a HashMap for O(1) topic lookups instead of O(n) per post
    let mut topic_map: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for (pid, name) in &all_post_topics {
        topic_map
            .entry(pid.as_str())
            .or_default()
            .push(name.as_str());
    }

    let nodes: Vec<GraphNode> = posts
        .iter()
        .filter(|p| p.analyzed_at.is_some())
        .map(|p| {
            let topics: Vec<String> = topic_map
                .get(p.id.as_str())
                .map(|names| names.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default();

            let engagement = (p.likes + p.replies_count + p.reposts + p.quotes) as f32;
            let size = (engagement + 1.0).ln().max(0.0) + 1.0;

            GraphNode {
                id: p.id.clone(),
                label: p.text_preview.clone().unwrap_or_default(),
                size,
                sentiment: p.sentiment,
                topics,
                timestamp: Some(p.timestamp.format("%Y-%m-%d").to_string()),
                engagement: p.likes + p.replies_count + p.reposts + p.quotes,
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
