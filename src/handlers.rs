use axum::Json;
use reqwest::Client;
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
}
#[derive(Serialize)]
pub struct ChatResponse {
    pub response: String,
}
pub async fn chat_handler(Json(chat_request): Json<ChatRequest>) -> Json<ChatResponse> {
    let client = Client::new();
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
    // Construct the payload for the OpenAI API.
    let payload = serde_json::json!({
        "model": "gpt-3.5-turbo",
        "prompt": chat_request.message,
        // Add other parameters as needed.
    });
    // Send the request to OpenAI's API.
    let response = client
        .post("https://api.openai.com/v1/engines/gpt-3.5-turbo/completions")
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
        .expect("Failed to send request to OpenAI");
    // Parse the response from OpenAI.
    let openai_response: serde_json::Value = response
        .json()
        .await
        .expect("Failed to parse response from OpenAI");
    // Extract the text from the OpenAI response.
    let response_text = openai_response["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    // Return the response text as JSON.
    Json(ChatResponse {
        response: response_text,
    })
}

