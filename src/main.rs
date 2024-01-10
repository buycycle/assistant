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
}
#[tokio::main]
async fn main() {
    dotenv().ok();
    let app = app();
    // Bind the server to an address and start it.
    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

