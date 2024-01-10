use reqwest::Client;
use axum::{Json, response::IntoResponse};
use serde::{Deserialize, Serialize};
use std::env;
#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
}
#[derive(Serialize)]
pub struct ChatResponse {
    pub response: String,
}
pub async fn chat_handler(Json(chat_request): Json<ChatRequest>) -> impl IntoResponse {
    let client = Client::new();
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => return Json(ChatResponse { response: "OPENAI_API_KEY not set".to_string() }),
    };
    let payload = serde_json::json!({
        "model": "gpt-4",
        "messages": [
            {
                "role": "system",
                "content": "You are a helpful buyers guide chat bot that helps to find used bikes."
            },
            {
                "role": "user",
                "content": chat_request.message,
            }
        ],
    });
    let response = match client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => return Json(ChatResponse { response: format!("Failed to send request to OpenAI: {}", e) }),
    };
    if !response.status().is_success() {
        // Attempt to extract the error message from the OpenAI response
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
    Json(ChatResponse {
        response: response_text,
    })
}

