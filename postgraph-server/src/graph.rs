use crate::db;
use crate::error::AppError;
use sqlx::PgPool;
use tracing::info;

/// Compute edges between subjects based on shared intent patterns.
/// Two subjects are connected if they share at least 2 intent types.
/// Weight = shared_intents / total_distinct_intents.
pub async fn compute_subject_edges(pool: &PgPool) -> Result<u32, AppError> {
    db::delete_all_subject_edges(pool).await?;

    let subjects = db::get_all_subjects(pool).await?;
    let total_intents = db::get_all_intents(pool).await?.len() as f32;

    if total_intents == 0.0 {
        return Ok(0);
    }

    let mut edge_count: u32 = 0;

    for i in 0..subjects.len() {
        for j in (i + 1)..subjects.len() {
            let shared: (i64,) = sqlx::query_as(
                r#"SELECT COUNT(*)::bigint FROM (
                    SELECT DISTINCT intent_id FROM posts WHERE subject_id = $1 AND intent_id IS NOT NULL
                    INTERSECT
                    SELECT DISTINCT intent_id FROM posts WHERE subject_id = $2 AND intent_id IS NOT NULL
                ) shared"#,
            )
            .bind(subjects[i].id)
            .bind(subjects[j].id)
            .fetch_one(pool)
            .await?;

            let shared_count = shared.0 as i32;
            if shared_count >= 2 {
                let weight = shared_count as f32 / total_intents;
                db::upsert_subject_edge(pool, subjects[i].id, subjects[j].id, weight, shared_count)
                    .await?;
                edge_count += 1;
            }
        }
    }

    info!(
        "Computed {edge_count} subject edges across {} subjects",
        subjects.len()
    );
    Ok(edge_count)
}
