use sqlx::PgPool;
use tracing::info;

use crate::db;
use crate::error::AppError;
use crate::types::PostEdge;

const EDGE_WEIGHT_THRESHOLD: f32 = 0.1;

pub async fn compute_edges_for_post(pool: &PgPool, post_id: &str) -> Result<u32, AppError> {
    // Find all posts that share topics with this post via SQL
    let shared_topic_edges: Vec<(String, f32)> = sqlx::query_as::<_, (String, f32)>(
        r#"SELECT pt2.post_id, SUM(pt1.weight * pt2.weight) as edge_weight
           FROM post_topics pt1
           JOIN post_topics pt2 ON pt1.topic_id = pt2.topic_id AND pt1.post_id != pt2.post_id
           WHERE pt1.post_id = $1
           GROUP BY pt2.post_id
           HAVING SUM(pt1.weight * pt2.weight) >= $2"#,
    )
    .bind(post_id)
    .bind(EDGE_WEIGHT_THRESHOLD)
    .fetch_all(pool)
    .await?;

    let mut edge_count: u32 = 0;

    for (target_id, weight) in &shared_topic_edges {
        let edge = PostEdge {
            source_post_id: post_id.to_string(),
            target_post_id: target_id.clone(),
            edge_type: "topic_overlap".to_string(),
            weight: *weight,
        };
        db::upsert_edge(pool, &edge).await?;
        edge_count += 1;
    }

    info!("Computed {} edges for post {}", edge_count, post_id);
    Ok(edge_count)
}

pub async fn compute_edges_for_recent(pool: &PgPool) -> Result<u32, AppError> {
    // Find posts that were recently analyzed but may not have edges yet
    let recently_analyzed: Vec<String> = sqlx::query_scalar::<_, String>(
        r#"SELECT p.id FROM posts p
           WHERE p.analyzed_at IS NOT NULL
           AND NOT EXISTS (
               SELECT 1 FROM post_edges pe WHERE pe.source_post_id = p.id
           )
           AND EXISTS (
               SELECT 1 FROM post_topics pt WHERE pt.post_id = p.id
           )
           LIMIT 50"#,
    )
    .fetch_all(pool)
    .await?;

    let mut total_edges: u32 = 0;
    for post_id in &recently_analyzed {
        total_edges += compute_edges_for_post(pool, post_id).await?;
    }

    Ok(total_edges)
}
