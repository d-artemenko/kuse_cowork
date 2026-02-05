use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Events emitted from Claude Code to the frontend
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeCodeEvent {
    /// Regular text output from Claude Code
    Output { content: String },

    /// Raw PTY data for terminal rendering (base64 encoded)
    PtyData { data: String },

    /// Tool use notification
    ToolUse { tool: String, input: Value },

    /// Permission request from Claude Code (e.g., "Allow bash command?")
    PermissionRequest {
        id: String,
        tool: String,
        description: String,
    },

    /// Authentication required for MCP server
    AuthRequired {
        id: String,
        service: String, // "vercel" or "flyio"
        url: Option<String>,
    },

    /// Question from Claude Code requiring user input
    Question {
        id: String,
        text: String,
        options: Vec<String>,
    },

    /// Session completed successfully
    Done,

    /// Error occurred
    Error { message: String },
}

/// Request to start a Claude Code session
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudeCodeRequest {
    /// The prompt/message to send to Claude Code
    pub prompt: String,
    /// MCP servers to connect (e.g., ["vercel", "flyio"])
    pub mcp_servers: Vec<String>,
    /// Working directory for the session
    pub working_directory: Option<String>,
    /// Run in interactive terminal mode (true) or print mode (false)
    #[serde(default)]
    pub interactive: bool,
    /// Terminal rows (for interactive mode)
    #[serde(default = "default_rows")]
    pub rows: u16,
    /// Terminal columns (for interactive mode)
    #[serde(default = "default_cols")]
    pub cols: u16,
}

fn default_rows() -> u16 { 24 }
fn default_cols() -> u16 { 80 }

/// Response to a Claude Code prompt/permission request
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeCodeResponse {
    /// Allow the requested action
    Allow { id: String },

    /// Deny the requested action
    Deny { id: String },

    /// Provide text input
    Input { id: String, text: String },

    /// Authentication flow completed
    AuthComplete { id: String },

    /// Cancel the entire session
    Cancel,
}

/// Status of the Claude Code session
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeCodeStatus {
    /// No active session
    Idle,
    /// Session is running
    Running,
    /// Waiting for user input/permission
    WaitingForInput { request_id: String },
    /// Session completed
    Completed,
    /// Session errored
    Error,
}

impl Default for ClaudeCodeStatus {
    fn default() -> Self {
        ClaudeCodeStatus::Idle
    }
}

/// Internal structure for pending prompts that need user response
#[derive(Clone, Debug)]
pub struct PendingPrompt {
    pub id: String,
    pub prompt_type: PendingPromptType,
    pub created_at: std::time::Instant,
}

#[derive(Clone, Debug)]
pub enum PendingPromptType {
    Permission { tool: String, description: String },
    Auth { service: String, url: Option<String> },
    Question { text: String, options: Vec<String> },
}
