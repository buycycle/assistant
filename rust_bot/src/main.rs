mod assistant;
use assistant::{assistant_chat_handler_form, create_assistant, create_ressources, DB};
use axum::{
    extract::Extension,
    routing::{get_service, post},
    Router,
};
use dotenv::dotenv;
use sqlx::MySqlPool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tower_http::services::ServeDir;
use chrono::prelude::*;
// Define a function to create the Axum app with the database pool and assistant.
async fn app(db_pool: MySqlPool, assistant_id: Arc<RwLock<String>>) -> Router {
    Router::new()
        .route("/assistant", post(assistant_chat_handler_form)) // Updated route
        .nest_service(
            "/", // Serve static files at the root of the domain
            get_service(ServeDir::new("static")),
        )
        .layer(Extension(db_pool))
        .layer(Extension(assistant_id)) // Add the assistant ID as a layer
}
#[tokio::main]
async fn main() {
    env_logger::init();
    dotenv().ok();
    // Create the files for the assistant.
    let mut ressources =
        match create_ressources("context", Vec::new(), "instruction/instruction.txt").await {
            Ok(ressources) => ressources,
            Err(e) => {
                log::error!("Failed to create ressources: {:?}", e);
                std::process::exit(1);
            }
        };
    let now = Utc::now();
    let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
    let assistant_name = format!("Assistant_{}", timestamp);
    let mut assistant = match create_assistant(&assistant_name, "gpt-4-1106-preview", ressources.clone()).await {
            Ok(assistant) => assistant,
            Err(e) => {
                log::error!("Failed to create assistant: {:?}", e);
                std::process::exit(1);
            }
        };
    // Create a connection pool for MySQL to the chatbot database where the messages and chat are saved
    let db = match DB::create_db_pool().await {
        Ok(db) => db,
        Err(e) => {
            log::error!("Failed to create database pool: {:?}", e);
            std::process::exit(1);
        }
    };
    let assistant_id = Arc::new(RwLock::new(assistant.id.clone()));
    // Start the server in a separate async task
    let server_assistant_id = assistant_id.clone();
    let server_db_pool = db.pool.clone();
    tokio::spawn(async move {
        let server = tokio::net::TcpListener::bind(&"0.0.0.0:3000")
            .await
            .expect("Failed to bind server to address");
        let router = app(server_db_pool, server_assistant_id).await; // Pass the assistant ID to the app
        axum::serve(server, router.into_make_service())
            .await
            .expect("Failed to start server");
    });
    // Start a loop that runs every 24 hours to create a new resource and assistant
    loop {
        // Wait for 24 hours
        sleep(Duration::from_secs(24 * 3600)).await;
        // Attempt to create new resources and assistant
        let now = Utc::now();
        let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
        let assistant_name = format!("Assistant_{}", timestamp);
        match create_ressources("context", Vec::new(), "instruction/instruction.txt").await {
            Ok(new_ressources) => {
                match create_assistant(&assistant_name, "gpt-4-1106-preview", new_ressources.clone())
                    .await
                {
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
