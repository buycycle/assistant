use reqwest;
const BASE_URL: &str = "http://localhost:3000";
#[tokio::test]
async fn test_chat_endpoint() {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/chat", BASE_URL))
        .json(&serde_json::json!({
            "message": "Hello, chatbot!"
        }))
        .send()
        .await
        .expect("Failed to send request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let response_text = response.text().await.expect("Failed to read response text");
    // Perform additional checks on `response_text` if necessary
}
