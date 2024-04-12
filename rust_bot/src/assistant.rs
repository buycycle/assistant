use tokio::sync::RwLock;
use std::sync::Arc;
use axum::{
    extract::Form as AxumForm,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::Utc;
use log::info;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use reqwest::{multipart::Form, multipart::Part, Client};
use serde::{Deserialize, Serialize};

use serde_json::json;
use std::env;

use sqlx::Pool;
use sqlx::{mysql::MySqlPoolOptions, FromRow, MySql, MySqlPool};

// Define a constant for the timeout duration of assistant response
const TIMEOUT_DURATION: u64 = 30;

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
    fn from(e: reqwest::Error) -> Self {
        AssistantError::OpenAIError(e.to_string())
    }
}
// Define the response type for attaching files to an assistant.
#[derive(Serialize)]
struct AttachFilesRequest {
    file_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatMessage {
    id: String,
    created_at: i64,
    role: String,
    content: Vec<Content>,
}
// Message content in the Chat
#[derive(Serialize, Deserialize, Debug)]
pub struct Content {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<TextContent>,
    // image: Option<ImageContent>,
    //
}
// TextContent in the Chat
#[derive(Serialize, Deserialize, Debug)]
pub struct TextContent {
    value: String,
}
// List messages in a chat
#[derive(Serialize, Deserialize, Debug)]
pub struct ChatMessageList {
    object: String,
    data: Vec<ChatMessage>,
}

// Struct for serializing the message to be sent to OpenAI
#[derive(Serialize)]
struct UserMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct RunResponse {
    id: String,
    status: String,
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

// Struct for serializing the simplified message format to be sent to the client
#[derive(Serialize, Clone)]
pub struct SimplifiedMessage {
    pub created_at: i64,
    pub role: String,
    pub text: String,
}
// Define the response type for the file upload response.
#[derive(Deserialize)]
struct FileUploadResponse {
    id: String,
    filename: String,
}

#[derive(Clone)]
pub struct FileInfo {
    pub file_id: String,
    pub file_name: String,
}

#[derive(Clone)]
pub struct Ressources {
    pub files_info: Vec<FileInfo>,
    folder_path: String,
    scrape_urls: Vec<String>,
    instruction_file_path: String,
    instruction: String,
}
#[derive(Serialize, FromRow)] // Derive the FromRow trait
struct Bike {
    category: String,
    color: Option<String>, // Changed to Option<String> to handle NULL values
    frame_size: String,
    price: f64,
    rider_height_max: Option<f64>,
    rider_height_min: Option<f64>,
    slug: String,
}
impl Ressources {
    pub async fn bikes_db(&self) -> Result<(), AssistantError> {
        let database_url = env::var("DATABASE_URL_PROD").map_err(|_| {
            AssistantError::DatabaseError("DATABASE_URL_PROD environment variable not set".to_string())
        })?;

        // Create a connection pool for MySQL
        let pool: Pool<MySql> = MySqlPoolOptions::new()
            .connect(&database_url)
            .await
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;

        // Define the query
        let main_query = "
            SELECT buycycle.bikes.slug as slug,
                   buycycle.bike_categories.slug as category,
                   buycycle.bike_additional_infos.frame_size as frame_size,
                   buycycle.bike_additional_infos.rider_height_min as rider_height_min,
                   buycycle.bike_additional_infos.rider_height_max as rider_height_max,
                   bikes.price,
                   bikes.color
            FROM buycycle.bikes
            JOIN buycycle.bike_additional_infos ON bikes.id = bike_additional_infos.bike_id
            JOIN buycycle.bike_categories ON bikes.bike_category_id = bike_categories.id
            WHERE bikes.status = 'active'
        ";

        // Execute the query using sqlx
        let bikes: Vec<Bike> = sqlx::query_as(main_query)
            .fetch_all(&pool)
            .await
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;

        // Serialize the bikes to JSON
        let bikes_json_string = serde_json::to_string_pretty(&bikes)
            .map_err(|e| AssistantError::OpenAIError(e.to_string()))?;

        // Write the JSON data to a file in the specified folder_path
        let file_path = PathBuf::from(&self.folder_path).join("bikes.json");
        let mut file =
            File::create(file_path).map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        file.write_all(bikes_json_string.as_bytes())
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;

        Ok(())
    }
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
            // Transform DirEntry into PathBuf, handle errors
            let path = path
                .map_err(|e| AssistantError::DatabaseError(e.to_string()))?
                .path();
            // Proceed if the path is a file
            if path.is_file() {
                // Read file content, handle errors
                let file_content =
                    fs::read(&path).map_err(|e| AssistantError::OpenAIError(e.to_string()))?;
                // Extract and transform file name, handle errors
                let filename = path
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
                    .to_owned(); // Convert &str to String
                let part = Part::bytes(file_content)
                    .file_name(filename)
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
                    // Case when the HTTP request is successful and the status code indicates success
                    Ok(res) if res.status().is_success() => {
                        if let Ok(file_response) = res.json::<FileUploadResponse>().await {
                            self.files_info.push(FileInfo {
                                file_id: file_response.id,
                                file_name: file_response.filename, // Use the filename from the response
                            });
                        } else {
                            return Err(AssistantError::OpenAIError(
                                "Failed to parse response from OpenAI".to_string(),
                            ));
                        }
                    }
                    // Case when the HTTP request is successful but the status code is not a success
                    Ok(res) => {
                        // Attempt to read the error message from the response body
                        let error_message = res.text().await.unwrap_or_default();
                        // Return an error with the message from the response or a default message
                        return Err(AssistantError::OpenAIError(error_message));
                    }
                    // Case when the HTTP request itself fails
                    Err(e) => return Err(AssistantError::OpenAIError(e.to_string())),
                }
            }
        }
        // If all iterations complete without error, return Ok to indicate success
        Ok(())
    }
    /// create instruction text from the instruction file by replacing the {files_name} placeholders with the file_ids
    async fn create_instruction(&mut self) -> Result<(), AssistantError> {
        let mut instruction = fs::read_to_string(&self.instruction_file_path)
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        // Replace any placeholders in the instruction text that match the {file_name} with the file_id
        for file_info in &self.files_info {
            // Perform the replacement directly without checking for placeholder existence
            instruction =
                instruction.replace(&format!("{{{}}}", file_info.file_name), &file_info.file_id);
        }
        // Assign the modified prompt to the struct's field
        self.instruction = instruction;
        Ok(())
    }

    pub async fn delete(&mut self) -> Result<(), AssistantError> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        let client = Client::new();
        for file_info in &self.files_info {
            let response = client
                .delete(format!(
                    "https://api.openai.com/v1/files/{}",
                    file_info.file_id
                ))
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
        self.files_info.clear(); // Clear the files_info vector
        Ok(())
    }
    /// Returns a vector of stored file IDs.
    pub fn get_file_ids(&self) -> Vec<String> {
        self.files_info
            .iter()
            .map(|info| info.file_id.clone())
            .collect()
    }
}
/// A struct representing an OpenAI assistant.
/// The tools are currently hardcoded as a code_interpreter.
pub struct Assistant {
    pub id: String,
    name: String,
    model: String,
    instruction: String,
}
impl Assistant {
    /// create an OpenAI assistant and set the assistant's ID
    pub async fn initialize(&mut self) -> Result<(), AssistantError> {
        let client = Client::new();
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;

        let payload = json!({
            "instructions": self.instruction,
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

    pub async fn attach_files(&self, file_ids: Vec<String>) -> Result<(), AssistantError> {
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
    /// this overwrites the assistant's instruction a str
    pub async fn update_instruction(&mut self, instruction: &str) -> Result<(), AssistantError> {
        // Ensure the API key is set
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;

        let client = Client::new();

        // Prepare the payload with the new instruction
        let payload = json!({
            "instructions": instruction,
        });

        // Send the request to update the assistant
        let response = client
            .patch(&format!("https://api.openai.com/v1/assistants/{}", self.id))
            .header("Content-Type", "application/json")
            .header("OpenAI-Beta", "assistants=v1")
            .bearer_auth(&api_key)
            .json(&payload)
            .send()
            .await;

        // Handle the response
        match response {
            Ok(res) if res.status().is_success() => Ok(()),
            Ok(res) => {
                let error_message = res.text().await.unwrap_or_default();
                Err(AssistantError::OpenAIError(error_message))
            }
            Err(e) => Err(AssistantError::OpenAIError(e.to_string())),
        }
    }
}
/// scrape urls and upload the resulting files to OpenAI
pub async fn create_ressources(
    folder_path: &str,
    scrape_urls: Vec<String>,
    instruction_file_path: &str,
) -> Result<Ressources, AssistantError> {
    // Initialize the Files struct directly
    let mut files = Ressources {
        files_info: Vec::new(), // Use files_info to store FileInfo objects
        folder_path: folder_path.to_string(),
        scrape_urls, // Provided scrape URLs
        instruction_file_path: instruction_file_path.to_string(),
        instruction: String::new(),
    };
    // Get bikes from the database and save them to a JSON file
    files.bikes_db().await?;
    // Scrape the context from the provided URLs
    files.scrape_context().await?;
    // Upload the scraped files to OpenAI
    files.upload_files().await?;
    // Create the instruction text by replacing the placeholders with the file IDs
    files.create_instruction().await?;
    Ok(files)
}
pub async fn create_assistant(
    assistant_name: &str,
    model: &str,
    ressources: Ressources,
) -> Result<Assistant, AssistantError> {
    let mut assistant = Assistant {
        id: String::new(),
        name: assistant_name.to_string(),
        model: model.to_string(),
        instruction: ressources.instruction.clone(),
    };
    // Initialize the assistant by creating it on the OpenAI platform
    assistant.initialize().await?;
    info!("Assistant created with ID: {}", assistant.id);
    // Attach the uploaded files to the assistant using the file IDs from the Files struct
    assistant.attach_files(ressources.get_file_ids()).await?;
    Ok(assistant)
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
                let message_list_response = res.json::<ChatMessageList>().await.map_err(|_| {
                    AssistantError::OpenAIError("Failed to parse response from OpenAI".to_string())
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
                    if let Some(last_message) =
                        simplified_messages.iter().max_by_key(|m| m.created_at)
                    {
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
        let payload = UserMessage {
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
    pub pool: Pool<MySql>,
}

impl DB {
    /// Creates a new database connection pool use dev DB.
    pub async fn create_db_pool() -> Result<Self, AssistantError> {
        let database_url = env::var("DATABASE_URL").map_err(|_| {
            AssistantError::DatabaseError("DATABASE_URL environment variable not set".to_string())
        })?;
        let pool: Pool<MySql> = MySqlPoolOptions::new()
            .connect(&database_url)
            .await
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        Ok(DB { pool })
    }
    pub async fn get_chat_id(&self, user_id: &str) -> Result<Option<String>, AssistantError> {
        let result = sqlx::query!(
            "SELECT id FROM buycycle_chatbot.chats WHERE user_id = ? ORDER BY created_at DESC LIMIT 1",
            user_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        Ok(result.map(|row| row.id)) // row.id is a String, but result is an Option due to fetch_optional
    }
    /// Saves the chat ID into the database.
    pub async fn save_chat_id(&self, user_id: &str, chat_id: &str) -> Result<(), AssistantError> {
        sqlx::query!(
            "INSERT INTO buycycle_chatbot.chats (id, user_id) VALUES (?, ?)",
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
        role: &str,
        message: &str,
    ) -> Result<(), AssistantError> {
        sqlx::query!(
            "INSERT INTO buycycle_chatbot.messages (chat_id, role, content) VALUES (?, ?, ?)",
            chat_id,
            role,
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
                let run_status_response = res.json::<RunResponse>().await.map_err(|_| {
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

// Define a struct that represents the form data.
#[derive(Deserialize)]
pub struct AssistantChatForm {
    pub user_id: String,
    pub message: String,
}

// Handles chat interactions with an OpenAI assistant using form data.
pub async fn assistant_chat_handler_form(
    Extension(db_pool): Extension<MySqlPool>,
    Extension(assistant_id): Extension<Arc<RwLock<String>>>, // Change this line
    AxumForm(assistant_chat_form): AxumForm<AssistantChatForm>, // Use Form extractor here
) -> Result<Json<AssistantChatResponse>, AssistantError> {
    let db = DB { pool: db_pool };
    let user_id = &assistant_chat_form.user_id;
    let message = &assistant_chat_form.message;
    // log user_id and message
    info!("chat_id: {}, message: {}", user_id, message);
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
    // log user_id and message
    info!("chat_id: {}", chat_id);
    // Save the user's message to the database
    db.save_message_to_db(&chat_id.to_string(), "user", message)
        .await?;
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
    // Acquire a read lock when you only need to read the value
    let assistant_id_read_guard = assistant_id.read().await;
    let assistant_id_string = assistant_id_read_guard.clone();

    run.create(&chat.id, &assistant_id_string).await?;
    // Check the status of the run until it's completed or a timeout occurs
    let start_time = std::time::Instant::now();
    while start_time.elapsed().as_secs() < TIMEOUT_DURATION {
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
    // If run is not finished save and return a sorry message with the role "error"
    if run.status != "completed" {
        // Save the error message to the database
        db.save_message_to_db(
            &chat_id,
            "error",
            "Sorry I am currently facing some technical issues, please try again.",
        )
        .await?;
        // Return the error message as part of the response, wrapped in a vector
        return Ok(Json(AssistantChatResponse {
            messages: vec![SimplifiedMessage {
                created_at: Utc::now().timestamp(),
                role: "error".to_string(),
                text: "Sorry I am currently facing some technical issues, please try again."
                    .to_string(),
            }],
        }));
    }
    // Retrieve the last message from the conversation, which should be the assistant's response
    chat.get_messages(true).await?;
    if let Some(last_message) = chat.messages.last() {
        // Save the assistant message to the database
        db.save_message_to_db(&chat_id, "assistant", &last_message.text)
            .await?;
    }
    // Return the updated conversation history including the assistant's response
    Ok(Json(AssistantChatResponse {
        messages: chat.messages,
    }))
}
