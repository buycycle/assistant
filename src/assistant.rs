use std::fs;
use std::iter::Extend;
use std::path::Path;

use axum::{response::IntoResponse, Extension, Json};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool},
    Error,
};

use reqwest::Client;
use serde::{Deserialize, Serialize};

use log::{error, info};
use serde_json::json;
use std::env;



use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool},
    Error,
};


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

// Struct for deserializing the OpenAI API response
#[derive(Serialize, Deserialize, Debug)]
pub struct Message {
    id: String,
    created_at: i64,
    role: String,
    content: Vec<Content>,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct Content {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<TextContent>,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct TextContent {
    value: String,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct MessageListResponse {
    object: String,
    data: Vec<Message>,
}
// Struct for serializing the simplified message format to be sent to the client
#[derive(Serialize, Clone)]
pub struct SimplifiedMessage {
    pub created_at: i64,
    pub role: String,
    pub text: String,
}

// Struct for serializing the message content to be sent to OpenAI
#[derive(Serialize)]
struct MessageContent {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct RunResponse {
    id: String,
}
#[derive(Deserialize, Debug)]
struct RunStatusResponse {
    id: String,
    status: String,
    // Other fields can be added here if needed
}
#[derive(Deserialize)]
pub struct AssistantChatRequest {
    pub user_id: String,
    pub message: String,
}
// Define the response type for the assistant chat handler.
#[derive(Serialize)]
pub struct AssistantChatResponse {
    pub messages: Vec<SimplifiedMessage>,
}

/// A struct representing an OpenAI assistant.
/// The tools are currently hardcoded as a code_interpreter.
struct Assistant {
    id: String,
    name: String,
    model: String,
    instructions: String,
    folder_path: String,
    scrape_urls: Vec<String>,
}
impl Assistant {
    /// create an OpenAI assistant and set the assistant's ID
    pub async fn initialize(&mut self) -> Result<(), String> {
        let client = Client::new();
        let api_key =
            env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
        let payload = json!({
            "instructions": self.instructions,
            "name": self.name,
            "tools": [{"type": "code_interpreter"}],
            "model": self.model,
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
                        self.id = id.to_string();
                        Ok(())
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
    /// scrape context from URLs and save them as HTML files
    pub async fn scrape_context(&self) -> Result<(), String> {
        let client = Client::new();
        let folder_path = Path::new(&self.folder_path);
        // Create the folder if it does not exist
        fs::create_dir_all(&folder_path).map_err(|e| e.to_string())?;
        for url in &self.scrape_urls {
            let response = client.get(url).send().await;
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
    /// upload a file to OpenAI and return the file ID
    pub async fn upload_file(&self, file_path: &str) -> Result<String, String> {
        let api_key =
            env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
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
    /// attach a list of file IDs to the assistant
    pub async fn attach_files(&self, file_ids: Vec<String>) -> Result<(), String> {
        let api_key = env::var("OPENAI_API_KEY").unwrap_or_default();
        let client = Client::new();
        for file_id in file_ids {
            let payload = AttachFilesRequest { file_id };
            let response = client
                .post(format!(
                    "https://api.openai.com/v1/assistants/{}/files",
                    self.id
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
}

/// Creates an OpenAI assistant with the specified name, model, instructions, and file path.
/// All files in the specified directory will be uploaded and attached to the assistant.
pub async fn create_assistant(
    assistant_name: &str,
    model: &str,
    instructions: &str,
    folder_path: &str,
) -> Result<Assistant, String> {
    // Create the assistant using the OpenAI API
    let assistant_id = initialize(assistant_name, model, instructions).await?;
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
    // Return the created Assistant instance
    Ok(Assistant {
        id: assistant_id,
        name: assistant_name.to_string(),
        model: model.to_string(),
        instructions: instructions.to_string(),
        folder_path: folder_path.to_string(),
        scrape_urls: Vec::new(), // Initialize with an empty vector or populate as needed
    })
}

struct Chat {
    id: String,
    user_id: String,
    messages: Vec<SimplifiedMessage>,
}

impl Chat {
    /// Method to initialize a chat or retrieve an existing one
    /// if yes, return chat_id, if no, initialize chat, save user_id, chat_idto db table chats and return chat_id
    pub async fn initialize(&mut self) -> Result<(), String> {
        let client = Client::new();
        let api_key =
            env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
        let response = client
            .post("https://api.openai.com/v1/threads")
            .header("Content-Type", "application/json")
            .bearer_auth(&api_key)
            .header("OpenAI-Beta", "assistants=v1")
            .send()
            .await;
        match response {
            Ok(res) if res.status().is_success() => match res.json::<serde_json::Value>().await {
                Ok(create_chat_response) => {
                    if let Some(id) = create_chat_response["id"].as_str() {
                        self.id = id.to_string();
                        Ok(())
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
                    Err(_) => {
                        Err("Failed to read error message from OpenAI API response".to_string())
                    }
                }
            }
            Err(e) => {
                // The request itself failed, so we return the error
                Err(format!("Failed to send request to OpenAI: {}", e))
            }
        }
    }

    /// Retrieves a list of simplified messages for the chat.
    /// Each message includes the `created_at` timestamp, `role`, and text content.
    /// If `only_last` is true, only the last message is returned.
    pub async fn list_messages(&mut self, only_last: bool) -> Result<(), String> {
        let client = Client::new();
        let api_key =
            env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
        let response = client
            .get(&format!(
                "https://api.openai.com/v1/threads/{}/messages",
                self.id
            ))
            .header("Content-Type", "application/json")
            .bearer_auth(&api_key)
            .header("OpenAI-Beta", "assistants=v1")
            .send()
            .await;
        match response {
            Ok(res) if res.status().is_success() => {
                let message_list_response = res
                    .json::<MessageListResponse>()
                    .await
                    .map_err(|_| "Failed to parse response from OpenAI".to_string())?;
                let mut simplified_messages: Vec<SimplifiedMessage> = message_list_response
                    .data
                    .into_iter()
                    .filter_map(|msg| {
                        if let Some(content) =
                            msg.content.into_iter().find(|c| c.content_type == "text")
                        {
                            if let Some(text_content) = content.text {
                                return Some(SimplifiedMessage {
                                    created_at: msg.created_at,
                                    role: msg.role,
                                    text: text_content.value,
                                });
                            }
                        }
                        None
                    })
                    .collect();
                if only_last {
                    simplified_messages = simplified_messages.into_iter().rev().take(1).collect();
                }
                self.messages = simplified_messages;
                Ok(())
            }
            Ok(res) => match res.text().await {
                Ok(text) => Err(text),
                Err(_) => Err("Failed to read error message from OpenAI API response".to_string()),
            },
            Err(e) => Err(format!("Failed to send request to OpenAI: {}", e)),
        }
    }
    /// Sends a message to the chat using the OpenAI API.
    /// The `role` parameter typically is "user" or "system".
    pub async fn add_message(&self, message: &str, role: &str) -> Result<(), String> {
        let client = Client::new();
        let api_key =
            env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
        let payload = MessageContent {
            role: role.to_string(),
            content: message.to_string(),
        };
        let response = client
            .post(&format!(
                "https://api.openai.com/v1/threads/{}/messages",
                self.id
            ))
            .header("Content-Type", "application/json")
            .bearer_auth(&api_key)
            .header("OpenAI-Beta", "assistants=v1")
            .json(&payload)
            .send()
            .await;
        match response {
            Ok(res) if res.status().is_success() => Ok(()),
            Ok(res) => match res.text().await {
                Ok(text) => Err(text),
                Err(_) => Err("Failed to read error message from OpenAI API response".to_string()),
            },
            Err(e) => Err(format!("Failed to send request to OpenAI: {}", e)),
        }
    }
}

//add a create_chat function that returns a chat struct
// check the db for an existing chat_id for the user_id
// if yes, return chat_id and initialize chat struct
// if no, initialize chat struct, save user_id, chat_id to db table chats and return chat_id

// add a DB struct here
// it should have all the db related functions as methods
/// get chat_id for a given user_id
pub async fn create_db_pool(database_url: &str) -> Result<SqlitePool, Error> {
// Remove the `sqlite:` scheme from the `database_url` if it's present
    let connect_options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .to_owned();
    let pool = SqlitePool::connect_with(connect_options).await?;
    Ok(pool)
}

async fn get_chat_id(db_pool: &SqlitePool, user_id: &str) -> Result<Option<String>, String> {
    let result = sqlx::query!(
        "SELECT id FROM chats WHERE user_id = ? ORDER BY created_at DESC LIMIT 1",
        user_id
    )
    .fetch_optional(db_pool)
    .await
    .map_err(|e| format!("Database error: {}", e))?;
    match result {
        Some(row) => Ok(Some(row.id)),
        None => Ok(None), // No chat ID found, return None
    }
}
/// Function to save the chat ID into the database
async fn save_chat_id(db_pool: &SqlitePool, user_id: &str, chat_id: &str) -> Result<(), String> {
    sqlx::query!(
        "INSERT INTO chats (id, user_id) VALUES (?, ?)",
        chat_id,
        user_id
    )
    .execute(db_pool)
    .await
    .map_err(|e| format!("Database error: {}", e))?;
    Ok(())
}

/// Saves a user's message to the database.
///
/// # Arguments
///
/// * `db_pool` - A `SqlitePool` for database connectivity.
/// * `chat_id` - The identifier of the chat thread.
/// * `message` - The message content to be saved.
///
/// # Returns
///
/// This function returns a `Result` which is either:
/// - `Ok(())`: An empty tuple indicating the message was saved successfully.
/// - `Err(String)`: An error message string indicating what went wrong during the operation.
async fn save_message_to_db(
    db_pool: &SqlitePool,
    chat_id: &str,
    message: &str,
) -> Result<(), String> {
    // Implement the logic to save the message to the database.
    // This is a placeholder and should be replaced with actual database interaction code.
    // For example:
    sqlx::query!(
        "INSERT INTO messages (chat_id, content) VALUES (?, ?)",
        chat_id,
        message
    )
    .execute(db_pool)
    .await
    .map_err(|e| format!("Database error: {}", e))?;
    Ok(())
}

struct Run {
    id: String,
    status: String,
}

/// Creates a run for a given thread and assistant and assigns the ID and status to the struct.
pub async fn create(&mut self, chat_id: &str, assistant_id: &str) -> Result<(), String> {
    let client = Client::new();
    let api_key = env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
    let payload = json!({
        "assistant_id": assistant_id,
    });
    let response = client
        .post(&format!(
            "https://api.openai.com/v1/threads/{}/runs",
            chat_id
        ))
        .header("Content-Type", "application/json")
        .bearer_auth(&api_key)
        .header("OpenAI-Beta", "assistants=v1")
        .json(&payload)
        .send()
        .await;
    match response {
        Ok(res) if res.status().is_success() => {
            let run_response = res
                .json::<RunResponse>()
                .await
                .map_err(|_| "Failed to parse response from OpenAI".to_string())?;
            // Assign the ID and status to the struct
            self.id = run_response.id;
            self.status = run_response.status;
            Ok(())
        }
        Ok(res) => match res.text().await {
            Ok(text) => Err(text),
            Err(_) => Err("Failed to read error message from OpenAI API response".to_string()),
        },
        Err(e) => Err(format!("Failed to send request to OpenAI: {}", e)),
    }

    /// Retrieves the status of the run for the given thread.
    pub async fn status(&self, chat_id: &str) -> Result<String, String> {
        let client = Client::new();
        let api_key =
            env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
        let response = client
            .get(&format!(
                "https://api.openai.com/v1/threads/{}/runs/{}",
                chat_id, self.id
            ))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("OpenAI-Beta", "assistants=v1")
            .send()
            .await;
        match response {
            Ok(res) if res.status().is_success() => {
                let run_status_response = res
                    .json::<RunStatusResponse>()
                    .await
                    .map_err(|_| "Failed to parse response from OpenAI".to_string())?;
                Ok(run_status_response.status)
            }
            Ok(res) => match res.text().await {
                Ok(text) => Err(text),
                Err(_) => Err("Failed to read error message from OpenAI API response".to_string()),
            },
            Err(e) => Err(format!("Failed to send request to OpenAI: {}", e)),
        }
    }
}


// think about websockets here
/// Handles chat interactions with an OpenAI assistant.
///
/// This function manages the chat initialization, message sending, and response retrieval.
/// It initializes a chat or retrieves an existing chat_id, savesthe user's message to the db,
/// sends the message to the chat,
/// creates a run for the assistant to process the message, waits for its completion
/// and retrieves the assistant's response.
///
/// # Arguments
///
/// * `db_pool` - A `SqlitePool` for database connectivity.
/// * `assistant_chat_request` - A `Json<AssistantChatRequest>` containing the user_id and message.
/// * `assistant_id` - The identifier of the OpenAI assistant.
///
/// # Returns
///
/// This function returns an `impl IntoResponse` which is a JSON response containing the updated
/// conversation history including the assistant's response.
pub async fn assistant_chat_handler(
    Extension(db_pool): Extension<SqlitePool>,
    Json(assistant_chat_request): Json<AssistantChatRequest>,
    assistant_id: &str, // This should be provided to the function or retrieved from the environment/config
) -> impl IntoResponse {
    let user_id = assistant_chat_request.user_id;
    let message = assistant_chat_request.message;
    // Initialize chat or get existing chat_id
    let chat_id = match get_chat_id(&db_pool, &user_id).await {
        Ok(chat_id) => chat_id,
        Err(e) => {
            error!("Failed to initialize or retrieve chat: {}", e);
            return Json(AssistantChatResponse { messages: vec![] });
        }
    };
    // Save the user's message to the database
    if let Err(e) = save_message_to_db(&db_pool, &chat_id, &message).await {
        error!("Failed to save user message to database: {}", e);
        // Decide how to handle the error, e.g., return an error response or continue processing
    }
    // Retrieve the full conversation history
    let history = match list_messages(&chat_id, false).await {
        Ok(messages) => messages,
        Err(e) => {
            error!("Failed to retrieve conversation history: {}", e);
            return Json(AssistantChatResponse { messages: vec![] });
        }
    };
    // Send the user's message to the chat
    if let Err(e) = add_message(&chat_id, &message, "user").await {
        error!("Failed to send message to chat: {}", e);
        return Json(AssistantChatResponse { messages: history });
    }
    // Create a run for the assistant to process the message
    let run_id = match create_run(&chat_id, assistant_id).await {
        Ok(run_id) => run_id,
        Err(e) => {
            error!("Failed to create run: {}", e);
            return Json(AssistantChatResponse { messages: history });
        }
    };
    // Check the status of the run until it's completed or a timeout occurs
    let mut status = String::new();
    let start_time = std::time::Instant::now();
    while start_time.elapsed().as_secs() < 10 {
        match run_status(&chat_id, &run_id).await {
            Ok(run_status) => {
                status = run_status;
                if status == "completed" {
                    break;
                }
            }
            Err(e) => {
                error!("Failed to check run status: {}", e);
                return Json(AssistantChatResponse { messages: history });
            }
        }
        // Sleep for a short duration before checking the status again
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    if status != "completed" {
        error!("Run did not complete in time");
        return Json(AssistantChatResponse { messages: history });
    }
    // Retrieve the last message from the conversation, which should be the assistant's response
    let last_message = match list_messages(&chat_id, true).await {
        Ok(messages) => messages,
        Err(e) => {
            error!("Failed to retrieve last message: {}", e);
            return Json(AssistantChatResponse { messages: history });
        }
    };
    // Return the updated conversation history including the assistant's response
    Json(AssistantChatResponse {
        messages: [history, last_message].concat(),
    })
}
