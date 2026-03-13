use sqlx::PgPool;
use tracing::{info, warn};

use crate::db;
use crate::error::AppError;
use crate::mercury::MercuryClient;

const ANALYSIS_BATCH_SIZE: i64 = 15;

pub async fn run_analysis(pool: &PgPool, mercury: &MercuryClient) -> Result<u32, AppError> {
    let unanalyzed = db::get_unanalyzed_posts(pool, ANALYSIS_BATCH_SIZE).await?;
    if unanalyzed.is_empty() {
        return Ok(0);
    }

    let existing_topics = db::get_all_topics(pool).await?;
    let topic_names: Vec<String> = existing_topics.iter().map(|t| t.name.clone()).collect();

    let posts_for_llm: Vec<(String, String)> = unanalyzed
        .iter()
        .filter_map(|p| p.text.as_ref().map(|text| (p.id.clone(), text.clone())))
        .collect();

    if posts_for_llm.is_empty() {
        // All posts are media-only with no text; mark them analyzed with neutral sentiment
        for post in &unanalyzed {
            db::mark_post_analyzed(pool, &post.id, 0.0).await?;
        }
        return Ok(unanalyzed.len() as u32);
    }

    let result = match mercury.analyze_posts(&posts_for_llm, &topic_names).await {
        Ok(r) => r,
        Err(e) => {
            warn!("Mercury analysis failed: {e}");
            return Err(e);
        }
    };

    let mut analyzed_count: u32 = 0;

    for analyzed in &result.posts {
        // Upsert topics and create post_topics links
        for topic_assignment in &analyzed.topics {
            let topic =
                db::upsert_topic(pool, &topic_assignment.name, &topic_assignment.description)
                    .await?;
            db::upsert_post_topic(pool, &analyzed.post_id, topic.id, topic_assignment.weight)
                .await?;
        }

        db::mark_post_analyzed(pool, &analyzed.post_id, analyzed.sentiment).await?;
        analyzed_count += 1;
    }

    info!("Analyzed {} posts", analyzed_count);
    Ok(analyzed_count)
}
