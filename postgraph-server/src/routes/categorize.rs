use axum::{Json, extract::State};
use serde::Serialize;
use std::sync::atomic::Ordering;
use tracing::info;

use crate::db;
use crate::state::AppState;

#[derive(Serialize)]
pub struct CategorizeStartResult {
    pub started: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct CategorizeStatus {
    pub running: bool,
    pub progress: u32,
    pub total: u32,
}

#[derive(Serialize)]
pub struct CategoryWithTopics {
    pub id: uuid::Uuid,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub topics: Vec<String>,
}

#[derive(Serialize)]
pub struct ListCategoriesResponse {
    pub categories: Vec<CategoryWithTopics>,
}

pub async fn start_categorize(State(state): State<AppState>) -> Json<CategorizeStartResult> {
    // Check if analysis is running
    if state.analysis_running.load(Ordering::SeqCst) {
        return Json(CategorizeStartResult {
            started: false,
            message: "Analysis in progress...".to_string(),
        });
    }

    // Check if categorize already running
    if state.categorize_running.swap(true, Ordering::SeqCst) {
        return Json(CategorizeStartResult {
            started: false,
            message: "Categorization already in progress".to_string(),
        });
    }

    // Count topics
    let topic_count: i64 = match sqlx::query_scalar("SELECT COUNT(*) FROM topics")
        .fetch_one(&state.pool)
        .await
    {
        Ok(n) => n,
        Err(e) => {
            state.categorize_running.store(false, Ordering::SeqCst);
            tracing::error!("Failed to count topics: {e}");
            return Json(CategorizeStartResult {
                started: false,
                message: format!("Failed to count topics: {e}"),
            });
        }
    };

    state.categorize_progress.store(0, Ordering::SeqCst);
    state
        .categorize_total
        .store(topic_count as u32, Ordering::SeqCst);

    info!("Starting background categorization of {topic_count} topics");

    let bg_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = run_full_categorization(&bg_state).await {
            tracing::error!("Categorization failed: {e}");
        }
        bg_state.categorize_running.store(false, Ordering::SeqCst);
        info!("Background categorization task finished");
    });

    Json(CategorizeStartResult {
        started: true,
        message: format!("{topic_count} topics queued for categorization"),
    })
}

pub async fn run_full_categorization(state: &AppState) -> Result<(), crate::error::AppError> {
    // Get all topics from DB
    let topics = db::get_all_topics(&state.pool).await?;
    if topics.is_empty() {
        info!("No topics to categorize");
        return Ok(());
    }

    let topic_pairs: Vec<(String, String)> = topics
        .iter()
        .map(|t| (t.name.clone(), t.description.clone().unwrap_or_default()))
        .collect();

    state
        .categorize_total
        .store(topics.len() as u32, Ordering::SeqCst);

    // Call Mercury to categorize topics
    let categorize_resp = state.mercury.categorize_topics(&topic_pairs).await?;

    // Get existing categories to preserve colors
    let existing_categories = db::get_all_categories(&state.pool).await?;

    let mut color_index = existing_categories.len();

    for category_group in &categorize_resp.categories {
        // Check if this category already exists (preserve its color)
        let existing_color = existing_categories
            .iter()
            .find(|c| c.name == category_group.name)
            .and_then(|c| c.color.clone());

        let color = existing_color.unwrap_or_else(|| {
            let c = db::CATEGORY_COLORS
                .get(color_index % db::CATEGORY_COLORS.len())
                .copied()
                .unwrap_or("#888888");
            color_index += 1;
            c.to_string()
        });

        let upserted = db::upsert_category(
            &state.pool,
            &category_group.name,
            &category_group.description,
            &color,
        )
        .await?;

        for topic_name in &category_group.topics {
            if let Err(e) = db::set_topic_category(&state.pool, topic_name, upserted.id).await {
                tracing::warn!(
                    "Failed to assign topic '{topic_name}' to category '{}': {e}",
                    category_group.name
                );
            }
        }

        state
            .categorize_progress
            .fetch_add(category_group.topics.len() as u32, Ordering::SeqCst);
    }

    // Delete orphaned categories
    let deleted = db::delete_orphaned_categories(&state.pool).await?;
    if deleted > 0 {
        info!("Deleted {deleted} orphaned categories");
    }

    Ok(())
}

pub async fn categorize_status(State(state): State<AppState>) -> Json<CategorizeStatus> {
    Json(CategorizeStatus {
        running: state.categorize_running.load(Ordering::SeqCst),
        progress: state.categorize_progress.load(Ordering::SeqCst),
        total: state.categorize_total.load(Ordering::SeqCst),
    })
}

pub async fn list_categories(State(state): State<AppState>) -> Json<ListCategoriesResponse> {
    let categories_with_topics = match db::get_categories_with_topics(&state.pool).await {
        Ok(data) => data,
        Err(e) => {
            tracing::error!("Failed to load categories: {e}");
            return Json(ListCategoriesResponse { categories: vec![] });
        }
    };

    let categories = categories_with_topics
        .into_iter()
        .map(|(cat, topic_names)| CategoryWithTopics {
            id: cat.id,
            name: cat.name,
            description: cat.description,
            color: cat.color,
            topics: topic_names,
        })
        .collect();

    Json(ListCategoriesResponse { categories })
}
