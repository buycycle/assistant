use axum::{
    extract::{Extension, Path},
    routing::post,
    Router,
};
use dotenv::dotenv;
use sqlx::SqlitePool;
use chat_handlers::{chat_handler, create_db_pool};
mod chat_handlers;
// Define a function to create the Axum app with the database pool.
async fn app(db_pool: SqlitePool) -> Router {
    Router::new()
        .route("/chat/:chat_id", post(chat_handler))
        .layer(Extension(db_pool))
}
#[tokio::main]
async fn main() {
    env_logger::init();
    dotenv().ok();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let db_pool = create_db_pool(&database_url)
        .await
        .expect("Failed to create database pool");
    // Run database migrations here if necessary
    // sqlx::migrate!("./migrations").run(&db_pool).await.expect("Failed to run database migrations");
    // Bind the server to an address and start it.
    let server = tokio::net::TcpListener::bind(&"0.0.0.0:3000")
        .await
        .unwrap();
    let router = app(db_pool).await;
    axum::serve(server, router.into_make_service())
        .await
        .unwrap();
}
