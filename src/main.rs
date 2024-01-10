mod handlers;
use axum::{
    routing::{get, post},
    Router,
};
use dotenv::dotenv;
use handlers::chat_handler;
fn app() -> Router {
    Router::new()
        .route("/chat", post(chat_handler))
        // Add more routes as needed.
}
#[tokio::main]
async fn main() {
    // Load environment variables from a .env file at the beginning of the program
    dotenv().ok();
    let app = app();
    // Bind the server to an address and start it.
    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

