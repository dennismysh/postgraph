use crate::mercury::MercuryClient;
use crate::threads::ThreadsClient;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub threads: Arc<ThreadsClient>,
    pub mercury: Arc<MercuryClient>,
    pub api_key: String,
}
