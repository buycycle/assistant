use axum::{
    extract::Extension,
    Json, response::IntoResponse,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{sqlite::{SqlitePool, SqliteConnectOptions}, Error};
use std::str::FromStr;

use std::env;
use log::{error, info};

use std::fs;
use std::path::Path;

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
        "SELECT content FROM messages WHERE chat_id = ? ORDER BY timestamp ASC",
        chat_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|record| record.content) // Directly map to String, assuming content is NOT NULL
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

#[derive(Deserialize)]
pub struct ChatRequest {
    pub chat_id: String,
    pub message: String,
}
#[derive(Serialize)]
pub struct ChatResponse {
    pub response: String,
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


// Define the response type for the JSON response.
#[derive(Serialize)]
#[serde(untagged)]
pub enum AssistantResponse {
    Success { id: String },
    Error { error: String },
}
// Define the response type for the file upload response.
#[derive(Deserialize)]
struct FileUploadResponse {
    id: String,
}
// Define the response type for attaching files to an assistant.
#[derive(Serialize)]
struct AttachFilesRequest {
    file_id: String,
}


// Creates an OpenAI assistant with the specified name, model, and instructions. Tools are so far
// hardcoded as a code_interpreter.
///
/// # Arguments
/// * `assistant_name` - The name of the assistant to create.
/// * `model` - The model to use for the assistant (e.g., "gpt-4").
/// * `instructions` - The instructions for the assistant's behavior.
///
/// # Returns
/// A `Result` containing either the assistant's ID on success or an error message on failure.
async fn initialize_assistant(
    assistant_name: &str,
    model: &str,
    instructions: &str,
) -> Result<String, String> {
    let client = Client::new();
    let api_key = env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
    let payload = json!({
        "instructions": instructions,
        "name": assistant_name,
        "tools": [{"type": "code_interpreter"}],
        "model": model,
    });
    let response = client
        .post("https://api.openai.com/v1/assistants")
        .bearer_auth(&api_key)
        .json(&payload)
        .send()
        .await;
    match response {
        Ok(res) if res.status().is_success() => {
            match res.json::<serde_json::Value>().await {
                Ok(assistant_response) => {
                    if let Some(id) = assistant_response["id"].as_str() {
                        Ok(id.to_string())
                    } else {
                        Err("Failed to extract assistant ID from response".to_string())
                    }
                },
                Err(_) => Err("Failed to parse response from OpenAI".to_string()),
            }
        },
        Ok(res) => {
            let error_message = res.text().await.unwrap_or_default();
            Err(error_message)
        },
        Err(e) => {
            Err(format!("Failed to send request to OpenAI: {}", e))
        },
    }
}

/// Scrapes a list of URLs and saves them as html files in the specified folder.
pub async fn scrape_context(folder_path: &str, urls: Vec<String>) -> Result<(), String> {
    let client = Client::new();
    let folder_path = Path::new(folder_path);
    // Create the folder if it does not exist
    fs::create_dir_all(&folder_path).map_err(|e| e.to_string())?;
    for url in urls {
        let response = client.get(&url).send().await;
        match response {
            Ok(res) if res.status().is_success() => {
                // Sanitize the file name by removing URL schemes and replacing slashes with underscores
                let file_name = url
                    .replace("https://", "")
                    .replace("http://", "")
                    .replace("/", "_");
                let file_path = folder_path.join(format!("{}.html", file_name));
                let html = res.text().await.map_err(|e| e.to_string())?;
                fs::write(file_path, html).map_err(|e| e.to_string())?;
            },
            Ok(res) => {
                return Err(res.text().await.map_err(|e| e.to_string())?);
            },
            Err(e) => {
                return Err(e.to_string());
            },
        }
    }
    Ok(())
}

/// Uploads a file to OpenAI and returns the file ID.
async fn upload_file(file_path: &str) -> Result<String, String> {
    let api_key = env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
    let client = Client::new();
    let payload = json!({
        "purpose": "assistants",
        "file": file_path,
    });
    let response = client
        .post("https://api.openai.com/v1/files")
        .bearer_auth(&api_key)
        .json(&payload)
        .send()
        .await;
    match response {
        Ok(res) if res.status().is_success() => {
            match res.json::<FileUploadResponse>().await {
                Ok(file_response) => Ok(file_response.id),
                Err(_) => Err("Failed to parse response from OpenAI".to_string()),
            }
        },
        Ok(res) => Err(res.text().await.unwrap_or_default()),
        Err(e) => Err(e.to_string()),
    }
}
/// Attaches a list of file IDs to an assistant.
async fn attach_files(assistant_id: &str, file_ids: Vec<String>) -> Result<(), String> {
    let api_key = env::var("OPENAI_API_KEY").unwrap_or_default();
    let client = Client::new();
    for file_id in file_ids {
        let payload = AttachFilesRequest { file_id };
        let response = client
            .post(format!("https://api.openai.com/v1/assistants/{}/files", assistant_id))
            .bearer_auth(&api_key)
            .json(&payload)
            .send()
            .await;
        if let Err(e) = response {
            return Err(e.to_string());
        }
    }
    Ok(())
}

/// Creates an OpenAI assistant with the specified name, model, instructions, and file path.
/// All files in the specified directory will be uploaded and attached to the assistant.
pub async fn create_assistant(
    assistant_name: &str,
    model: &str,
    instructions: &str,
    folder_path: &str,
) -> Result<String, String> {
    let assistant_id = initialize_assistant(assistant_name, model, instructions).await?;

    // Read the directory contents
    let paths = fs::read_dir(Path::new(folder_path)).map_err(|e| e.to_string())?;

    // Iterate over each file and upload and attach it
    for path in paths {
        let path = path.map_err(|e| e.to_string())?.path();
        if path.is_file() {
            // Upload the file
            let file_id = upload_file(path.to_str().unwrap()).await?;
            // Attach the file to the assistant
            attach_files(&assistant_id, vec![file_id]).await?;
        }
    }

    Ok(assistant_id)
}

