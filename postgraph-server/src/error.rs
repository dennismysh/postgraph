use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Threads API error: {0}")]
    ThreadsApi(String),

    #[error("Mercury API error: {0}")]
    MercuryApi(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Rate limited, retry after {0}s")]
    RateLimited(u64),
}
