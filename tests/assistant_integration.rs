use reqwest;
use serde_json::json;
const BASE_URL: &str = "http://localhost:3000";
#[tokio::test]
async fn test_chat_endpoint() {
    let client = reqwest::Client::new();
    let user_id = "test_user"; // Example user ID for testing
    // Send a chat message to the chat endpoint
    let response = client
        .post(format!("{}/chat", BASE_URL))
        .json(&json!({
            "user_id": user_id,
            "message": "Hello, chatbot!"
        }))
        .send()
        .await
        .expect("Failed to send request");
    // Check that the response status code is OK
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    // Parse the response body as JSON
    let response_json: serde_json::Value = response.json().await.expect("Failed to parse response as JSON");
    // Perform additional checks on the response JSON
    // For example, check that the response contains the expected message structure
    if let Some(messages) = response_json.get("messages") {
        assert!(messages.is_array(), "Expected 'messages' to be an array");
        // Check that the array contains at least one message
        assert!(!messages.as_array().unwrap().is_empty(), "Expected at least one message in the response");
        // Check that the message content matches what was sent
        assert_eq!(messages[0].get("text").unwrap(), "Hello, chatbot!");
    } else {
        panic!("Response JSON does not contain 'messages' key");
    }
}

