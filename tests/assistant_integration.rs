use sqlx::{Executor, SqlitePool};
use std::net::SocketAddr;
use tokio::sync::oneshot;
use rust_bot::assistant::{assistant_chat_handler, create_assistant, teardown_assistant, create_files, DB}; // Replace `your_crate` with the actual crate name
use axum::{
    extract::Extension,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
    routing::post,
    Router,
};

use tower::ServiceExt; // Import the `ServiceExt` trait
// Define a function to create the Axum app with the database pool and assistant.
async fn app(db_pool: SqlitePool, assistant_id: String) -> Router {
    Router::new()
        .route("/assistant", post(assistant_chat_handler)) // Updated route
        .layer(Extension(db_pool))
        .layer(Extension(assistant_id)) // Add the assistant ID as a layer
}
#[tokio::test]
async fn test_chat_endpoint() {
    // Set up a temporary in-memory SQLite database for testing
    let db_pool = SqlitePool::connect(":memory:").await.unwrap();
    // Run database migrations here if necessary
    // sqlx::migrate!("./migrations").run(&db_pool).await.unwrap();
    let files = match create_files(
        "context",
        Vec::new(),
    )
    .await
    {
        Ok(files) => files,
        Err(e) => {
            eprintln!("Failed to create files: {:?}", e);
            return;
        }
    };
    // Create an assistant for testing
    let assistant = create_assistant(
        "Test Assistant",
        "gpt-4",
        "Your instructions here",
        files,
    )
    .await
    .unwrap();
    // Create the router
    let router = app(db_pool.clone(), assistant.id).await;
    // Create a mock request
    let request = Request::builder()
        .uri("/assistant")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "user_id": "user_123",
                "message": "Can you help me?"
            })
            .to_string(),
        ))
        .unwrap();
    // Call the service directly without a server
    let response = router.oneshot(request).await.unwrap();
    // Check the response
    assert_eq!(response.status(), StatusCode::OK);
    // You can also deserialize the response body if needed
    // ...
    // Clean up files and assistant
    teardown_assistant(assistant).await.unwrap();
    files.delete().await.unwrap();
}
