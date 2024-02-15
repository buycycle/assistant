mod assistant;
use assistant::{assistant_chat_handler, create_assistant, create_ressources, DB};
use axum::{extract::Extension, routing::post, Router};
use dotenv::dotenv;
use sqlx::SqlitePool;
// Define a function to create the Axum app with the database pool and assistant.
async fn app(db_pool: SqlitePool, assistant_id: String) -> Router {
    Router::new()
        .route("/assistant", post(assistant_chat_handler)) // Updated route
        .layer(Extension(db_pool))
        .layer(Extension(assistant_id)) // Add the assistant ID as a layer
}
#[tokio::main]
async fn main() {
    env_logger::init();
    dotenv().ok();
    // Create the files for the assistant.
    let ressources = match create_ressources("context",
                                        Vec::new(),
                                        "data/instructions.txt",
                                        ).await {
        Ok(ressources) => ressources,
        Err(e) => {
            log::error!("Failed to create ressoures: {:?}", e);
            std::process::exit(1);
        }
    };
    // Create an assistant outside of the main function.
    let assistant = match create_assistant(
        "My Assistant",
        "gpt-4-turbo-preview",
        ressources,

    )
    .await
    {
        Ok(assistant) => assistant,
        Err(e) => {
            log::error!("Failed to create assistant: {:?}", e);
            std::process::exit(1);
        }
    };
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = match DB::create_db_pool(&database_url).await {
        Ok(db) => db,
        Err(e) => {
            log::error!("Failed to create database pool: {:?}", e);
            std::process::exit(1);
        }
    };
    let db_pool = db.pool; // Extract the SqlitePool from the DB struct
                           // Run database migrations here if necessary
                           // sqlx::migrate!("./migrations").run(&db_pool).await.expect("Failed to run database migrations");
                           // Bind the server to an address and start it.
    let server = tokio::net::TcpListener::bind(&"0.0.0.0:3000")
        .await
        .expect("Failed to bind server to address");
    let router = app(db_pool, assistant.id).await; // Pass the assistant ID to the app
    axum::serve(server, router.into_make_service())
        .await
        .expect("Failed to start server");
}
