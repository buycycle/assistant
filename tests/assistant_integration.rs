use ::rust_bot::assistant;
use assistant::{create_assistant, create_files};
use axum::{
    body::Body,
    extract::Extension,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use dotenv::dotenv;
use rust_bot::assistant::{assistant_chat_handler, DB};
use sqlx::SqlitePool;
use tower::ServiceExt; // for `app.oneshot()`
async fn app(db_pool: SqlitePool, assistant_id: String) -> Router {
    Router::new()
        .route("/assistant", post(assistant_chat_handler)) // Updated route
        .layer(Extension(db_pool))
        .layer(Extension(assistant_id)) // Add the assistant ID as a layer
}
// Helper function to create a test instance of the app with a temporary database
async fn setup_test_app() -> axum::Router {
    dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = DB::create_db_pool(&database_url)
        .await
        .expect("Failed to create database pool for testing");
    let db_pool = db.pool; // Extract the SqlitePool from the DB struct
    // Create the files for the assistant.
    let files = create_files("context", Vec::new()).await.expect("Failed to create files");
    // Create an assistant.
    let assistant = create_assistant(
        "Test",
        "gpt-4-1106-preview",
        "On buycycle.com, users can buy and sell pre-owned bicycles. \
        Help the users with how the website works, use the faq.html for referral links.",
        &files.file_ids,
    )
    .await
    .expect("Failed to create assistant");
    app(db_pool, assistant.id).await
}
#[tokio::test]
async fn test_assistant_chat_handler_returns_200() {
    let test_app = setup_test_app().await;
    // Create a dummy POST request to the `/assistant` endpoint
    let request_body = r#"{"user_id": "test_user", "message": "how does the shipping process work, return the url"}"#;
    let request = Request::builder()
        .method("POST")
        .uri("/assistant")
        .header("content-type", "application/json")
        .body(Body::from(request_body))
        .unwrap();
    // Send the request to the app and wait for the response
    let response = test_app
        .oneshot(request)
        .await
        .expect("Failed to get response");
    // Assert that the response status is 200 OK
    assert_eq!(response.status(), StatusCode::OK);
}
