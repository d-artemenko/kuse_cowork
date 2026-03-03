use crate::agent::AgentEvent;
use crate::claude::ClaudeClient;
use crate::database::{Conversation, Database, Message, Settings, Task, TaskMessage};
use crate::mcp::{MCPManager, MCPServerConfig, MCPServerStatus, MCPToolCall, MCPToolResult};
use crate::moltis_client::{MoltisClient, MoltisClientConfig, MoltisClientError};
use crate::skills::{get_available_skills, SkillMetadata};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tauri::{command, Emitter, State, Window};
use tokio::sync::Mutex;

pub struct AppState {
    pub db: Arc<Database>,
    pub claude_client: Mutex<Option<ClaudeClient>>,
    pub mcp_manager: Arc<MCPManager>,
}

#[derive(Debug, Serialize)]
pub struct CommandError {
    message: String,
}

impl From<crate::database::DbError> for CommandError {
    fn from(e: crate::database::DbError) -> Self {
        CommandError {
            message: e.to_string(),
        }
    }
}

impl From<crate::claude::ClaudeError> for CommandError {
    fn from(e: crate::claude::ClaudeError) -> Self {
        CommandError {
            message: e.to_string(),
        }
    }
}

impl From<MoltisClientError> for CommandError {
    fn from(e: MoltisClientError) -> Self {
        CommandError {
            message: e.to_string(),
        }
    }
}

fn build_moltis_client(settings: &Settings) -> Result<MoltisClient, CommandError> {
    if settings.moltis_server_url.trim().is_empty() {
        return Err(CommandError {
            message: "Moltis server URL not configured".to_string(),
        });
    }
    let api_key = if settings.moltis_api_key.trim().is_empty() {
        None
    } else {
        Some(settings.moltis_api_key.clone())
    };
    let mut cfg = MoltisClientConfig::new(settings.moltis_server_url.clone(), api_key);
    cfg.timeout = Duration::from_secs(45);
    Ok(MoltisClient::new(cfg))
}

fn moltis_model_override(settings: &Settings) -> Option<String> {
    let raw_model = settings.model.trim();
    if raw_model.is_empty() {
        return None;
    }

    // If a Moltis-style model id is already provided, pass it through.
    if raw_model.contains("::") {
        return Some(raw_model.to_string());
    }

    // Kuse model ids are typically provider-less (e.g. "gpt-5", "claude-sonnet-...").
    // Moltis expects "provider::model" (or "openrouter::provider/model").
    let provider = settings.get_provider();
    match provider.as_str() {
        "anthropic" | "openai" | "google" | "minimax" => Some(format!("{provider}::{raw_model}")),
        "openrouter" => Some(format!("openrouter::{raw_model}")),
        _ => Some(raw_model.to_string()),
    }
}

fn is_moltis_model_not_found_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    (lower.contains("model") && lower.contains("not found")) || lower.contains("unknown model")
}

// Platform command
#[command]
pub fn get_platform() -> String {
    #[cfg(target_os = "macos")]
    return "darwin".to_string();

    #[cfg(target_os = "windows")]
    return "windows".to_string();

    #[cfg(target_os = "linux")]
    return "linux".to_string();

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    return "unknown".to_string();
}

// Settings commands
#[command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> Result<Settings, CommandError> {
    let settings = state.db.get_settings()?;
    println!(
        "[get_settings] api_key length from db: {}",
        settings.api_key.len()
    );
    Ok(settings)
}

#[command]
pub async fn save_settings(
    state: State<'_, Arc<AppState>>,
    settings: Settings,
) -> Result<(), CommandError> {
    println!("[save_settings] model: {}", settings.model);
    println!("[save_settings] base_url: {}", settings.base_url);
    println!("[save_settings] api_key length: {}", settings.api_key.len());
    // Show first and last 10 chars for debugging
    if settings.api_key.len() > 20 {
        println!(
            "[save_settings] api_key preview: {}...{}",
            &settings.api_key[..10],
            &settings.api_key[settings.api_key.len() - 10..]
        );
    }

    state.db.save_settings(&settings)?;

    // Moltis-only mode does not keep direct provider clients hot.
    let mut client = state.claude_client.lock().await;
    *client = None;

    Ok(())
}

#[command]
pub async fn test_connection(state: State<'_, Arc<AppState>>) -> Result<String, CommandError> {
    let settings = state.db.get_settings()?;
    // Backward-compatible alias for legacy UI code; Moltis is the only connection path.
    test_moltis_connection_with_settings(&settings).await
}

#[command]
pub async fn test_moltis_connection(
    state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    let settings = state.db.get_settings()?;
    test_moltis_connection_with_settings(&settings).await
}

#[derive(Debug, Serialize)]
pub struct MoltisConnectionStatus {
    pub ok: bool,
    pub version: Option<String>,
    pub protocol: Option<u32>,
    pub server_url: String,
    pub auth_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[command]
pub async fn get_moltis_connection_status(
    state: State<'_, Arc<AppState>>,
) -> Result<MoltisConnectionStatus, CommandError> {
    let settings = state.db.get_settings()?;
    Ok(moltis_connection_status_with_settings(&settings).await)
}

#[command]
pub async fn moltis_health(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, CommandError> {
    let settings = state.db.get_settings()?;
    moltis_health_with_settings(&settings).await
}

#[derive(Debug, Deserialize)]
pub struct MoltisCallRequest {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[command]
pub async fn moltis_call(
    state: State<'_, Arc<AppState>>,
    request: MoltisCallRequest,
) -> Result<serde_json::Value, CommandError> {
    let settings = state.db.get_settings()?;
    moltis_call_with_settings(&settings, request).await
}

#[command]
pub async fn send_chat_message_via_moltis(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    content: String,
) -> Result<String, CommandError> {
    let settings = state.db.get_settings()?;
    send_chat_message_via_moltis_with_db(&state.db, &settings, &conversation_id, &content).await
}

async fn test_moltis_connection_with_settings(settings: &Settings) -> Result<String, CommandError> {
    let status = moltis_connection_status_with_settings(settings).await;
    if !status.ok {
        return Err(CommandError {
            message: status
                .error
                .unwrap_or_else(|| "Moltis connection failed".to_string()),
        });
    }
    let version = status.version.unwrap_or_else(|| "unknown".to_string());
    let protocol = status.protocol.unwrap_or_default();
    Ok(format!("success (version={version}, protocol={protocol})"))
}

async fn moltis_connection_status_with_settings(settings: &Settings) -> MoltisConnectionStatus {
    let auth_mode = if settings.moltis_api_key.trim().is_empty() {
        "none".to_string()
    } else {
        "bearer".to_string()
    };
    let server_url = settings.moltis_server_url.trim().to_string();

    let client = match build_moltis_client(settings) {
        Ok(client) => client,
        Err(err) => {
            return MoltisConnectionStatus {
                ok: false,
                version: None,
                protocol: None,
                server_url,
                auth_mode,
                error: Some(err.message),
            }
        }
    };

    let health = match client.health().await {
        Ok(health) => health,
        Err(err) => {
            return MoltisConnectionStatus {
                ok: false,
                version: None,
                protocol: None,
                server_url,
                auth_mode,
                error: Some(format_moltis_error(&err)),
            }
        }
    };

    let hello = match client.check_ws_connection().await {
        Ok(hello) => hello,
        Err(err) => {
            return MoltisConnectionStatus {
                ok: false,
                version: health
                    .get("version")
                    .and_then(serde_json::Value::as_str)
                    .map(ToString::to_string),
                protocol: None,
                server_url,
                auth_mode,
                error: Some(format_moltis_error(&err)),
            }
        }
    };

    let version = health
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string);
    let protocol = hello
        .get("protocol")
        .and_then(serde_json::Value::as_u64)
        .map(|p| p as u32);

    MoltisConnectionStatus {
        ok: true,
        version,
        protocol,
        server_url,
        auth_mode,
        error: None,
    }
}

fn format_moltis_error(err: &MoltisClientError) -> String {
    match err {
        MoltisClientError::UnsupportedScheme(scheme) => {
            format!("Unsupported Moltis URL scheme '{scheme}'. Use http/https/ws/wss.")
        }
        MoltisClientError::InvalidUrl(_) => "Invalid Moltis server URL".to_string(),
        MoltisClientError::Http(http) => match http.status() {
            Some(reqwest::StatusCode::UNAUTHORIZED) | Some(reqwest::StatusCode::FORBIDDEN) => {
                "Moltis authentication rejected (unauthorized)".to_string()
            }
            _ => format!("Moltis HTTP request failed: {http}"),
        },
        MoltisClientError::Rpc { code, message } => {
            if code.eq_ignore_ascii_case("UNAUTHORIZED")
                || message.to_ascii_lowercase().contains("unauthorized")
            {
                "Moltis authentication rejected (unauthorized)".to_string()
            } else {
                format!("Moltis RPC error [{code}]: {message}")
            }
        }
        MoltisClientError::ProtocolMismatch { server, min, max } => {
            format!("Moltis protocol mismatch: server={server}, desktop supports {min}..{max}")
        }
        other => other.to_string(),
    }
}

async fn moltis_health_with_settings(
    settings: &Settings,
) -> Result<serde_json::Value, CommandError> {
    let client = build_moltis_client(settings)?;
    client.health().await.map_err(Into::into)
}

async fn moltis_call_with_settings(
    settings: &Settings,
    request: MoltisCallRequest,
) -> Result<serde_json::Value, CommandError> {
    if request.method.trim().is_empty() {
        return Err(CommandError {
            message: "Method name is required".to_string(),
        });
    }
    let client = build_moltis_client(settings)?;
    client
        .call(&request.method, request.params)
        .await
        .map_err(Into::into)
}

async fn send_chat_message_via_moltis_with_db(
    db: &Database,
    settings: &Settings,
    conversation_id: &str,
    content: &str,
) -> Result<String, CommandError> {
    let client = build_moltis_client(settings)?;

    // Persist user message locally before dispatching to Moltis.
    let existing_messages = db.get_messages(conversation_id)?;
    let user_msg_id = uuid::Uuid::new_v4().to_string();
    db.add_message(&user_msg_id, conversation_id, "user", content)?;

    let session_key = format!("kuse:{conversation_id}");
    let model_override = moltis_model_override(settings);
    let reply = match client
        .chat_send_and_wait(&session_key, content, model_override.as_deref())
        .await
    {
        Ok(reply) => reply,
        Err(MoltisClientError::Rpc { message, .. })
            if model_override.is_some() && is_moltis_model_not_found_error(&message) =>
        {
            client
                .chat_send_and_wait(&session_key, content, None)
                .await?
        }
        Err(err) => return Err(err.into()),
    };

    let assistant_msg_id = uuid::Uuid::new_v4().to_string();
    db.add_message(&assistant_msg_id, conversation_id, "assistant", &reply.text)?;

    if existing_messages.len() <= 1 {
        let title = if content.chars().count() > 30 {
            let prefix: String = content.chars().take(30).collect();
            format!("{prefix}...")
        } else {
            content.to_string()
        };
        db.update_conversation_title(conversation_id, &title)?;
    }

    Ok(reply.text)
}

async fn send_task_message_via_moltis(
    settings: &Settings,
    task_id: &str,
    content: &str,
) -> Result<String, CommandError> {
    let client = build_moltis_client(settings)?;
    let session_key = format!("kuse-task:{task_id}");
    let model_override = moltis_model_override(settings);

    let reply = match client
        .chat_send_and_wait(&session_key, content, model_override.as_deref())
        .await
    {
        Ok(reply) => reply,
        Err(MoltisClientError::Rpc { message, .. })
            if model_override.is_some() && is_moltis_model_not_found_error(&message) =>
        {
            client
                .chat_send_and_wait(&session_key, content, None)
                .await?
        }
        Err(err) => return Err(err.into()),
    };

    Ok(reply.text)
}

// Conversation commands
#[command]
pub fn list_conversations(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Conversation>, CommandError> {
    state.db.list_conversations().map_err(Into::into)
}

#[command]
pub fn create_conversation(
    state: State<'_, Arc<AppState>>,
    title: String,
) -> Result<Conversation, CommandError> {
    let id = uuid::Uuid::new_v4().to_string();
    state
        .db
        .create_conversation(&id, &title)
        .map_err(Into::into)
}

#[command]
pub fn update_conversation_title(
    state: State<'_, Arc<AppState>>,
    id: String,
    title: String,
) -> Result<(), CommandError> {
    state
        .db
        .update_conversation_title(&id, &title)
        .map_err(Into::into)
}

#[command]
pub fn delete_conversation(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), CommandError> {
    state.db.delete_conversation(&id).map_err(Into::into)
}

// Message commands
#[command]
pub fn get_messages(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
) -> Result<Vec<Message>, CommandError> {
    state.db.get_messages(&conversation_id).map_err(Into::into)
}

#[command]
pub fn add_message(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    role: String,
    content: String,
) -> Result<Message, CommandError> {
    let id = uuid::Uuid::new_v4().to_string();
    state
        .db
        .add_message(&id, &conversation_id, &role, &content)
        .map_err(Into::into)
}

// Chat command with streaming
#[derive(Clone, Serialize)]
struct StreamPayload {
    text: String,
    done: bool,
}

#[command]
pub async fn send_chat_message(
    window: Window,
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    content: String,
) -> Result<String, CommandError> {
    let settings = state.db.get_settings()?;
    let response =
        send_chat_message_via_moltis_with_db(&state.db, &settings, &conversation_id, &content)
            .await?;

    let _ = window.emit(
        "chat-stream",
        StreamPayload {
            text: response.clone(),
            done: true,
        },
    );

    Ok(response)
}

// Chat event for tool-enabled chat
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ChatEvent {
    #[serde(rename = "text")]
    Text { content: String },
    #[serde(rename = "tool_start")]
    ToolStart {
        tool: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_end")]
    ToolEnd {
        tool: String,
        result: String,
        success: bool,
    },
    #[serde(rename = "done")]
    Done { final_text: String },
}

// Agent command
#[derive(Debug, Deserialize)]
pub struct AgentRequest {
    pub message: String,
    pub project_path: Option<String>,
    pub system_prompt: Option<String>,
    pub max_turns: Option<u32>,
}

#[command]
pub async fn run_agent(
    window: Window,
    state: State<'_, Arc<AppState>>,
    request: AgentRequest,
) -> Result<String, CommandError> {
    let _ = (window, state, request);
    Err(CommandError {
        message: "Direct provider agent mode is disabled. Use Moltis-backed task flow.".to_string(),
    })
}

// Enhanced chat with tools - integrates agent capabilities into chat
#[derive(Debug, Deserialize)]
pub struct EnhancedChatRequest {
    pub conversation_id: String,
    pub content: String,
    pub project_path: Option<String>,
    pub enable_tools: bool,
}

#[command]
pub async fn send_chat_with_tools(
    window: Window,
    state: State<'_, Arc<AppState>>,
    request: EnhancedChatRequest,
) -> Result<String, CommandError> {
    let _ = (window, state, request);
    Err(CommandError {
        message: "Tool-enabled chat is disabled in Moltis-only mode.".to_string(),
    })
}

// Task commands
#[command]
pub fn list_tasks(state: State<'_, Arc<AppState>>) -> Result<Vec<Task>, CommandError> {
    state.db.list_tasks().map_err(Into::into)
}

#[command]
pub fn get_task(state: State<'_, Arc<AppState>>, id: String) -> Result<Option<Task>, CommandError> {
    state.db.get_task(&id).map_err(Into::into)
}

#[command]
pub fn create_task(
    state: State<'_, Arc<AppState>>,
    title: String,
    description: String,
    project_path: Option<String>,
) -> Result<Task, CommandError> {
    let id = uuid::Uuid::new_v4().to_string();
    state
        .db
        .create_task(&id, &title, &description, project_path.as_deref())
        .map_err(Into::into)
}

#[command]
pub fn delete_task(state: State<'_, Arc<AppState>>, id: String) -> Result<(), CommandError> {
    state.db.delete_task(&id).map_err(Into::into)
}

// Run agent with task tracking
#[derive(Debug, Deserialize)]
pub struct TaskAgentRequest {
    pub task_id: String,
    pub message: String,
    pub project_path: Option<String>,
    pub max_turns: Option<u32>,
}

#[command]
pub async fn run_task_agent(
    window: Window,
    state: State<'_, Arc<AppState>>,
    request: TaskAgentRequest,
) -> Result<String, CommandError> {
    let settings = state.db.get_settings()?;

    // Save new user message
    let user_msg_id = uuid::Uuid::new_v4().to_string();
    state
        .db
        .add_task_message(&user_msg_id, &request.task_id, "user", &request.message)?;

    // Update task status to running
    state.db.update_task_status(&request.task_id, "running")?;
    let final_text =
        match send_task_message_via_moltis(&settings, &request.task_id, &request.message).await {
            Ok(text) => text,
            Err(err) => {
                let _ = state.db.update_task_status(&request.task_id, "failed");
                let _ = window.emit(
                    "agent-event",
                    AgentEvent::Error {
                        message: err.message.clone(),
                    },
                );
                return Err(err);
            }
        };

    let assistant_msg_id = uuid::Uuid::new_v4().to_string();
    state.db.add_task_message(
        &assistant_msg_id,
        &request.task_id,
        "assistant",
        &final_text,
    )?;
    state.db.update_task_status(&request.task_id, "completed")?;

    let _ = window.emit(
        "agent-event",
        AgentEvent::Text {
            content: final_text.clone(),
        },
    );
    let _ = window.emit("agent-event", AgentEvent::Done { total_turns: 1 });

    Ok("Task completed successfully".to_string())
}

// Get task messages command
#[command]
pub fn get_task_messages(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<Vec<TaskMessage>, CommandError> {
    state.db.get_task_messages(&task_id).map_err(Into::into)
}

// Skills commands
#[command]
pub fn get_skills_list() -> Vec<SkillMetadata> {
    get_available_skills()
}

// MCP commands
#[command]
pub fn list_mcp_servers(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<MCPServerConfig>, CommandError> {
    state.db.get_mcp_servers().map_err(|e| CommandError {
        message: format!("Failed to get MCP servers: {}", e),
    })
}

#[command]
pub fn save_mcp_server(
    state: State<'_, Arc<AppState>>,
    config: MCPServerConfig,
) -> Result<(), CommandError> {
    state.db.save_mcp_server(&config).map_err(|e| CommandError {
        message: format!("Failed to save MCP server: {}", e),
    })
}

#[command]
pub fn delete_mcp_server(state: State<'_, Arc<AppState>>, id: String) -> Result<(), CommandError> {
    state.db.delete_mcp_server(&id).map_err(|e| CommandError {
        message: format!("Failed to delete MCP server: {}", e),
    })
}

#[command]
pub async fn connect_mcp_server(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), CommandError> {
    // Get server config from database
    let config = match state.db.get_mcp_server(&id).map_err(|e| CommandError {
        message: format!("Failed to get server config: {}", e),
    })? {
        Some(config) => config,
        None => {
            return Err(CommandError {
                message: "MCP server not found".to_string(),
            })
        }
    };

    // Connect using MCP manager
    state
        .mcp_manager
        .connect_server(&config)
        .await
        .map_err(|e| CommandError {
            message: format!("Failed to connect to MCP server: {}", e),
        })?;

    // Update enabled status in database
    state
        .db
        .update_mcp_server_enabled(&id, true)
        .map_err(|e| CommandError {
            message: format!("Failed to update server status: {}", e),
        })
}

#[command]
pub async fn disconnect_mcp_server(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), CommandError> {
    // Disconnect using MCP manager
    state.mcp_manager.disconnect_server(&id).await;

    // Update enabled status in database
    state
        .db
        .update_mcp_server_enabled(&id, false)
        .map_err(|e| CommandError {
            message: format!("Failed to update server status: {}", e),
        })
}

#[command]
pub async fn get_mcp_server_statuses(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<MCPServerStatus>, CommandError> {
    Ok(state.mcp_manager.get_server_statuses().await)
}

#[command]
pub async fn execute_mcp_tool(
    state: State<'_, Arc<AppState>>,
    call: MCPToolCall,
) -> Result<MCPToolResult, CommandError> {
    Ok(state.mcp_manager.execute_tool(&call).await)
}

/// Convert Claude API request format to OpenAI format
fn convert_to_openai_format(
    request: &crate::agent::message_builder::ClaudeApiRequest,
    model: &str,
) -> serde_json::Value {
    use crate::agent::message_builder::ApiContent;

    // Build messages, including system prompt
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // Add system message
    if !request.system.is_empty() {
        messages.push(serde_json::json!({
            "role": "system",
            "content": request.system
        }));
    }

    // Convert conversation messages
    for msg in &request.messages {
        let role = &msg.role;

        match &msg.content {
            ApiContent::Text(text) => {
                messages.push(serde_json::json!({
                    "role": role,
                    "content": text
                }));
            }
            ApiContent::Blocks(blocks) => {
                // Handle content blocks (text, tool_use, tool_result)
                let mut text_parts: Vec<String> = Vec::new();
                let mut tool_calls: Vec<serde_json::Value> = Vec::new();

                for block in blocks {
                    let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                text_parts.push(text.to_string());
                            }
                        }
                        "tool_use" => {
                            tool_calls.push(serde_json::json!({
                                "id": block.get("id"),
                                "type": "function",
                                "function": {
                                    "name": block.get("name"),
                                    "arguments": serde_json::to_string(block.get("input").unwrap_or(&serde_json::json!({}))).unwrap_or_default()
                                }
                            }));
                        }
                        "tool_result" => {
                            // OpenAI uses tool role to represent tool results
                            messages.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": block.get("tool_use_id"),
                                "content": block.get("content")
                            }));
                        }
                        _ => {}
                    }
                }

                // If there's text content
                if !text_parts.is_empty() {
                    let mut msg_obj = serde_json::json!({
                        "role": role,
                        "content": text_parts.join("\n")
                    });

                    // If there are tool_calls
                    if !tool_calls.is_empty() {
                        msg_obj["tool_calls"] = serde_json::json!(tool_calls);
                    }

                    messages.push(msg_obj);
                } else if !tool_calls.is_empty() {
                    // Only tool_calls, no text
                    messages.push(serde_json::json!({
                        "role": role,
                        "content": serde_json::Value::Null,
                        "tool_calls": tool_calls
                    }));
                }
            }
        }
    }

    // Convert tools definition
    let tools: Vec<serde_json::Value> = request
        .tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema
                }
            })
        })
        .collect();

    let mut openai_request = serde_json::json!({
        "model": request.model,
        "stream": request.stream,
        "messages": messages
    });

    // Use correct max tokens parameter based on model
    let model_lower = model.to_lowercase();
    let is_legacy = model_lower.contains("gpt-3.5")
        || (model_lower.contains("gpt-4")
            && !model_lower.contains("gpt-4o")
            && !model_lower.contains("gpt-4-turbo"));

    if is_legacy {
        openai_request["max_tokens"] = serde_json::json!(request.max_tokens);
    } else {
        openai_request["max_completion_tokens"] = serde_json::json!(request.max_tokens);
    }

    // Only add temperature for non-reasoning models (o1, o3, gpt-5 don't support custom temperature)
    let is_reasoning = model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("gpt-5")
        || model_lower.contains("-o1")
        || model_lower.contains("-o3")
        || model_lower.contains("o1-")
        || model_lower.contains("o3-");

    if !is_reasoning {
        if let Some(temp) = request.temperature {
            openai_request["temperature"] = serde_json::json!(temp);
        }
    }

    if !tools.is_empty() {
        openai_request["tools"] = serde_json::json!(tools);
        openai_request["tool_choice"] = serde_json::json!("auto");
    }

    openai_request
}

/// Convert Claude API request format to Google Gemini format
fn convert_to_google_format(
    request: &crate::agent::message_builder::ClaudeApiRequest,
    _model: &str,
    max_tokens: u32,
    thought_signatures: &std::collections::HashMap<String, String>,
) -> serde_json::Value {
    use crate::agent::message_builder::ApiContent;

    // Build contents array
    let mut contents: Vec<serde_json::Value> = Vec::new();

    // Convert messages to Google format
    for msg in &request.messages {
        // Google uses "user" and "model" instead of "user" and "assistant"
        let role = if msg.role == "assistant" {
            "model"
        } else {
            &msg.role
        };

        let parts = match &msg.content {
            ApiContent::Text(text) => {
                vec![serde_json::json!({"text": text})]
            }
            ApiContent::Blocks(blocks) => {
                let mut parts_list = Vec::new();
                for block in blocks {
                    let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                parts_list.push(serde_json::json!({"text": text}));
                            }
                        }
                        "tool_use" => {
                            // Convert to functionCall format with thoughtSignature if present (for Gemini 3)
                            let tool_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            let mut fc_part = serde_json::json!({
                                "functionCall": {
                                    "name": block.get("name"),
                                    "args": block.get("input")
                                }
                            });
                            // Include thoughtSignature if we have it for this tool
                            if let Some(sig) = thought_signatures.get(tool_id) {
                                fc_part["thoughtSignature"] = serde_json::json!(sig);
                            }
                            parts_list.push(fc_part);
                        }
                        "tool_result" => {
                            // Convert to functionResponse format with thoughtSignature (required for Gemini 3)
                            let tool_use_id = block
                                .get("tool_use_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let mut fr_part = serde_json::json!({
                                "functionResponse": {
                                    "name": tool_use_id,
                                    "response": {
                                        "content": block.get("content")
                                    }
                                }
                            });
                            // Include thoughtSignature from matching tool_use (required for Gemini 3)
                            if let Some(sig) = thought_signatures.get(tool_use_id) {
                                fr_part["thoughtSignature"] = serde_json::json!(sig);
                            }
                            parts_list.push(fr_part);
                        }
                        _ => {}
                    }
                }
                parts_list
            }
        };

        if !parts.is_empty() {
            contents.push(serde_json::json!({
                "role": role,
                "parts": parts
            }));
        }
    }

    // Convert tools to Google functionDeclarations format
    let function_declarations: Vec<serde_json::Value> = request
        .tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.input_schema
            })
        })
        .collect();

    let mut google_request = serde_json::json!({
        "contents": contents,
        "generationConfig": {
            "maxOutputTokens": max_tokens
        }
    });

    // Add system instruction if present
    if !request.system.is_empty() {
        google_request["systemInstruction"] = serde_json::json!({
            "parts": [{"text": request.system}]
        });
    }

    // Add tools if present
    if !function_declarations.is_empty() {
        google_request["tools"] = serde_json::json!([{
            "functionDeclarations": function_declarations
        }]);
    }

    google_request
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    fn settings_with_model(model: &str, provider: &str) -> Settings {
        let mut settings = Settings::default();
        settings.model = model.to_string();
        settings.provider = provider.to_string();
        settings
    }

    #[test]
    fn build_moltis_client_validates_server_url() {
        let mut settings = Settings::default();
        settings.moltis_server_url = "   ".to_string();
        let err = build_moltis_client(&settings).unwrap_err();
        assert!(err.message.contains("Moltis server URL not configured"));
    }

    #[test]
    fn build_moltis_client_accepts_empty_and_non_empty_api_key() {
        let mut settings = Settings::default();
        settings.moltis_server_url = "http://127.0.0.1:13131".to_string();
        settings.moltis_api_key = String::new();
        assert!(build_moltis_client(&settings).is_ok());

        settings.moltis_api_key = "token-123".to_string();
        assert!(build_moltis_client(&settings).is_ok());
    }

    #[test]
    fn moltis_model_override_handles_provider_mapping_and_passthrough() {
        let openai = settings_with_model("gpt-5", "openai");
        assert_eq!(
            moltis_model_override(&openai).as_deref(),
            Some("openai::gpt-5")
        );

        let openrouter = settings_with_model("anthropic/claude-sonnet-4", "openrouter");
        assert_eq!(
            moltis_model_override(&openrouter).as_deref(),
            Some("openrouter::anthropic/claude-sonnet-4")
        );

        let already_prefixed = settings_with_model("google::gemini-2.5-pro", "google");
        assert_eq!(
            moltis_model_override(&already_prefixed).as_deref(),
            Some("google::gemini-2.5-pro")
        );

        let unknown = settings_with_model("custom-model", "custom");
        assert_eq!(
            moltis_model_override(&unknown).as_deref(),
            Some("custom-model")
        );

        let empty = settings_with_model("   ", "anthropic");
        assert_eq!(moltis_model_override(&empty), None);
    }

    #[test]
    fn model_not_found_error_detection_matches_expected_patterns() {
        assert!(is_moltis_model_not_found_error(
            "Model not found in registry"
        ));
        assert!(is_moltis_model_not_found_error("UNKNOWN MODEL id"));
        assert!(!is_moltis_model_not_found_error("rate limit exceeded"));
    }

    async fn ws_read_json(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    ) -> serde_json::Value {
        match socket.next().await {
            Some(Ok(WsMessage::Text(text))) => serde_json::from_str(&text).unwrap(),
            Some(Ok(WsMessage::Binary(bytes))) => serde_json::from_slice(&bytes).unwrap(),
            other => panic!("unexpected ws message: {other:?}"),
        }
    }

    async fn ws_send_json(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        value: serde_json::Value,
    ) {
        let text = serde_json::to_string(&value).unwrap();
        socket.send(WsMessage::Text(text.into())).await.unwrap();
    }

    async fn ws_accept_and_handshake(
        listener: &TcpListener,
    ) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let connect = ws_read_json(&mut socket).await;
        let connect_id = connect
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap()
            .to_string();
        ws_send_json(
            &mut socket,
            serde_json::json!({
                "type":"res",
                "id":connect_id,
                "ok":true,
                "payload":{"type":"hello-ok","protocol":3}
            }),
        )
        .await;
        socket
    }

    #[tokio::test]
    async fn bridge_helpers_handle_health_connect_and_rpc() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            async fn respond_health(stream: &mut tokio::net::TcpStream) {
                let mut buf = [0_u8; 1024];
                let _ = stream.read(&mut buf).await.unwrap();
                let body = r#"{"status":"ok","version":"9.9.9"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }

            // 1) health request from test_moltis_connection_with_settings
            let (mut health_stream, _) = listener.accept().await.unwrap();
            respond_health(&mut health_stream).await;

            // 2) ws connect used by test_moltis_connection_with_settings
            let mut socket = ws_accept_and_handshake(&listener).await;
            let _ = socket.close(None).await;

            // 3) health request from moltis_health_with_settings
            let (mut health_stream, _) = listener.accept().await.unwrap();
            respond_health(&mut health_stream).await;

            // 4) ws connect used by moltis_call_with_settings
            let mut socket = ws_accept_and_handshake(&listener).await;
            let req = ws_read_json(&mut socket).await;
            let req_id = req
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap()
                .to_string();
            ws_send_json(
                &mut socket,
                serde_json::json!({
                    "type":"res",
                    "id": req_id,
                    "ok": true,
                    "payload": {"ok": true}
                }),
            )
            .await;
            let _ = socket.close(None).await;
        });

        let mut settings = Settings::default();
        settings.moltis_server_url = format!("http://{addr}");

        let status = test_moltis_connection_with_settings(&settings)
            .await
            .unwrap();
        assert!(status.contains("version=9.9.9"));
        assert!(status.contains("protocol=3"));

        let health = moltis_health_with_settings(&settings).await.unwrap();
        assert_eq!(
            health.get("version").and_then(serde_json::Value::as_str),
            Some("9.9.9")
        );

        let rpc = moltis_call_with_settings(
            &settings,
            MoltisCallRequest {
                method: "health".to_string(),
                params: serde_json::json!({}),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            rpc.get("ok").and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[tokio::test]
    async fn bridge_helpers_validate_empty_method_for_moltis_call() {
        let settings = Settings::default();
        let err = moltis_call_with_settings(
            &settings,
            MoltisCallRequest {
                method: "   ".to_string(),
                params: serde_json::json!({}),
            },
        )
        .await
        .unwrap_err();
        assert!(err.message.contains("Method name is required"));
    }

    #[tokio::test]
    async fn moltis_connection_status_reports_success_and_validation_errors() {
        let mut invalid = Settings::default();
        invalid.moltis_server_url = "   ".to_string();
        let invalid_status = moltis_connection_status_with_settings(&invalid).await;
        assert!(!invalid_status.ok);
        assert!(invalid_status
            .error
            .unwrap_or_default()
            .contains("Moltis server URL not configured"));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            async fn respond_health(stream: &mut tokio::net::TcpStream) {
                let mut buf = [0_u8; 1024];
                let _ = stream.read(&mut buf).await.unwrap();
                let body = r#"{"status":"ok","version":"3.2.1"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }

            let (mut health_stream, _) = listener.accept().await.unwrap();
            respond_health(&mut health_stream).await;

            let mut socket = ws_accept_and_handshake(&listener).await;
            let _ = socket.close(None).await;
        });

        let mut valid = Settings::default();
        valid.moltis_server_url = format!("http://{addr}");
        valid.moltis_api_key = "bridge-key".to_string();
        let status = moltis_connection_status_with_settings(&valid).await;
        assert!(status.ok);
        assert_eq!(status.auth_mode, "bearer");
        assert_eq!(status.version.as_deref(), Some("3.2.1"));
        assert_eq!(status.protocol, Some(3));
    }

    #[tokio::test]
    async fn send_chat_message_via_moltis_with_db_falls_back_on_model_not_found() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            // First chat attempt: return model-not-found error
            let mut socket = ws_accept_and_handshake(&listener).await;
            let req = ws_read_json(&mut socket).await;
            let req_id = req
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap()
                .to_string();
            assert!(req
                .get("params")
                .and_then(|v| v.get("model"))
                .and_then(serde_json::Value::as_str)
                .is_some());
            ws_send_json(
                &mut socket,
                serde_json::json!({
                    "type":"res",
                    "id": req_id,
                    "ok": true,
                    "payload": {"runId":"run-1"}
                }),
            )
            .await;
            ws_send_json(
                &mut socket,
                serde_json::json!({
                    "type":"event",
                    "event":"chat",
                    "payload":{
                        "runId":"run-1",
                        "sessionKey":"kuse:conv-1",
                        "state":"error",
                        "error":{"message":"unknown model"}
                    }
                }),
            )
            .await;
            let _ = socket.close(None).await;

            // Second chat attempt (fallback): succeeds without model param
            let mut socket = ws_accept_and_handshake(&listener).await;
            let req = ws_read_json(&mut socket).await;
            let req_id = req
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap()
                .to_string();
            assert!(req.get("params").and_then(|v| v.get("model")).is_none());
            ws_send_json(
                &mut socket,
                serde_json::json!({
                    "type":"res",
                    "id": req_id,
                    "ok": true,
                    "payload": {"runId":"run-2"}
                }),
            )
            .await;
            ws_send_json(
                &mut socket,
                serde_json::json!({
                    "type":"event",
                    "event":"chat",
                    "payload":{
                        "runId":"run-2",
                        "sessionKey":"kuse:conv-1",
                        "state":"final",
                        "text":"Fallback OK"
                    }
                }),
            )
            .await;
            let _ = socket.close(None).await;
        });

        let db = Database::new_in_memory_for_tests().unwrap();
        db.create_conversation("conv-1", "tmp").unwrap();

        let mut settings = settings_with_model("gpt-5", "openai");
        settings.moltis_server_url = format!("ws://{addr}");
        settings.moltis_api_key = "bridge-key".to_string();

        let result = send_chat_message_via_moltis_with_db(
            &db,
            &settings,
            "conv-1",
            "This is a fairly long user message for title trimming",
        )
        .await
        .unwrap();
        assert_eq!(result, "Fallback OK");

        let messages = db.get_messages("conv-1").unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "Fallback OK");

        let conversation = db
            .list_conversations()
            .unwrap()
            .into_iter()
            .find(|c| c.id == "conv-1")
            .unwrap();
        assert_eq!(conversation.title, "This is a fairly long user mes...");
    }
}
