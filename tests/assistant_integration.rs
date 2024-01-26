use axum::{routing::post, Router};
use rust_bot::assistant::{assistant_chat_handler, create_assistant, DB};
use sqlx::{Executor, SqlitePool};
use std::net::SocketAddr;
use tokio::sync::oneshot;
// Define a function to create the Axum app with the database pool and assistant.
async fn app(db_pool: SqlitePool, assistant_id: String) -> Router {
    // ... (same as in your main file)
}
#[tokio::test]
async fn test_chat_endpoint() {
    // Set up a temporary in-memory SQLite database for testing
    let db_pool = SqlitePool::connect(":memory:").await.unwrap();
    // Run database migrations here if necessary
    // sqlx::migrate!("./migrations").run(&db_pool).await.unwrap();
    // Create an assistant for testing
    let assistant = create_assistant(
        "Test Assistant",
        "gpt-4",
        "Your instructions here",
        "/context",
    )
    .await
    .unwrap();
    // Start the server using the `app` function
    let router = app(db_pool.clone(), assistant.id).await;
    let (tx, rx) = oneshot::channel::<()>();
    let server = axum::Server::bind(&"127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .serve(router.into_make_service())
        .with_graceful_shutdown(async {
            rx.await.ok();
        });
    let (server, addr) = tokio::spawn(server).await.unwrap();
    // Perform the test
    let client = reqwest::Client::new();
    let user_id = "test_user";
    let response = client
        .post(format!("http://{}/chat", addr))
        .json(&serde_json::json!({
            "user_id": user_id,
            "message": "Hello, chatbot!"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let response_json: serde_json::Value = response.json().await.unwrap();
    // ... (rest of your test assertions)
    // Shut down the server
    tx.send(()).unwrap();
    server.await.unwrap();
}
