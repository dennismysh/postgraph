use crate::db;
use crate::state::AppState;
use axum::{Json, extract::State};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Clone)]
pub struct NodeCategory {
    pub name: String,
    pub color: String,
}

#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub size: f32,
    pub sentiment: Option<f32>,
    pub topics: Vec<String>,
    pub timestamp: Option<String>,
    pub engagement: i32,
    pub category: Option<NodeCategory>,
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

#[derive(serde::Deserialize)]
pub struct GraphQuery {
    pub category: Option<String>,
}

#[derive(Serialize)]
pub struct TagGraphNode {
    pub id: String,
    pub label: String,
    pub post_count: i32,
    pub total_engagement: i64,
    pub post_ids: Vec<String>,
    pub category_id: Option<String>,
    pub category_name: Option<String>,
    pub category_color: Option<String>,
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
    axum::extract::Query(query): axum::extract::Query<GraphQuery>,
) -> Result<Json<GraphData>, axum::http::StatusCode> {
    // Run all three queries in parallel
    let (posts_result, edges_result, topics_result) = tokio::join!(
        db::get_posts_for_graph(&state.pool),
        db::get_all_edges(&state.pool),
        sqlx::query_as::<_, (String, String, f32, Option<String>, Option<String>)>(
            "SELECT pt.post_id, t.name, pt.weight, c.name AS cat_name, c.color AS cat_color \
             FROM post_topics pt \
             JOIN topics t ON pt.topic_id = t.id \
             LEFT JOIN categories c ON t.category_id = c.id",
        )
        .fetch_all(&state.pool),
    );

    let posts = posts_result.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let edges = edges_result.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let all_post_topics =
        topics_result.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build a HashMap for O(1) topic lookups instead of O(n) per post
    let mut topic_map: HashMap<String, Vec<String>> = HashMap::new();
    // Build post_id -> cat_name -> (total_weight, color) for dominant category
    let mut category_weights: HashMap<String, HashMap<String, (f32, String)>> = HashMap::new();

    for (pid, name, weight, cat_name, cat_color) in &all_post_topics {
        topic_map
            .entry(pid.clone())
            .or_default()
            .push(name.clone());

        if let (Some(cat), Some(color)) = (cat_name, cat_color) {
            category_weights
                .entry(pid.clone())
                .or_default()
                .entry(cat.clone())
                .and_modify(|(w, _)| *w += weight)
                .or_insert((*weight, color.clone()));
        }
    }

    let nodes: Vec<GraphNode> = posts
        .iter()
        .filter(|p| p.analyzed_at.is_some())
        .filter_map(|p| {
            // Compute dominant category for this post
            let dominant_category = category_weights
                .get(&p.id)
                .and_then(|cats| {
                    cats.iter()
                        .max_by(|a, b| a.1.0.partial_cmp(&b.1.0).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|(name, (_, color))| NodeCategory {
                            name: name.clone(),
                            color: color.clone(),
                        })
                });

            // Apply category filter
            if let Some(ref filter_cat) = query.category {
                match &dominant_category {
                    Some(cat) if &cat.name == filter_cat => {}
                    _ => return None,
                }
            }

            let topics: Vec<String> = topic_map
                .get(&p.id)
                .cloned()
                .unwrap_or_default();

            let engagement = (p.likes + p.replies_count + p.reposts + p.quotes) as f32;
            let size = (engagement + 1.0).ln().max(0.0) + 1.0;

            Some(GraphNode {
                id: p.id.clone(),
                label: p.text_preview.clone().unwrap_or_default(),
                size,
                sentiment: p.sentiment,
                topics,
                timestamp: Some(p.timestamp.format("%Y-%m-%d").to_string()),
                engagement: p.likes + p.replies_count + p.reposts + p.quotes,
                category: dominant_category,
            })
        })
        .collect();

    // Filter edges to only include those where both endpoints survived filtering
    let valid_ids: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();

    let graph_edges: Vec<GraphEdge> = edges
        .iter()
        .filter(|e| valid_ids.contains(e.source_post_id.as_str()) && valid_ids.contains(e.target_post_id.as_str()))
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
    // Query: for each topic, get its post IDs, total engagement, and category info
    let rows = sqlx::query_as::<_, (String, String, String, i64, Option<String>, Option<String>, Option<String>)>(
        r#"SELECT t.id::text, t.name, pt.post_id,
                  COALESCE(p.likes + p.replies_count + p.reposts + p.quotes, 0)::bigint AS engagement,
                  c.id::text AS category_id, c.name AS category_name, c.color AS category_color
           FROM topics t
           JOIN post_topics pt ON pt.topic_id = t.id
           JOIN posts p ON p.id = pt.post_id AND p.analyzed_at IS NOT NULL
           LEFT JOIN categories c ON t.category_id = c.id
           ORDER BY t.name"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build topic -> (name, post_ids, total_engagement, category_id, category_name, category_color)
    let mut topic_data: HashMap<String, (String, Vec<String>, i64, Option<String>, Option<String>, Option<String>)> = HashMap::new();
    // Build post_id -> set of topic_ids (for edge computation)
    let mut post_topics_map: HashMap<String, Vec<String>> = HashMap::new();

    for (topic_id, topic_name, post_id, engagement, category_id, category_name, category_color) in &rows {
        let entry = topic_data
            .entry(topic_id.clone())
            .or_insert_with(|| (topic_name.clone(), Vec::new(), 0, category_id.clone(), category_name.clone(), category_color.clone()));
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
        .map(|(topic_id, (name, post_ids, total_eng, cat_id, cat_name, cat_color))| TagGraphNode {
            id: topic_id.clone(),
            label: name.clone(),
            post_count: post_ids.len() as i32,
            total_engagement: *total_eng,
            post_ids: post_ids.clone(),
            category_id: cat_id.clone(),
            category_name: cat_name.clone(),
            category_color: cat_color.clone(),
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
