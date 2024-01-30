use std::fs;
use std::path::Path;
use log::info;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};

use reqwest::{multipart::Form, multipart::Part, Client};
use serde::{Deserialize, Serialize};

use serde_json::json;
use std::{env, time::Duration};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

// Define a custom error type that can be converted into an HTTP response.
#[derive(Debug)]
pub enum AssistantError {
    DatabaseError(String),
    OpenAIError(String),
}
impl IntoResponse for AssistantError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            AssistantError::DatabaseError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            AssistantError::OpenAIError(msg) => (StatusCode::BAD_GATEWAY, msg),
        };
        let body = Json(json!({ "error": error_message }));
        (status, body).into_response()
    }
}
// Convert sqlx::Error into AssistantError, preserving the error message
impl From<sqlx::Error> for AssistantError {
    fn from(e: sqlx::Error) -> Self {
        AssistantError::DatabaseError(e.to_string())
    }
}

// Implement From<reqwest::Error> for AssistantError
impl From<reqwest::Error> for AssistantError {
    fn from(err: reqwest::Error) -> Self {
        AssistantError::OpenAIError(err.to_string())
    }
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
    status: String,
}
#[derive(Deserialize, Debug)]
struct RunStatusResponse {
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

// Define the response type for the file upload response.
#[derive(Deserialize)]
struct FileUploadResponse {
    id: String,
}

pub struct Files {
    folder_path: String,
    file_ids: Vec<String>,
    scrape_urls: Vec<String>,
}
impl Files {
    pub async fn scrape_context(&self) -> Result<(), AssistantError> {
        let client = Client::new();
        let folder_path = Path::new(&self.folder_path);
        fs::create_dir_all(&folder_path)
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        for url in &self.scrape_urls {
            let response = client.get(url).send().await;
            match response {
                Ok(res) if res.status().is_success() => {
                    let file_name = url
                        .replace("https://", "")
                        .replace("http://", "")
                        .replace("/", "_");
                    let file_path = folder_path.join(format!("{}.html", file_name));
                    let html = res
                        .text()
                        .await
                        .map_err(|e| AssistantError::OpenAIError(e.to_string()))?;
                    fs::write(file_path, html)
                        .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
                }
                Ok(res) => {
                    let error_message = res
                        .text()
                        .await
                        .map_err(|e| AssistantError::OpenAIError(e.to_string()))?;
                    return Err(AssistantError::OpenAIError(error_message));
                }
                Err(e) => {
                    return Err(AssistantError::OpenAIError(e.to_string()));
                }
            }
        }
        Ok(())
    }
    pub async fn upload_files(&mut self) -> Result<(), AssistantError> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        let client = Client::new();
        let paths = fs::read_dir(Path::new(&self.folder_path))
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        for path in paths {
            let path = path
                .map_err(|e| AssistantError::DatabaseError(e.to_string()))?
                .path();
            if path.is_file() {
                let file_content =
                    fs::read(&path).map_err(|e| AssistantError::OpenAIError(e.to_string()))?;
                let file_name = path
                    .file_name()
                    .ok_or_else(|| {
                        AssistantError::OpenAIError("Failed to get file name".to_string())
                    })?
                    .to_str()
                    .ok_or_else(|| {
                        AssistantError::OpenAIError(
                            "Failed to convert file name to string".to_string(),
                        )
                    })?
                    .to_owned();
                let part = Part::bytes(file_content)
                    .file_name(file_name)
                    .mime_str("application/octet-stream")?;
                let form = Form::new().part("file", part).text("purpose", "assistants");
                let response = client
                    .post("https://api.openai.com/v1/files")
                    .header("OpenAI-Beta", "assistants=v1")
                    .bearer_auth(&api_key)
                    .multipart(form)
                    .send()
                    .await;
                match response {
                    Ok(res) if res.status().is_success() => {
                        if let Ok(file_response) = res.json::<FileUploadResponse>().await {
                            self.file_ids.push(file_response.id);
                        } else {
                            return Err(AssistantError::OpenAIError(
                                "Failed to parse response from OpenAI".to_string(),
                            ));
                        }
                    }
                    Ok(res) => {
                        let error_message = res.text().await.unwrap_or_default();
                        return Err(AssistantError::OpenAIError(error_message));
                    }
                    Err(e) => return Err(AssistantError::OpenAIError(e.to_string())),
                }
            }
        }
        Ok(())
    }
    pub async fn delete(&mut self) -> Result<(), AssistantError> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        let client = Client::new();
        for file_id in &self.file_ids {
            let response = client
                .delete(format!("https://api.openai.com/v1/files/{}", file_id))
                .bearer_auth(&api_key)
                .send()
                .await;
            match response {
                Ok(res) if res.status().is_success() => {}
                Ok(res) => {
                    let error_message = res.text().await.unwrap_or_default();
                    return Err(AssistantError::OpenAIError(error_message));
                }
                Err(e) => {
                    return Err(AssistantError::OpenAIError(format!(
                        "Failed to send DELETE request to OpenAI: {}",
                        e
                    )));
                }
            }
        }
        self.file_ids.clear();
        Ok(())
    }
}
/// A struct representing an OpenAI assistant.
/// The tools are currently hardcoded as a code_interpreter.
pub struct Assistant {
    pub id: String,
    name: String,
    model: String,
    instructions: String,
}
impl Assistant {
    /// create an OpenAI assistant and set the assistant's ID
    pub async fn initialize(&mut self) -> Result<(), AssistantError> {
        let client = Client::new();
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        let payload = json!({
            "instructions": self.instructions,
            "name": self.name,
            "tools": [
                {"type": "retrieval"},
                {"type": "code_interpreter"}
            ],
            "model": self.model,
        });
        let response = client
            .post("https://api.openai.com/v1/assistants")
            .header("OpenAI-Beta", "assistants=v1")
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
                        Err(AssistantError::OpenAIError(
                            "Failed to extract assistant ID from response".to_string(),
                        ))
                    }
                }
                Err(_) => Err(AssistantError::OpenAIError(
                    "Failed to parse response from OpenAI".to_string(),
                )),
            },
            Ok(res) => {
                let error_message = res.text().await.unwrap_or_default();
                Err(AssistantError::OpenAIError(error_message))
            }
            Err(e) => Err(AssistantError::OpenAIError(format!(
                "Failed to send request to OpenAI: {}",
                e
            ))),
        }
    }
    /// Delete the OpenAI assistant with the given ID
    pub async fn delete(&self) -> Result<(), AssistantError> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        let client = Client::new();
        let response = client
            .delete(format!("https://api.openai.com/v1/assistants/{}", self.id))
            .header("OpenAI-Beta", "assistants=v1")
            .bearer_auth(&api_key)
            .send()
            .await;
        match response {
            Ok(res) if res.status().is_success() => Ok(()),
            Ok(res) => {
                let error_message = res.text().await.unwrap_or_default();
                Err(AssistantError::OpenAIError(error_message))
            }
            Err(e) => Err(AssistantError::OpenAIError(format!(
                "Failed to send DELETE request to OpenAI: {}",
                e
            ))),
        }
    }

    pub async fn attach_files(&self, file_ids: &[String]) -> Result<(), AssistantError> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        let client = Client::new();
        for file_id in file_ids {
            let payload = json!({ "file_id": file_id });
            let response = client
                .post(format!(
                    "https://api.openai.com/v1/assistants/{}/files",
                    self.id
                ))
                .header("OpenAI-Beta", "assistants=v1")
                .bearer_auth(&api_key)
                .json(&payload)
                .send()
                .await;
            match response {
                Ok(res) if res.status().is_success() => continue,
                Ok(res) => {
                    let error_message = res.text().await.unwrap_or_default();
                    return Err(AssistantError::OpenAIError(error_message));
                }
                Err(e) => return Err(AssistantError::OpenAIError(e.to_string())),
            }
        }
        Ok(())
    }
}
/// scrape urls and upload the resulting files to OpenAI
pub async fn create_files(folder_path: &str, scrape_urls: Vec<String>) -> Result<Files, AssistantError> {
    // Initialize the Files struct directly
    let mut files = Files {
        folder_path: folder_path.to_string(),
        file_ids: Vec::new(), // Initially empty, will be filled during file upload
        scrape_urls, // Provided scrape URLs
    };
    // Scrape the context from the provided URLs
    files.scrape_context().await?;
    // Upload the scraped files to OpenAI
    files.upload_files().await?;
    Ok(files)
}
pub async fn create_assistant(
    assistant_name: &str,
    model: &str,
    instructions: &str,
    files: Files, // Add this parameter to accept a Files struct
) -> Result<Assistant, AssistantError> {
    let mut assistant = Assistant {
        id: String::new(),
        name: assistant_name.to_string(),
        model: model.to_string(),
        instructions: instructions.to_string(),
    };
    // Initialize the assistant by creating it on the OpenAI platform
    assistant.initialize().await?;
    info!("Assistant created with ID: {}", assistant.id);
    // Attach the uploaded files to the assistant using the file IDs from the Files struct
    assistant.attach_files(&files.file_ids).await?;
    Ok(assistant)
}

pub async fn teardown_assistant(
    assistant: Assistant,
) -> Result<(), AssistantError> {
    // Delete the assistant on the OpenAI platform
    assistant.delete().await?;
    info!("Assistant with ID: {} has been deleted", assistant.id);
    Ok(())
}

struct Chat {
    id: String,
    messages: Vec<SimplifiedMessage>,
}

impl Chat {
    /// Method to initialize a chat or retrieve an existing one
    /// if yes, return chat_id, if no, initialize chat, save user_id, chat_idto db table chats and return chat_id
    pub async fn initialize(&mut self) -> Result<(), AssistantError> {
        let client = Client::new();
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
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
                        Err(AssistantError::OpenAIError(
                            "Failed to extract chat ID from response".to_string(),
                        ))
                    }
                }
                Err(_) => Err(AssistantError::OpenAIError(
                    "Failed to parse response from OpenAI".to_string(),
                )),
            },
            Ok(res) => {
                let error_message = res.text().await.unwrap_or_default();
                Err(AssistantError::OpenAIError(error_message))
            }
            Err(e) => Err(AssistantError::OpenAIError(format!(
                "Failed to send request to OpenAI: {}",
                e
            ))),
        }
    }
    pub async fn get_messages(&mut self, only_last: bool) -> Result<(), AssistantError> {
    let client = Client::new();
    let api_key = env::var("OPENAI_API_KEY")
        .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
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
            let message_list_response =
                res.json::<MessageListResponse>().await.map_err(|_| {
                    AssistantError::OpenAIError(
                        "Failed to parse response from OpenAI".to_string(),
                    )
                })?;
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
                // Get the last message based on created_at without sorting
                if let Some(last_message) = simplified_messages.iter().max_by_key(|m| m.created_at) {
                    simplified_messages = vec![last_message.clone()];
                }
            } else {
                // Sort by created_at in ascending order only if we need the full list
                simplified_messages.sort_by_key(|m| m.created_at);
            }
            self.messages = simplified_messages;
            Ok(())
        }
        Ok(res) => {
            let error_message = res.text().await.unwrap_or_default();
            Err(AssistantError::OpenAIError(error_message))
        }
        Err(e) => Err(AssistantError::OpenAIError(e.to_string())),
    }
}
    pub async fn add_message(&self, message: &str, role: &str) -> Result<(), AssistantError> {
        let client = Client::new();
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
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
            Ok(res) => {
                let error_message = res.text().await.unwrap_or_default();
                Err(AssistantError::OpenAIError(error_message))
            }
            Err(e) => Err(AssistantError::OpenAIError(format!(
                "Failed to send request to OpenAI: {}",
                e
            ))),
        }
    }
}

//add a create_chat function that returns a chat struct
// check the db for an existing chat_id for the user_id
// if yes, return chat_id and initialize chat struct
// if no, initialize chat struct, save user_id, chat_id to db table chats and return chat_id

pub struct DB {
    pub pool: SqlitePool,
}

impl DB {
    /// Creates a new database connection pool.
    pub async fn create_db_pool(database_url: &str) -> Result<Self, AssistantError> {
        // Remove the `sqlite:` scheme from the `database_url` if it's present
        let connect_options = SqliteConnectOptions::new()
            .filename(database_url) // Set the path to the SQLite database file
            .create_if_missing(true) // Create the database file if it does not exist
            .to_owned()
            .busy_timeout(Duration::from_secs(5)); // Set a busy timeout if needed
        let pool = SqlitePool::connect_with(connect_options)
            .await
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        Ok(DB { pool })
    }
    /// Gets the chat ID for a given user ID.
    pub async fn get_chat_id(&self, user_id: &String) -> Result<Option<String>, AssistantError> {
        let result = sqlx::query!(
            "SELECT id FROM chats WHERE user_id = ? ORDER BY created_at DESC LIMIT 1",
            user_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        match result {
            Some(row) => Ok(row.id), // row.id is already an Option<String>
            None => Ok(None), // No chat ID found, return None
        }
    }
    /// Saves the chat ID into the database.
    pub async fn save_chat_id(&self, user_id: &str, chat_id: &str) -> Result<(), AssistantError> {
        sqlx::query!(
            "INSERT INTO chats (id, user_id) VALUES (?, ?)",
            chat_id,
            user_id
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        Ok(())
    }
    /// Saves a user's message to the database.
    pub async fn save_message_to_db(
        &self,
        chat_id: &str,
        message: &str,
    ) -> Result<(), AssistantError> {
        sqlx::query!(
            "INSERT INTO messages (chat_id, content) VALUES (?, ?)",
            chat_id,
            message
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        Ok(())
    }
}

struct Run {
    id: String,
    status: String,
}
impl Run {
    /// Creates a run for a given thread and assistant and assigns the ID and status to the struct.
    pub async fn create(
        &mut self,
        chat_id: &str,
        assistant_id: &str,
    ) -> Result<(), AssistantError> {
        let client = Client::new();
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
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
                let run_response = res.json::<RunResponse>().await.map_err(|_| {
                    AssistantError::OpenAIError("Failed to parse response from OpenAI".to_string())
                })?;
                // Assign the ID and status to the struct
                self.id = run_response.id;
                self.status = run_response.status;
                Ok(())
            }
            Ok(res) => {
                let error_message = res.text().await.unwrap_or_default();
                Err(AssistantError::OpenAIError(error_message))
            }
            Err(e) => Err(AssistantError::OpenAIError(format!(
                "Failed to send request to OpenAI: {}",
                e
            ))),
        }
    }
    /// Retrieves the status of the run for the given thread.
    pub async fn get_status(&mut self, chat_id: &str) -> Result<(), AssistantError> {
        let client = Client::new();
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
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
                let run_status_response = res.json::<RunStatusResponse>().await.map_err(|_| {
                    AssistantError::OpenAIError("Failed to parse response from OpenAI".to_string())
                })?;
                self.status = run_status_response.status;
                Ok(())
            }
            Ok(res) => {
                let error_message = res.text().await.unwrap_or_default();
                Err(AssistantError::OpenAIError(error_message))
            }
            Err(e) => Err(AssistantError::OpenAIError(format!(
                "Failed to send request to OpenAI: {}",
                e
            ))),
        }
    }
}
// think about websockets here
/// Handles chat interactions with an OpenAI assistant.
///
/// This function manages the chat initialization, message sending, and response retrieval.
/// It initializes a chat or retrieves an existing chat_id, saves the user's message to the db,
/// sends the message to the chat, creates a run for the assistant to process the message,
/// waits for its completion, and retrieves the assistant's response.
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
    Extension(assistant_id): Extension<String>,
    Json(assistant_chat_request): Json<AssistantChatRequest>,
) -> Result<Json<AssistantChatResponse>, AssistantError> {
    let db = DB { pool: db_pool };
    let user_id = &assistant_chat_request.user_id;
    let message = &assistant_chat_request.message;
    // Initialize chat or get existing chat_id
    let chat_id = match db.get_chat_id(user_id).await? {
        Some(id) => id,
        None => {
            let mut chat = Chat {
                id: String::new(), // Temporarily set to String::new(), will be updated below
                messages: Vec::new(),
            };
            chat.initialize().await?;
            let new_chat_id = chat.id; // No need to parse as i64, it's already a String
            db.save_chat_id(user_id, &new_chat_id).await?;
            new_chat_id
        }
    };
    // Save the user's message to the database
    db.save_message_to_db(&chat_id.to_string(), message).await?;
    // Initialize the chat struct with the correct chat_id type
    let mut chat = Chat {
        id: chat_id.to_string(),
        messages: Vec::new(),
    };
    // Send the user's message to the chat
    chat.add_message(message, "user").await?;
    // Create a run for the assistant to process the message
    let mut run = Run {
        id: String::new(),
        status: String::new(),
    };
    run.create(&chat.id, &assistant_id).await?;
    // Check the status of the run until it's completed or a timeout occurs
    let start_time = std::time::Instant::now();
    while start_time.elapsed().as_secs() < 120 {
        run.get_status(&chat.id).await?; // This sets the run.status field
        if run.status == "completed" {
            info!("Run completed, status: {}", run.status);
            break;
        }
        info!("Run not completed, current status: {}", run.status);
        // Sleep for a short duration before checking the status again
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
    // Use the run.status field for the final check
    if run.status != "completed" {
        return Err(AssistantError::OpenAIError(
            "Run did not complete in time".to_string(),
        ));
    }
    // Retrieve the last message from the conversation, which should be the assistant's response
    chat.get_messages(true).await?;
    // Return the updated conversation history including the assistant's response
    Ok(Json(AssistantChatResponse {
        messages: chat.messages,
    }))
}
