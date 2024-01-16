use std::fs;
use std::path::Path;

use sqlx::SqlitePool;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use serde_json::json;
use std::env;

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

///context.rs
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
            }
            Ok(res) => {
                return Err(res.text().await.map_err(|e| e.to_string())?);
            }
            Err(e) => {
                return Err(e.to_string());
            }
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
        Ok(res) if res.status().is_success() => match res.json::<FileUploadResponse>().await {
            Ok(file_response) => Ok(file_response.id),
            Err(_) => Err("Failed to parse response from OpenAI".to_string()),
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
            .post(format!(
                "https://api.openai.com/v1/assistants/{}/files",
                assistant_id
            ))
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
        Ok(res) if res.status().is_success() => match res.json::<serde_json::Value>().await {
            Ok(assistant_response) => {
                if let Some(id) = assistant_response["id"].as_str() {
                    Ok(id.to_string())
                } else {
                    Err("Failed to extract assistant ID from response".to_string())
                }
            }
            Err(_) => Err("Failed to parse response from OpenAI".to_string()),
        },
        Ok(res) => {
            let error_message = res.text().await.unwrap_or_default();
            Err(error_message)
        }
        Err(e) => Err(format!("Failed to send request to OpenAI: {}", e)),
    }
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

/// check db_pool if thread_id exists for user_id
/// if yes, return chat_id, if no, initialize chat, save user_id, chat_idto db table chats and return chat_id
async fn initialize_chat() -> Result<String, String> {
    let client = Client::new();
    let api_key = env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
    let response = client
        .post("https://api.openai.com/v1/threads")
        .header("Content-Type", "application/json")
        .bearer_auth(&api_key)
        .header("OpenAI-Beta", "assistants=v1")
        .send()
        .await;
    // Correctly handle the Result returned by the reqwest call
    match response {
        Ok(res) if res.status().is_success() => match res.json::<serde_json::Value>().await {
            Ok(create_chat_response) => {
                if let Some(id) = create_chat_response["id"].as_str() {
                    Ok(id.to_string())
                } else {
                    Err("Failed to extract chat ID from response".to_string())
                }
            }
            Err(_) => Err("Failed to parse response from OpenAI".to_string()),
        },
        Ok(res) => {
            // The response is not successful, so we attempt to read the error message
            match res.text().await {
                Ok(text) => Err(text),
                Err(_) => Err("Failed to read error message from OpenAI API response".to_string()),
            }
        }
        Err(e) => {
            // The request itself failed, so we return the error
            Err(format!("Failed to send request to OpenAI: {}", e))
        }
    }
}
async fn get_chat_id(db_pool: &SqlitePool, user_id: &str) -> Result<String, String> {
    // First, attempt to fetch the chat ID from the database.
    let result = sqlx::query!(
        "SELECT id FROM chats WHERE user_id = ? ORDER BY created_at DESC LIMIT 1",
        user_id
    )
    .fetch_optional(db_pool)
    .await
    .map_err(|e| format!("Database error: {}", e))?;
    // Match on the Option to determine if a chat ID was found.
    match result {
        Some(row) => {
            // If a chat_id is found, return it.
            Ok(row.id.to_string())
        }
        None => {
            // If no chat_id is found, call the create_chat function to create a new chat.
            let new_chat_id = initialize_chat()
                .await
                .map_err(|e| format!("Error creating chat: {}", e))?;
            // Insert the new chat_id into the database.
            sqlx::query!(
                "INSERT INTO chats (id, user_id) VALUES (?, ?)",
                new_chat_id,
                user_id
            )
            .execute(db_pool)
            .await
            .map_err(|e| format!("Database error: {}", e))?;
            // Return the new chat_id.
            Ok(new_chat_id)
        }
    }
}
// list_messages
// Arguments:
// chat_id
// Reurns:
// Json Response of all messages in chat
// log out all of them

// create_chat
// Arguments:
// assistant_id, db_pool, user_id
// 1. check threads table if chat exists for user, if yes return this thread_id
// 2. if no, initialize chat and return thread_id
// 3. list all messages from chat
// 4. add welcome back message
// 5. format and dislay

// think about websockets here
// assitant_chat_handler
// 1. add message to chat
// 2. run chat
// 3. return new message
