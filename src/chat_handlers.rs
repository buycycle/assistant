use axum::{
    extract::{Extension, Path},
    Json, response::IntoResponse,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::{SqlitePool, SqliteConnectOptions}, Error};
use std::str::FromStr;

use std::env;
use log::{error, info};

#[derive(Deserialize)]
pub struct ChatRequest {
    pub chat_id: String,
    pub message: String,
}
#[derive(Serialize)]
pub struct ChatResponse {
    pub response: String,
}
pub async fn create_db_pool(database_url: &str) -> Result<SqlitePool, Error> {
    // Remove the `sqlite:` scheme from the `database_url` if it's present
    let connect_options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .to_owned();
    let pool = SqlitePool::connect_with(connect_options).await?;
    Ok(pool)
}
// Function to retrieve the conversation history from the database.
async fn get_conversation_history(pool: &SqlitePool, chat_id: &str) -> Result<Vec<String>, Error> {
    let messages = sqlx::query!(
        "SELECT content FROM messages WHERE chat_id = ? ORDER BY chat_id",
        chat_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .filter_map(|record| record.content) // Only keep records with Some(String)
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
    Extension(db_pool): Extension<SqlitePool>,
    Json(chat_request): Json<ChatRequest>,
) -> impl IntoResponse {
    let chat_id = chat_request.chat_id;
    info!("Received chat request for chat_id: {}", chat_id);
    let client = Client::new();
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            error!("OPENAI_API_KEY not set");
            return Json(ChatResponse { response: "OPENAI_API_KEY not set".to_string() });
        },
    };
    // Retrieve the conversation history from the database.
    let history = match get_conversation_history(&db_pool, &chat_id).await {
        Ok(history) => {
            info!("Successfully retrieved conversation history for chat_id: {}", chat_id);
            history
        },
        Err(e) => {
            error!("Failed to retrieve conversation history for chat_id: {}: {}", chat_id, e);
            return Json(ChatResponse { response: "Failed to retrieve conversation history".to_string() });
        },
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
        Ok(res) => {
            info!("Request to OpenAI API sent successfully");
            res
        },
        Err(e) => {
            error!("Failed to send request to OpenAI: {}", e);
            return Json(ChatResponse { response: format!("Failed to send request to OpenAI: {}", e) });
        },
    };
    if !response.status().is_success() {
        let error_message = match response.text().await {
            Ok(text) => text,
            Err(_) => {
                error!("Failed to read error message from OpenAI API response");
                "Failed to read error message from OpenAI API response".to_string()
            },
        };
        return Json(ChatResponse { response: error_message });
    }
    let openai_response: serde_json::Value = match response.json().await {
        Ok(res) => res,
        Err(_) => {
            error!("Failed to parse response from OpenAI");
            return Json(ChatResponse { response: "Failed to parse response from OpenAI".to_string() });
        },
    };
    let response_text = openai_response["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    // Save the new message to the conversation history in the database.
    if let Err(e) = save_message_to_history(&db_pool, &chat_id, &chat_request.message).await {
        error!("Failed to save message to history for chat_id: {}: {}", chat_id, e);
        return Json(ChatResponse { response: "Failed to save message to history".to_string() });
    }
    info!("Message saved to history for chat_id: {}", chat_id);
    Json(ChatResponse {
        response: response_text,
    })
}

