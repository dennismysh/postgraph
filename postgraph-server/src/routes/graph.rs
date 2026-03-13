use crate::db;
use crate::state::AppState;
use axum::{Json, extract::State};
use serde::Serialize;
use std::collections::HashMap;

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

#[derive(Serialize)]
pub struct TagGraphNode {
    pub id: String,
    pub label: String,
    pub post_count: i32,
    pub total_engagement: i64,
    pub post_ids: Vec<String>,
}

#[derive(Serialize)]
pub struct TagGraphEdge {
    pub source: String,
    pub target: String,
    pub weight: f32,
    pub shared_posts: i32,
}

#[derive(Serialize)]
pub struct TagGraphData {
    pub nodes: Vec<TagGraphNode>,
    pub edges: Vec<TagGraphEdge>,
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

pub async fn get_tag_graph(
    State(state): State<AppState>,
) -> Result<Json<TagGraphData>, axum::http::StatusCode> {
    // Query: for each topic, get its post IDs and total engagement
    let rows = sqlx::query_as::<_, (String, String, String, i64)>(
        r#"SELECT t.id::text, t.name, pt.post_id,
                  COALESCE(p.likes + p.replies_count + p.reposts + p.quotes, 0)::bigint AS engagement
           FROM topics t
           JOIN post_topics pt ON pt.topic_id = t.id
           JOIN posts p ON p.id = pt.post_id AND p.analyzed_at IS NOT NULL
           ORDER BY t.name"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build topic -> (name, post_ids, total_engagement)
    let mut topic_data: HashMap<String, (String, Vec<String>, i64)> = HashMap::new();
    // Build post_id -> set of topic_ids (for edge computation)
    let mut post_topics_map: HashMap<String, Vec<String>> = HashMap::new();

    for (topic_id, topic_name, post_id, engagement) in &rows {
        let entry = topic_data
            .entry(topic_id.clone())
            .or_insert_with(|| (topic_name.clone(), Vec::new(), 0));
        entry.1.push(post_id.clone());
        entry.2 += engagement;

        post_topics_map
            .entry(post_id.clone())
            .or_default()
            .push(topic_id.clone());
    }

    // Build nodes
    let nodes: Vec<TagGraphNode> = topic_data
        .iter()
        .map(|(topic_id, (name, post_ids, total_eng))| TagGraphNode {
            id: topic_id.clone(),
            label: name.clone(),
            post_count: post_ids.len() as i32,
            total_engagement: *total_eng,
            post_ids: post_ids.clone(),
        })
        .collect();

    // Build edges: topics that co-occur on the same post
    let mut edge_counts: HashMap<(String, String), i32> = HashMap::new();
    for topic_ids in post_topics_map.values() {
        for i in 0..topic_ids.len() {
            for j in (i + 1)..topic_ids.len() {
                let (a, b) = if topic_ids[i] < topic_ids[j] {
                    (topic_ids[i].clone(), topic_ids[j].clone())
                } else {
                    (topic_ids[j].clone(), topic_ids[i].clone())
                };
                *edge_counts.entry((a, b)).or_insert(0) += 1;
            }
        }
    }

    let edges: Vec<TagGraphEdge> = edge_counts
        .into_iter()
        .map(|((source, target), count)| TagGraphEdge {
            source,
            target,
            weight: count as f32,
            shared_posts: count,
        })
        .collect();

    Ok(Json(TagGraphData { nodes, edges }))
}
