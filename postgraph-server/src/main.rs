mod db;
mod types;

use axum::{Router, routing::get};
use shuttle_axum::ShuttleAxum;
use sqlx::PgPool;

async fn health() -> &'static str {
    "ok"
}

#[shuttle_runtime::main]
async fn main(
    #[shuttle_shared_db::Postgres] pool: PgPool,
) -> ShuttleAxum {
    sqlx::migrate!().run(&pool).await.expect("migrations failed");

    let router = Router::new()
        .route("/health", get(health))
        .with_state(pool);

    Ok(router.into())
}
