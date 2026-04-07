use sqlx::PgPool;
use tracing::{info, warn};

use crate::db;
use crate::error::AppError;
use crate::mercury::MercuryClient;

fn analysis_batch_size() -> i64 {
    std::env::var("ANALYSIS_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16)
}

pub async fn run_analysis(pool: &PgPool, mercury: &MercuryClient) -> Result<u32, AppError> {
    let unanalyzed = db::get_unanalyzed_posts(pool, analysis_batch_size()).await?;
    if unanalyzed.is_empty() {
        return Ok(0);
    }

    let existing_intents = db::get_all_intents(pool).await?;
    let intent_names: Vec<String> = existing_intents.iter().map(|i| i.name.clone()).collect();

    let existing_subjects = db::get_all_subjects(pool).await?;
    let subject_names: Vec<String> = existing_subjects.iter().map(|s| s.name.clone()).collect();

    let posts_for_llm: Vec<(String, String)> = unanalyzed
        .iter()
        .filter_map(|p| p.text.as_ref().map(|text| (p.id.clone(), text.clone())))
        .collect();

    if posts_for_llm.is_empty() {
        // All posts are media-only with no text; mark them analyzed with neutral sentiment
        for post in &unanalyzed {
            db::mark_post_analyzed(pool, &post.id, 0.0, "reflective").await?;
        }
        return Ok(unanalyzed.len() as u32);
    }

    let results = match mercury
        .analyze_posts(&posts_for_llm, &intent_names, &subject_names)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!("Mercury analysis failed: {e}");
            return Err(e);
        }
    };

    let mut analyzed_count: u32 = 0;

    for result in &results {
        let intent_count = db::get_all_intents(pool).await?.len();
        let intent =
            db::upsert_intent(pool, &result.intent, "", db::next_color(intent_count)).await?;

        let subject_count = db::get_all_subjects(pool).await?.len();
        let subject =
            db::upsert_subject(pool, &result.subject, "", db::next_color(subject_count)).await?;

        db::set_post_intent_subject(pool, &result.post_id, intent.id, subject.id).await?;
        db::mark_post_analyzed(pool, &result.post_id, result.sentiment, &result.emotion).await?;
        analyzed_count += 1;
    }

    info!("Analyzed {} posts", analyzed_count);
    Ok(analyzed_count)
}
