use axum::{
    extract::{Extension, Path},
    Json, response::IntoResponse,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::env;
#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
}
#[derive(Serialize)]
pub struct ChatResponse {
    pub response: String,
}
use sqlx::Error;
// Initialize a connection pool to the SQLite database.
async fn create_db_pool(database_url: &str) -> Result<SqlitePool, Error> {
    SqlitePoolOptions::new()
        .connect(database_url)
        .await
}
// Function to retrieve the conversation history from the database.
async fn get_conversation_history(pool: &SqlitePool, chat_id: &str) -> Result<Vec<String>, Error> {
    let messages = sqlx::query!(
        "SELECT content FROM messages WHERE chat_id = ? ORDER BY id",
        chat_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|record| record.content)
    .collect();
    Ok(messages)
}
// Function to save a new message to the conversation history in the database.
async fn save_message_to_history(pool: &SqlitePool, chat_id: &str, message: &str) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO messages (chat_id, content) VALUES (?, ?)",
        chat_id,
        message
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn chat_handler(
    Path(chat_id): Path<String>,
    Extension(db_pool): Extension<SqlitePool>,
    Json(chat_request): Json<ChatRequest>,
) -> impl IntoResponse {
    let client = Client::new();
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => return Json(ChatResponse { response: "OPENAI_API_KEY not set".to_string() }),
    };
    // Retrieve the conversation history from the database.
    let history = match get_conversation_history(&db_pool, &chat_id).await {
        Ok(history) => history,
        Err(_) => return Json(ChatResponse { response: "Failed to retrieve conversation history".to_string() }),
    };
    // Construct the messages payload using the conversation history.
    let mut messages = vec![
        serde_json::json!({
            "role": "system",
            "content": "You are a helpful buyers guide chat bot that helps to find used bikes."
        }),
    ];
    for message in &history {
        messages.push(serde_json::json!({
            "role": "user",
            "content": message,
        }));
    }
    messages.push(serde_json::json!({
        "role": "user",
        "content": chat_request.message,
    }));
    let payload = serde_json::json!({
        "model": "gpt-4",
        "messages": messages,
    });
    // Send the request to OpenAI API
    let response = match client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(&api_key)
        .json(&payload)
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => return Json(ChatResponse { response: format!("Failed to send request to OpenAI: {}", e) }),
    };
    if !response.status().is_success() {
        let error_message = match response.text().await {
            Ok(text) => text,
            Err(_) => "Failed to read error message from OpenAI API response".to_string(),
        };
        return Json(ChatResponse { response: error_message });
    }
    let openai_response: serde_json::Value = match response.json().await {
        Ok(res) => res,
        Err(_) => return Json(ChatResponse { response: "Failed to parse response from OpenAI".to_string() }),
    };
    let response_text = openai_response["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    // Save the new message to the conversation history in the database.
    if let Err(_) = save_message_to_history(&db_pool, &chat_id, &chat_request.message).await {
        return Json(ChatResponse { response: "Failed to save message to history".to_string() });
    }
    Json(ChatResponse {
        response: response_text,
    })
}

