use sqlx::PgPool;
use std::sync::Arc;
use crate::mercury::MercuryClient;
use crate::threads::ThreadsClient;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub threads: Arc<ThreadsClient>,
    pub mercury: Arc<MercuryClient>,
    pub api_key: String,
}
