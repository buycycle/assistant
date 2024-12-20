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
use std::sync::Arc;
use tokio::sync::RwLock;

use reqwest::{multipart::Form, multipart::Part, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use serde_json::json;
use std::env;

use sqlx::Pool;
use sqlx::{mysql::MySqlPoolOptions, FromRow, MySql, MySqlPool};

// Define a constant for the timeout duration of assistant response
const TIMEOUT_DURATION: u64 = 100;

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
    db_pool: Pool<MySql>,
    vector_store_id: String,
    pub files_info_file_search: Vec<FileInfo>,
    pub files_info_code_interpreter: Vec<FileInfo>,
    folder_path_file_search: String,
    folder_path_code_interpreter: String,
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
    pub fn new(
        db_pool: Pool<MySql>,
        folder_path_file_search: String,
        folder_path_code_interpreter: String,
        scrape_urls: Vec<String>,
        instruction_file_path: String,
    ) -> Self {
        Ressources {
            db_pool,
            vector_store_id: String::new(),
            files_info_file_search: Vec::new(),
            files_info_code_interpreter: Vec::new(),
            folder_path_file_search,
            folder_path_code_interpreter,
            scrape_urls,
            instruction_file_path,
            instruction: String::new(),
        }
    }
    pub async fn bikes_db(&self) -> Result<(), AssistantError> {
        // Define the query
        let main_query = "
            SELECT bikes.slug as slug,
                   bike_categories.slug as category,
                   bike_additional_infos.frame_size as frame_size,
                   bike_additional_infos.rider_height_min as rider_height_min,
                   bike_additional_infos.rider_height_max as rider_height_max,
                   bikes.price,
                   bikes.color
            FROM buycycle_2023_01_20.bikes
            JOIN buycycle_2023_01_20.bike_additional_infos ON bikes.id = bike_additional_infos.bike_id
            JOIN buycycle_2023_01_20.bike_categories ON bikes.bike_category_id = bike_categories.id
            WHERE bikes.status = 'active'
            LIMIT 100
        ";
        // Execute the query using sqlx
        let bikes: Vec<Bike> = sqlx::query_as(main_query)
            .fetch_all(&self.db_pool)
            .await
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        // Serialize the bikes to JSON
        let bikes_json_string = serde_json::to_string_pretty(&bikes)
            .map_err(|e| AssistantError::OpenAIError(e.to_string()))?;
        // Write the JSON data to a file in the specified folder_path
        let folder_path = PathBuf::from(&self.folder_path_code_interpreter);
        if !folder_path.exists() {
            fs::create_dir_all(&folder_path)
                .expect("Failed to create folder_path_code_interpreter");
        };
        let file_path = PathBuf::from(&self.folder_path_code_interpreter).join("bikes.json");
        let mut file =
            File::create(file_path).map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        file.write_all(bikes_json_string.as_bytes())
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        Ok(())
    }
    pub async fn upload_files_search(&mut self) -> Result<(), AssistantError> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        let client = Client::new();
        let paths = fs::read_dir(Path::new(&self.folder_path_file_search))
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
                    .header("OpenAI-Beta", "assistants=v2")
                    .bearer_auth(&api_key)
                    .multipart(form)
                    .send()
                    .await;
                match response {
                    // Case when the HTTP request is successful and the status code indicates success
                    Ok(res) if res.status().is_success() => {
                        if let Ok(file_response) = res.json::<FileUploadResponse>().await {
                            self.files_info_file_search.push(FileInfo {
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
    pub async fn upload_code_interpreter(&mut self) -> Result<(), AssistantError> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        let client = Client::new();
        let paths = fs::read_dir(Path::new(&self.folder_path_code_interpreter))
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
                    .header("OpenAI-Beta", "assistants=v2")
                    .bearer_auth(&api_key)
                    .multipart(form)
                    .send()
                    .await;
                match response {
                    // Case when the HTTP request is successful and the status code indicates success
                    Ok(res) if res.status().is_success() => {
                        if let Ok(file_response) = res.json::<FileUploadResponse>().await {
                            self.files_info_code_interpreter.push(FileInfo {
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
    pub async fn create_vector_store(&mut self) -> Result<(), AssistantError> {
        // Extract file_ids from files_info_file_search
        let file_ids: Vec<String> = self
            .files_info_file_search
            .iter()
            .map(|info| info.file_id.clone())
            .collect();
        // Prepare the JSON payload with file_ids
        let payload = json!({
            "file_ids": file_ids,
            "name": "assistant_vector_store",
            "expires_after": {
                "anchor": "last_active_at",
                "days": 7
            },

        });
        // Get the OpenAI API key from the environment
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        // Create an HTTP client
        let client = Client::new();
        // Make the POST request to create the vector store
        let response = client
            .post("https://api.openai.com/v1/vector_stores")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .header("OpenAI-Beta", "assistants=v2")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AssistantError::OpenAIError(e.to_string()))?;
        // Check the response status and handle accordingly
        if response.status().is_success() {
            let response_body = response
                .json::<serde_json::Value>()
                .await
                .map_err(|e| AssistantError::OpenAIError(e.to_string()))?;
            // Extract the vector_store_id from the response
            if let Some(vector_store_id) = response_body.get("id").and_then(|id| id.as_str()) {
                self.vector_store_id = vector_store_id.to_string();
            } else {
                return Err(AssistantError::OpenAIError(
                    "Failed to get vector_store_id from response".to_string(),
                ));
            }
        } else {
            // Handle non-successful response
            let error_message = response.text().await.unwrap_or_default();
            return Err(AssistantError::OpenAIError(error_message));
        }
        Ok(())
    }
    /// create instruction text from the instruction file by replacing the {files_name} placeholders with the file_ids
    async fn create_instruction(&mut self) -> Result<(), AssistantError> {
        let mut instruction = fs::read_to_string(&self.instruction_file_path)
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        // Replace any placeholders in the instruction text that match the {file_name} with the file_id
        for file_info in self
            .files_info_file_search
            .iter()
            .chain(self.files_info_code_interpreter.iter())
        {
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
        for file_info in self
            .files_info_file_search
            .iter()
            .chain(self.files_info_code_interpreter.iter())
        {
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
        self.files_info_file_search.clear(); // Clear the files_info vector
        self.files_info_code_interpreter.clear(); // Clear the files_info vector
        Ok(())
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
    pub async fn initialize(
        &mut self,
        files_info_code_interpreter: Vec<FileInfo>,
        vector_store_id: String,
    ) -> Result<(), AssistantError> {
        let client = Client::new();
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        // Extract file_ids from the FileInfo objects
        let file_ids_code_interpreter: Vec<String> = files_info_code_interpreter
            .into_iter()
            .map(|file_info| file_info.file_id)
            .collect();
        // Construct the payload with the extracted file_ids and vector_store_id
        let payload = json!({
            "instructions": self.instruction,
            "name": self.name,
            "tools": [
                {"type": "file_search"},
                {"type": "code_interpreter"}
            ],
            "tool_resources": {
                "code_interpreter": {
                    "file_ids": file_ids_code_interpreter
                },
                "file_search": {
                    "vector_store_ids": [vector_store_id]
                }
            },
            "model": self.model,
        });
        let response = client
            .post("https://api.openai.com/v1/assistants")
            .header("OpenAI-Beta", "assistants=v2")
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
    pub async fn initialize_with_tools(
        &mut self,
        files_info_code_interpreter: Vec<FileInfo>,
        vector_store_id: String,
    ) -> Result<(), AssistantError> {
        let client = Client::new();
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        let file_ids_code_interpreter: Vec<String> = files_info_code_interpreter
            .into_iter()
            .map(|file_info| file_info.file_id)
            .collect();
        let payload = json!({
            "instructions": self.instruction,
            "name": self.name,
            "tools": get_tool_definition(),
            "tool_resources": {
                "code_interpreter": {
                    "file_ids": file_ids_code_interpreter
                },
                "file_search": {
                    "vector_store_ids": [vector_store_id]
                }
            },
            "model": self.model,
        });
        let response = client
            .post("https://api.openai.com/v1/assistants")
            .header("OpenAI-Beta", "assistants=v2")
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
            .header("OpenAI-Beta", "assistants=v2")
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
            .header("OpenAI-Beta", "assistants=v2")
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
    db_pool: Pool<MySql>,
    folder_path_file_search: &str,
    folder_path_code_interpreter: &str,
    scrape_urls: Vec<String>,
    instruction_file_path: &str,
) -> Result<Ressources, AssistantError> {
    // Initialize the Files struct directly
    let mut files = Ressources {
        db_pool: db_pool,
        vector_store_id: String::new(),
        files_info_file_search: Vec::new(), // Use files_info to store FileInfo objects
        files_info_code_interpreter: Vec::new(), // Use files_info to store FileInfo objects
        folder_path_file_search: folder_path_file_search.to_string(),
        folder_path_code_interpreter: folder_path_code_interpreter.to_string(),
        scrape_urls, // Provided scrape URLs
        instruction_file_path: instruction_file_path.to_string(),
        instruction: String::new(),
    };
    // Get bikes from the database and save them to a JSON file
    files.bikes_db().await?;
    files.upload_files_search().await?;
    files.upload_code_interpreter().await?;
    files.create_vector_store().await?;
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
    assistant
        .initialize_with_tools(
            ressources.files_info_code_interpreter,
            ressources.vector_store_id,
        )
        .await?;
    info!("Assistant created with ID: {}", assistant.id);
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
            .header("OpenAI-Beta", "assistants=v2")
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
            .header("OpenAI-Beta", "assistants=v2")
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
            .header("OpenAI-Beta", "assistants=v2")
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

pub struct DB;
impl DB {
    /// Creates a new database connection pool using the provided database URL.
    pub async fn create_pool(database_url: &str) -> Result<Pool<MySql>, AssistantError> {
        let pool: Pool<MySql> = MySqlPoolOptions::new()
            .connect(database_url)
            .await
            .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        Ok(pool)
    }
}

pub struct LOG {
    db_pool: Pool<MySql>,
}
impl LOG {
   /// Retrieves the chat ID for a given user ID from the database.
    pub async fn get_chat_id(&self, user_id: &str) -> Result<Option<String>, AssistantError> {
        let result = sqlx::query!(
            "SELECT id FROM buycycle_chatbot.chats WHERE user_id = ? ORDER BY created_at DESC LIMIT 1",
            user_id
        )
        .fetch_optional(&self.db_pool)
        .await
        .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        Ok(result.map(|row| row.id))
    }
    /// Saves a new chat ID for a user into the database.
    pub async fn save_chat_id(&self, user_id: &str, chat_id: &str) -> Result<(), AssistantError> {
        sqlx::query!(
            "INSERT INTO buycycle_chatbot.chats (id, user_id) VALUES (?, ?)",
            chat_id,
            user_id
        )
        .execute(&self.db_pool)
        .await
        .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        Ok(())
    }
    /// Saves a message to the database for a given chat ID.
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
        .execute(&self.db_pool)
        .await
        .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;
        Ok(())
    }
}
struct Run {
    id: String,
    status: String,
    required_action: Option<RequiredAction>,
}
#[derive(Deserialize, Debug)]
struct RequiredAction {
    submit_tool_outputs: Option<SubmitToolOutputs>,
}
#[derive(Deserialize, Debug)]
struct SubmitToolOutputs {
    tool_calls: Vec<ToolCall>,
}
#[derive(Deserialize, Debug)]
struct ToolCall {
    id: String,
    function: FunctionCall,
    #[serde(rename = "type")]
    call_type: String,
}
#[derive(Deserialize, Debug)]
struct FunctionCall {
    arguments: Value,
    name: String,
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
            .header("OpenAI-Beta", "assistants=v2")
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
    pub async fn get_response(&mut self, chat_id: &str) -> Result<(), AssistantError> {
        let client = Client::new();
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        let response = client
            .get(&format!(
                "https://api.openai.com/v1/threads/{}/runs/{}",
                chat_id, self.id
            ))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("OpenAI-Beta", "assistants=v2")
            .send()
            .await;
        match response {
            Ok(res) if res.status().is_success() => {
                // Parse the response into a serde_json::Value
                let run_response: serde_json::Value = res.json().await.map_err(|_| {
                    AssistantError::OpenAIError("Failed to parse response from OpenAI".to_string())
                })?;
                log::debug!("Run response: {:?}", run_response);
                // Extract the status
                if let Some(status) = run_response.get("status").and_then(|s| s.as_str()) {
                    self.status = status.to_string();
                }
                // Extract and parse the required_action if present
                if let Some(required_action_value) = run_response.get("required_action") {
                    self.required_action = serde_json::from_value(required_action_value.clone())
                        .map_err(|_| {
                            AssistantError::OpenAIError(
                                "Failed to parse RequiredAction".to_string(),
                            )
                        })?;
                } else {
                    self.required_action = None;
                }
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
    pub async fn submit_tool_outputs(
        &self,
        chat_id: &str,
        tool_outputs: Vec<serde_json::Value>,
    ) -> Result<(), AssistantError> {
        let client = Client::new();
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AssistantError::OpenAIError("OPENAI_API_KEY not set".to_string()))?;
        // Construct the payload with the tool outputs
        let payload = json!({
            "tool_outputs": tool_outputs
        });
        // Send the request to submit the tool outputs
        let response = client
            .post(&format!(
                "https://api.openai.com/v1/threads/{}/runs/{}/submit_tool_outputs",
                chat_id, self.id
            ))
            .header("Content-Type", "application/json")
            .bearer_auth(&api_key)
            .header("OpenAI-Beta", "assistants=v2")
            .json(&payload)
            .send()
            .await;
        // Handle the response
        match response {
            Ok(res) if res.status().is_success() => {
                log::info!(
                    "Tool outputs submitted successfully for run ID: {}",
                    self.id
                );
                Ok(())
            }
            Ok(res) => {
                let error_message = res.text().await.unwrap_or_default();
                log::error!("Failed to submit tool outputs: {}", error_message);
                Err(AssistantError::OpenAIError(error_message))
            }
            Err(e) => {
                log::error!("Failed to send request to OpenAI: {}", e);
                Err(AssistantError::OpenAIError(format!(
                    "Failed to send request to OpenAI: {}",
                    e
                )))
            }
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
    Extension(db_pool_buycycle): Extension<MySqlPool>,
    Extension(db_pool_log): Extension<MySqlPool>,
    Extension(assistant_id): Extension<Arc<RwLock<String>>>,
    AxumForm(assistant_chat_form): AxumForm<AssistantChatForm>,
) -> Result<Json<AssistantChatResponse>, AssistantError> {
    let log = LOG {
        db_pool: db_pool_log.clone(),
    };
    let user_id = &assistant_chat_form.user_id;
    let message = &assistant_chat_form.message;
    // Initialize chat or get existing chat_id
    let chat_id = match log.get_chat_id(user_id).await? {
        Some(id) => id,
        None => {
            let mut chat = Chat {
                id: String::new(),
                messages: Vec::new(),
            };
            chat.initialize().await?;
            let new_chat_id = chat.id;
            log.save_chat_id(user_id, &new_chat_id).await?;
            new_chat_id
        }
    };
    // Log user_id and message
    info!("chat_id: {}, message: {}", chat_id, message);
    // Save the user's message to the database
    log.save_message_to_db(&chat_id.to_string(), "user", message)
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
        required_action: None,
    };
    // Acquire a read lock when you only need to read the value
    let assistant_id_read_guard = assistant_id.read().await;
    let assistant_id_string = assistant_id_read_guard.clone();
    run.create(&chat.id, &assistant_id_string).await?;
    // Check the status of the run until it's completed or a timeout occurs
    let start_time = std::time::Instant::now();
    while start_time.elapsed().as_secs() < TIMEOUT_DURATION {
        // Log the current status of the run
        log::info!("Checking run status for chat ID: {}", chat.id);
        run.get_response(&chat.id).await?;
        if run.status == "requires_action" {
            log::info!("Run requires action for chat ID: {}", chat.id);
            if let Some(required_action) = &run.required_action {
                if let Some(submit_tool_outputs) = &required_action.submit_tool_outputs {
                    for tool_call in &submit_tool_outputs.tool_calls {
                        log::info!("Processing tool call with ID: {}", tool_call.id);
                        if tool_call.function.name == "get_order_status_dummy" {
                            // Existing logic for get_order_status_dummy
                            if let Some(arguments_str) = tool_call.function.arguments.as_str() {
                                if let Ok(arguments_json) =
                                    serde_json::from_str::<serde_json::Value>(arguments_str)
                                {
                                    if let Some(order_id) =
                                        arguments_json.get("order_id").and_then(|v| v.as_str())
                                    {
                                        log::info!(
                                            "Fetching order status for order ID: {}",
                                            order_id
                                        );
                                        let order_status = get_order_status_dummy(
                                            &db_pool_buycycle,
                                            user_id,
                                            order_id,
                                        )
                                        .await?;
                                        log::info!(
                                            "Order status for user ID {}, order ID {}: {:?}",
                                            user_id,
                                            order_id,
                                            order_status
                                        );
                                        let tool_output = json!({
                                            "tool_call_id": tool_call.id,
                                            "output": order_status.unwrap_or("Unknown order ID".to_string())
                                        });
                                        log::info!(
                                            "Submitting tool output {} for tool call ID: {}",
                                            tool_output,
                                            tool_call.id
                                        );
                                        run.submit_tool_outputs(&chat.id, vec![tool_output])
                                            .await?;
                                    } else {
                                        log::error!("Order ID is not a string or not found for tool call ID: {}", tool_call.id);
                                    }
                                } else {
                                    log::error!(
                                        "Failed to parse arguments for tool call ID: {}",
                                        tool_call.id
                                    );
                                }
                            } else {
                                log::error!(
                                    "Arguments are not a string for tool call ID: {}",
                                    tool_call.id
                                );
                            }
                        } else if tool_call.function.name == "get_orders" {
                            // New logic for get_orders
                            if let Some(arguments_str) = tool_call.function.arguments.as_str() {
                                if let Ok(arguments_json) =
                                    serde_json::from_str::<serde_json::Value>(arguments_str)
                                {
                                    log::info!("Fetching orders for user ID: {}", user_id);
                                    let orders = get_orders(user_id, &db_pool_buycycle).await?;
                                    log::info!("Orders for user ID {}: {:?}", user_id, orders);
                                    let tool_output = json!({
                                        "tool_call_id": tool_call.id,
                                        "output": orders.unwrap_or("No orders found".to_string())
                                    });
                                    log::info!(
                                        "Submitting tool output {} for tool call ID: {}",
                                        tool_output,
                                        tool_call.id
                                    );
                                    run.submit_tool_outputs(&chat.id, vec![tool_output]).await?;
                                } else {
                                    log::error!(
                                        "Failed to parse arguments for tool call ID: {}",
                                        tool_call.id
                                    );
                                }
                            } else {
                                log::error!(
                                    "Arguments are not a string for tool call ID: {}",
                                    tool_call.id
                                );
                            }
                        }
                    }
                }
            }
        } else if run.status == "completed" {
            info!("Run completed, status: {}", run.status);
            break;
        }
        info!("Run not completed, current status: {}", run.status);
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
    // If run is not finished, save and return a sorry message with the role "error"
    if run.status != "completed" {
        log.save_message_to_db(
            &chat_id,
            "error",
            "Sorry I am currently facing some technical issues, please try again.",
        )
        .await?;
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
        log.save_message_to_db(&chat_id, "assistant", &last_message.text)
            .await?;
    }
    // Return the updated conversation history including the assistant's response
    Ok(Json(AssistantChatResponse {
        messages: chat.messages,
    }))
}
async fn get_order_status_dummy(
    _db_pool: &MySqlPool,
    _user_id: &str,
    _order_id: &str,
) -> Result<Option<String>, AssistantError> {
    // Dummy function that returns a fixed delivery date
    let order_status = "delivered";
    Ok(Some(order_status.to_string()))
}

async fn get_authorization_token(
    db_pool: &MySqlPool,
    user_id: &str,
) -> Result<Option<String>, AssistantError> {
    info!("Received user_id: {}", user_id);
    let user_id_int: i32 = user_id
        .parse()
        .map_err(|e| AssistantError::DatabaseError(format!("Failed to parse user_id: {}", e)))?;
    let main_query = "
        SELECT custom_auth_token FROM buycycle_2023_01_20.users WHERE id = ?
    ";
    //xx check why this is necesarry
    let database_url_buycycle =
        env::var("DATABASE_URL_BUYCYCLE").expect("DATABASE_URL must be set");
    // Create a new database connection pool
    let db_pool_buycycle = match DB::create_pool(&database_url_buycycle).await {
        Ok(pool) => pool,
        Err(e) => {
            log::error!("Failed to create database pool buycycle: {:?}", e);
            std::process::exit(1);
        }
    };
    let authorization_token: Option<String> = sqlx::query_scalar(main_query)
        .bind(user_id_int)
        .fetch_optional(&db_pool_buycycle)
        .await
        .map_err(|e| AssistantError::DatabaseError(e.to_string()))?;

    Ok(authorization_token)
}
async fn get_orders(user_id: &str, db_pool: &MySqlPool) -> Result<Option<String>, AssistantError> {
    let x_proxy_authorization = env::var("X_PROXY_AUTHORIZATION").map_err(|_| {
        AssistantError::DatabaseError(
            "X_PROXY_AUTHORIZATION environment variable not set".to_string(),
        )
    })?;

    // Get the authorization token
    let authorization_token = get_authorization_token(db_pool, user_id).await?;

    // Check if the authorization token is available
    let token = match authorization_token {
        Some(token) => token,
        None => {
            return Err(AssistantError::OpenAIError(
                "Authorization token not found".to_string(),
            ))
        }
    };
    // Define the API endpoint
    let api_url = "https://api.buycycle.com/en/api/v3/account/orders?offset=0&limit=100&type=sale";
    // Create a new HTTP client
    let client = Client::new();
    // Send the GET request to the API
    let response = client
        .get(api_url)
        .header("X-Custom-Authorization", token)
        .header("Content-Type", "application/json")
        .header("X-Proxy-Authorization", x_proxy_authorization)
        .send()
        .await
        .map_err(|e| AssistantError::OpenAIError(e.to_string()))?;
    // Check if the response is successful
    if response.status().is_success() {
        // Parse the response body as a string
        let order_status = response
            .text()
            .await
            .map_err(|e| AssistantError::OpenAIError(e.to_string()))?;
        Ok(Some(order_status))
    } else {
        // Handle non-successful response
        let error_message = response.text().await.unwrap_or_default();
        Err(AssistantError::OpenAIError(error_message))
    }
}
pub fn get_tool_definition() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "get_orders",
                "description": "get the list of orders",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "user_id": {
                            "type": "string",
                            "description": "The ID of the user"
                        }
                    },
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_order_status",
                "description": "Get the status of an order by the order id",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "order_id": {
                            "type": "string",
                            "description": "The ID of the order"
                        }
                    },
                    "required": ["order_id"]
                }
            }
        }
    ])
}
