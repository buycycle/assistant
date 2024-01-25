use assistant::{create_assistant, AssistantError};
use axum::{extract::Extension, routing::post, Router};
use chat_handlers::{assistant_chat_handler, create_db_pool};
use dotenv::dotenv;
use sqlx::SqlitePool;
mod assistant;
mod chat_handlers;
// Define a function to create the Axum app with the database pool and assistant.
async fn app(db_pool: SqlitePool, assistant_id: String) -> Router {
    Router::new()
        .route("/chat", post(assistant_chat_handler)) // Updated route
        .layer(Extension(db_pool))
        .layer(Extension(assistant_id)) // Add the assistant ID as a layer
}
#[tokio::main]
async fn main() {
    env_logger::init();
    dotenv().ok();
    // Create an assistant outside of the main function.
    let assistant = match create_assistant(
        "My Assistant",
        "gpt-4",
        "Your instructions here",
        "path/to/folder",
    )
    .await
    {
        Ok(assistant) => assistant,
        Err(e) => {
            eprintln!("Failed to create assistant: {:?}", e);
            return;
        }
    };
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
    let router = app(db_pool, assistant.id).await; // Pass the assistant ID to the app
    axum::serve(server, router.into_make_service())
        .await
        .unwrap();
}

