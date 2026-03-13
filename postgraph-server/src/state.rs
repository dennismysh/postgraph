use crate::mercury::MercuryClient;
use crate::threads::ThreadsClient;
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub threads: Arc<ThreadsClient>,
    pub mercury: Arc<MercuryClient>,
    pub api_key: String,
    pub analysis_running: Arc<AtomicBool>,
    pub analysis_progress: Arc<AtomicU32>,
    pub analysis_total: Arc<AtomicU32>,
    pub sync_running: Arc<AtomicBool>,
    pub sync_message: Arc<RwLock<String>>,
}
