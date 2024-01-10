use axum::{
    extract::{Extension, Path},
    routing::post,
    Router,
};
use dotenv::dotenv;
use sqlx::SqlitePool;
use chat_handlers::chat_handler;
mod chat_handlers;
// Define a function to create the Axum app with the database pool.
async fn app(db_pool: SqlitePool) -> Router {
    Router::new()
        .route("/chat/:chat_id", post(chat_handler))
        .layer(Extension(db_pool))
}
#[tokio::main]
async fn main() {
    dotenv().ok();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    // Create a database pool.
    let db_pool = SqlitePool::connect(&database_url)
        .await
        .expect("Failed to create database pool");
    // Run database migrations here if necessary
    // sqlx::migrate!("./migrations").run(&db_pool).await.expect("Failed to run database migrations");
    // Bind the server to an address and start it.
    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app(db_pool).into_make_service())
        .await
        .unwrap();
}
