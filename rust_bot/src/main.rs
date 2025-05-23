mod assistant;
use assistant::{assistant_chat_handler_form, create_assistant, create_ressources, DB};
use axum::{
    extract::Extension,
    routing::{get, get_service, post},
    Router,
};
use chrono::prelude::*;
use dotenv::dotenv;
use sqlx::MySqlPool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tower_http::services::ServeDir;
use std::env;
// Define the health check handler
async fn health_check() -> &'static str {
    "OK"
}
// Define a function to create the Axum app with the database pool and assistant.
async fn app(
    db_pool_buycycle: MySqlPool,
    db_pool_log: MySqlPool,
    assistant_id: Arc<RwLock<String>>,
) -> Router {
    Router::new()
        .route("/health", get(health_check)) // Health check route
        .route("/assistant", post(assistant_chat_handler_form)) // Existing route
        .nest_service(
            "/", // Serve static files at the root of the domain
            get_service(ServeDir::new("static")),
        )
        .layer(Extension(db_pool_buycycle))
        .layer(Extension(db_pool_log))
        .layer(Extension(assistant_id)) // Add the assistant ID as a layer
}
#[tokio::main]
async fn main() {
    env_logger::init();
    dotenv().ok();
    // Create DB connection pools for log and buycycle DB
    let database_url_buycycle =
        env::var("DATABASE_URL_LOG").expect("DATABASE_URL must be set");
    // Create a new database connection pool
    let db_pool_buycycle = match DB::create_pool(&database_url_buycycle).await {
        Ok(pool) => pool,
        Err(e) => {
            log::error!("Failed to create database pool buycycle: {:?}", e);
            std::process::exit(1);
        }
    };
    let database_url_log = env::var("DATABASE_URL_LOG").expect("DATABASE_URL must be set");
    // Create a new database connection pool
    let db_pool_log = match DB::create_pool(&database_url_log).await {
        Ok(pool) => pool,
        Err(e) => {
            log::error!("Failed to create database pool log: {:?}", e);
            std::process::exit(1);
        }
    };
    // Create the files for the assistant.
    let mut ressources = match create_ressources(
        db_pool_buycycle.clone(),
        "context/file_search",
        "context/code_interpreter",
        Vec::new(),
        "instruction/instruction.txt",
    )
    .await
    {
        Ok(ressources) => ressources,
        Err(e) => {
            log::error!("Failed to create ressources: {:?}", e);
            std::process::exit(1);
        }
    };
    let now = Utc::now();
    let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
    let assistant_name = format!("Assistant_{}", timestamp);
    let mut assistant = match create_assistant(&assistant_name, "gpt-4o", ressources.clone()).await {
        Ok(assistant) => assistant,
        Err(e) => {
            log::error!("Failed to create assistant: {:?}", e);
            std::process::exit(1);
        }
    };
    let assistant_id = Arc::new(RwLock::new(assistant.id.clone()));
    // Start the server in a separate async task
    let server_assistant_id = Arc::clone(&assistant_id);
    tokio::spawn({
        let db_pool_buycycle = db_pool_buycycle.clone();
        let db_pool_log = db_pool_log.clone();
        async move {
            let server = tokio::net::TcpListener::bind(&"0.0.0.0:3000")
                .await
                .expect("Failed to bind server to address");
            let router = app(db_pool_buycycle, db_pool_log, server_assistant_id).await; // Pass the assistant ID to the app
            axum::serve(server, router.into_make_service())
                .await
                .expect("Failed to start server");
        }
    });
    // Start a loop that runs every 24 hours to create a new resource and assistant
    loop {
        // Wait for 24 hours
        sleep(Duration::from_secs(24 * 3600)).await;
        // Attempt to create new resources and assistant
        let now = Utc::now();
        let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
        let assistant_name = format!("Assistant_{}", timestamp);
        match create_ressources(
            db_pool_buycycle.clone(),
            "context/file_search",
            "context/code_interpreter",
            Vec::new(),
            "instruction/instruction.txt",
        )
        .await
        {
            Ok(new_ressources) => {
                match create_assistant(&assistant_name, "gpt-4o", new_ressources.clone()).await {
                    Ok(new_assistant) => {
                        // Update the assistant ID in the shared state
                        let mut assistant_id_guard = assistant_id.write().await;
                        *assistant_id_guard = new_assistant.id.clone();
                        // Delete the old assistant and resources after the last request with the old assistant_id is finished
                        tokio::spawn(async move {
                            assistant
                                .delete()
                                .await
                                .expect("Failed to delete old assistant");
                            ressources
                                .delete()
                                .await
                                .expect("Failed to delete old resources");
                        });
                        // Update the local variables to the new ones
                        assistant = new_assistant;
                        ressources = new_ressources;
                    }
                    Err(e) => {
                        log::error!("Failed to create new assistant: {:?}", e);
                        // Handle error (e.g., retry later, log error, etc.)
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to create new resources: {:?}", e);
                // Handle error (e.g., retry later, log error, etc.)
            }
        }
    }
}
