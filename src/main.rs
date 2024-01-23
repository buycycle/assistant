use assistant::scrape_context;
use axum::{extract::Extension, routing::post, Router};
use chat_handlers::{chat_handler, create_db_pool};
use dotenv::dotenv;
use sqlx::SqlitePool;
mod assistant;
mod chat_handlers;
// Define a function to create the Axum app with the database pool.
async fn app(db_pool: SqlitePool) -> Router {
    Router::new()
        .route("/chat", post(chat_handler)) // Updated route
        .layer(Extension(db_pool))
}
#[tokio::main]
async fn main() {
    env_logger::init();
    dotenv().ok();

    let folder_path = "context";
    let urls_to_scrape = vec![
        "https://buycycle.zendesk.com/hc/en-us".to_string(),
        // Add more URLs as needed.
    ];
    // Run the scrape_context function.
    match scrape_context(folder_path, urls_to_scrape).await {
        Ok(()) => println!("Scraping completed successfully."),
        Err(e) => eprintln!("Scraping failed: {}", e),
    }

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
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
