use crate::db;
use crate::state::AppState;
use axum::{Json, extract::Path, extract::Query, extract::State};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct SubjectPost {
    pub id: String,
    pub text: Option<String>,
    pub intent: String,
    pub engagement: i64,
    pub views: i32,
    pub timestamp: String,
}

#[derive(Serialize)]
pub struct SubjectPostsResponse {
    pub subject: String,
    pub posts: Vec<SubjectPost>,
}

#[derive(Deserialize)]
pub struct SubjectPostsQuery {
    pub intent: Option<String>,
}

pub async fn get_subject_posts(
    State(state): State<AppState>,
    Path(subject_id): Path<uuid::Uuid>,
    Query(query): Query<SubjectPostsQuery>,
) -> Result<Json<SubjectPostsResponse>, axum::http::StatusCode> {
    let subjects = db::get_all_subjects(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let subject = subjects
        .iter()
        .find(|s| s.id == subject_id)
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    let intent_id = if let Some(ref intent_name) = query.intent {
        let intents = db::get_all_intents(&state.pool)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
        intents
            .iter()
            .find(|i| i.name == *intent_name)
            .map(|i| i.id)
    } else {
        None
    };

    let posts = db::get_posts_by_subject(&state.pool, subject_id, intent_id)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let all_intents = db::get_all_intents(&state.pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let response_posts: Vec<SubjectPost> = posts
        .into_iter()
        .map(|p| {
            let intent_name = p
                .intent_id
                .and_then(|iid| all_intents.iter().find(|i| i.id == iid))
                .map(|i| i.name.clone())
                .unwrap_or_default();
            SubjectPost {
                id: p.id,
                text: p.text,
                intent: intent_name,
                engagement: (p.likes + p.replies_count + p.reposts + p.quotes) as i64,
                views: p.views,
                timestamp: p.timestamp.to_rfc3339(),
            }
        })
        .collect();

    Ok(Json(SubjectPostsResponse {
        subject: subject.name.clone(),
        posts: response_posts,
    }))
}
