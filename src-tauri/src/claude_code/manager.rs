use super::types::{
    ClaudeCodeEvent, ClaudeCodeRequest, ClaudeCodeResponse, ClaudeCodeStatus, PendingPrompt,
    PendingPromptType,
};
use portable_pty::{native_pty_system, CommandBuilder, Child, PtySize, MasterPty};
use std::env;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

/// Get the path to the claude CLI binary
fn get_claude_path() -> String {
    let home = env::var("HOME").unwrap_or_default();

    let common_paths = [
        "/usr/local/bin/claude",
        "/opt/homebrew/bin/claude",
        &format!("{}/.local/bin/claude", home),
        &format!("{}/.npm-global/bin/claude", home),
    ];

    for path in common_paths.iter() {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }

    "claude".to_string()
}

/// Get an extended PATH that includes common binary locations
fn get_extended_path() -> String {
    let current_path = env::var("PATH").unwrap_or_default();
    let home = env::var("HOME").unwrap_or_default();

    let additional_paths = vec![
        "/usr/local/bin".to_string(),
        "/opt/homebrew/bin".to_string(),
        format!("{}/.local/bin", home),
        format!("{}/.npm-global/bin", home),
        "/usr/bin".to_string(),
        "/bin".to_string(),
    ];

    let mut all_paths = additional_paths;
    all_paths.push(current_path);
    all_paths.join(":")
}

/// Manager for Claude Code CLI PTY sessions (interactive mode)
pub struct ClaudeCodeManager {
    status: Arc<RwLock<ClaudeCodeStatus>>,
    pending_prompts: Arc<RwLock<Vec<PendingPrompt>>>,
    pty_writer: Arc<Mutex<Option<Box<dyn Write + Send>>>>,
    pty_master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    child_process: Arc<Mutex<Option<Box<dyn Child + Send + Sync>>>>,
    cancel_tx: Arc<Mutex<Option<mpsc::Sender<()>>>>,
}

impl ClaudeCodeManager {
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(ClaudeCodeStatus::Idle)),
            pending_prompts: Arc::new(RwLock::new(Vec::new())),
            pty_writer: Arc::new(Mutex::new(None)),
            pty_master: Arc::new(Mutex::new(None)),
            child_process: Arc::new(Mutex::new(None)),
            cancel_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Start a new Claude Code session with interactive PTY
    pub async fn start_session(
        &self,
        app_handle: AppHandle,
        request: ClaudeCodeRequest,
    ) -> Result<String, String> {
        // Check if already running
        {
            let status = self.status.read().await;
            if *status == ClaudeCodeStatus::Running {
                return Err("A Claude Code session is already running".to_string());
            }
        }

        // Create PTY for interactive session with size from request
        let pty_system = native_pty_system();
        eprintln!("[ClaudeCode] Creating PTY with size: {}x{}", request.cols, request.rows);
        let pair = pty_system
            .openpty(PtySize {
                rows: request.rows,
                cols: request.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        // Build claude command
        let claude_path = get_claude_path();
        eprintln!("[ClaudeCode] Using claude path: {}", claude_path);
        eprintln!("[ClaudeCode] Interactive mode: {}", request.interactive);

        let mut cmd = CommandBuilder::new(&claude_path);

        if request.interactive {
            // Interactive mode - full terminal experience
            // Don't use -p flag, run claude normally for interactive use
            cmd.env("TERM", "xterm-256color");
        } else {
            // Print mode with streaming JSON for programmatic usage
            cmd.arg("-p");  // Print mode (non-interactive)
            cmd.arg("--output-format");
            cmd.arg("stream-json");  // Streaming JSON output
            cmd.arg("--verbose");  // Show full turn-by-turn output
            cmd.env("NO_COLOR", "1");
            cmd.env("TERM", "xterm-256color");

            // Add the prompt as the last argument in print mode
            if !request.prompt.trim().is_empty() {
                cmd.arg(&request.prompt);
            }
        }

        cmd.env("PATH", get_extended_path());

        // Set working directory
        if let Some(ref dir) = request.working_directory {
            eprintln!("[ClaudeCode] Working directory: {}", dir);
            cmd.cwd(dir);
        }

        // Spawn the process
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Failed to spawn claude: {}", e))?;

        // Get reader and writer
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to clone reader: {}", e))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to take writer: {}", e))?;

        // Store writer and master for resize
        {
            let mut pty_writer_guard = self.pty_writer.lock().await;
            *pty_writer_guard = Some(writer);
        }
        {
            let mut pty_master_guard = self.pty_master.lock().await;
            *pty_master_guard = Some(pair.master);
        }

        // Store child process
        {
            let mut child_proc = self.child_process.lock().await;
            *child_proc = Some(child);
        }

        // Create cancellation channel
        let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
        {
            let mut tx = self.cancel_tx.lock().await;
            *tx = Some(cancel_tx);
        }

        // Update status
        {
            let mut status = self.status.write().await;
            *status = ClaudeCodeStatus::Running;
        }

        let session_id = Uuid::new_v4().to_string();

        let last_output = Arc::new(StdMutex::new(Instant::now()));

        // Clone for reader thread
        let status_clone = self.status.clone();
        let pending_prompts_clone = self.pending_prompts.clone();
        let app_handle_clone = app_handle.clone();
        let last_output_clone = last_output.clone();
        let is_interactive = request.interactive;

        // Spawn reader thread
        std::thread::spawn(move || {
            eprintln!("[ClaudeCode] Reader thread started (interactive={})", is_interactive);
            let mut reader = reader;
            let mut buffer = [0u8; 8192];
            let mut line_buffer = String::new();

            loop {
                if cancel_rx.try_recv().is_ok() {
                    eprintln!("[ClaudeCode] Cancellation received");
                    break;
                }

                match reader.read(&mut buffer) {
                    Ok(0) => {
                        eprintln!("[ClaudeCode] EOF - process ended");
                        if !is_interactive && !line_buffer.trim().is_empty() {
                            Self::process_json_line(&line_buffer, &app_handle_clone, &status_clone, &pending_prompts_clone);
                        }
                        break;
                    }
                    Ok(n) => {
                        if let Ok(mut guard) = last_output_clone.lock() {
                            *guard = Instant::now();
                        }

                        if is_interactive {
                            // Interactive mode: emit raw PTY data for terminal rendering
                            use base64::{Engine as _, engine::general_purpose::STANDARD};
                            let encoded = STANDARD.encode(&buffer[..n]);
                            let _ = app_handle_clone.emit("claude-code-event", &ClaudeCodeEvent::PtyData {
                                data: encoded,
                            });
                        } else {
                            // Print mode: parse streaming JSON
                            let chunk = String::from_utf8_lossy(&buffer[..n]);
                            line_buffer.push_str(&chunk);

                            // Process complete lines (newline-delimited JSON)
                            while let Some(newline_pos) = line_buffer.find('\n') {
                                let line = line_buffer[..newline_pos].to_string();
                                line_buffer = line_buffer[newline_pos + 1..].to_string();

                                let trimmed = line.trim();
                                if trimmed.is_empty() {
                                    continue;
                                }

                                Self::process_json_line(trimmed, &app_handle_clone, &status_clone, &pending_prompts_clone);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[ClaudeCode] Read error: {}", e);
                        break;
                    }
                }
            }

            // Session ended
            if let Ok(mut status) = status_clone.try_write() {
                if *status == ClaudeCodeStatus::Running {
                    *status = ClaudeCodeStatus::Completed;
                    let _ = app_handle_clone.emit("claude-code-event", &ClaudeCodeEvent::Done);
                }
            }
        });

        // In print mode, the prompt is passed as argument, no need to send via PTY
        let _ = app_handle.emit(
            "claude-code-event",
            &ClaudeCodeEvent::Output {
                content: format!("→ Started Claude Code with prompt: {}", request.prompt),
            },
        );

        eprintln!("[ClaudeCode] Session started");

        Ok(session_id)
    }

    /// Process a line of streaming JSON output from Claude Code
    fn process_json_line(
        line: &str,
        app_handle: &AppHandle,
        status: &Arc<RwLock<ClaudeCodeStatus>>,
        pending_prompts: &Arc<RwLock<Vec<PendingPrompt>>>,
    ) {
        // Try to parse as JSON
        let json: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                // Not valid JSON, emit as plain text
                eprintln!("[ClaudeCode] Non-JSON output: {}", &line[..line.len().min(100)]);
                let _ = app_handle.emit("claude-code-event", &ClaudeCodeEvent::Output {
                    content: line.to_string(),
                });
                return;
            }
        };

        eprintln!("[ClaudeCode] JSON event: {}", json.get("type").and_then(|t| t.as_str()).unwrap_or("unknown"));

        // Handle different event types from stream-json format
        let event_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match event_type {
            "assistant" => {
                // Assistant message with content
                if let Some(message) = json.get("message") {
                    if let Some(content) = message.get("content") {
                        if let Some(arr) = content.as_array() {
                            for block in arr {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    let _ = app_handle.emit("claude-code-event", &ClaudeCodeEvent::Output {
                                        content: text.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            "content_block_delta" => {
                // Streaming text delta
                if let Some(delta) = json.get("delta") {
                    if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                        let _ = app_handle.emit("claude-code-event", &ClaudeCodeEvent::Output {
                            content: text.to_string(),
                        });
                    }
                }
            }
            "tool_use" => {
                // Tool being used
                let tool_name = json.get("name").and_then(|n| n.as_str()).unwrap_or("unknown").to_string();
                let input = json.get("input").cloned().unwrap_or(serde_json::Value::Null);
                let _ = app_handle.emit("claude-code-event", &ClaudeCodeEvent::ToolUse {
                    tool: tool_name,
                    input,
                });
            }
            "tool_result" => {
                // Tool result
                if let Some(content) = json.get("content").and_then(|c| c.as_str()) {
                    let _ = app_handle.emit("claude-code-event", &ClaudeCodeEvent::Output {
                        content: format!("Tool result: {}", content),
                    });
                }
            }
            "user_input_request" => {
                // Permission or input request
                let id = Uuid::new_v4().to_string();
                let tool = json.get("tool").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
                let description = json.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();

                let pending = PendingPrompt {
                    id: id.clone(),
                    prompt_type: PendingPromptType::Permission {
                        tool: tool.clone(),
                        description: description.clone(),
                    },
                    created_at: std::time::Instant::now(),
                };

                if let Ok(mut prompts) = pending_prompts.try_write() {
                    prompts.push(pending);
                }

                if let Ok(mut s) = status.try_write() {
                    *s = ClaudeCodeStatus::WaitingForInput {
                        request_id: id.clone(),
                    };
                }

                let _ = app_handle.emit("claude-code-event", &ClaudeCodeEvent::PermissionRequest {
                    id,
                    tool,
                    description,
                });
            }
            "error" => {
                let message = json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str())
                    .unwrap_or("Unknown error").to_string();
                if let Ok(mut s) = status.try_write() {
                    *s = ClaudeCodeStatus::Error;
                }
                let _ = app_handle.emit("claude-code-event", &ClaudeCodeEvent::Error { message });
            }
            "result" => {
                // Final result
                if let Some(result) = json.get("result").and_then(|r| r.as_str()) {
                    let _ = app_handle.emit("claude-code-event", &ClaudeCodeEvent::Output {
                        content: result.to_string(),
                    });
                }
                if let Ok(mut s) = status.try_write() {
                    *s = ClaudeCodeStatus::Completed;
                }
                let _ = app_handle.emit("claude-code-event", &ClaudeCodeEvent::Done);
            }
            _ => {
                // Unknown event type, log it
                eprintln!("[ClaudeCode] Unknown event type: {}", event_type);
            }
        }
    }

    /// Respond to a pending prompt
    pub async fn respond(&self, response: ClaudeCodeResponse) -> Result<(), String> {
        match response {
            ClaudeCodeResponse::Allow { id } => {
                let reply = self.permission_reply(&id, true).await;
                self.write_to_pty(&reply).await?;
                self.remove_pending_prompt(&id).await;
                self.update_status_to_running().await;
            }
            ClaudeCodeResponse::Deny { id } => {
                let reply = self.permission_reply(&id, false).await;
                self.write_to_pty(&reply).await?;
                self.remove_pending_prompt(&id).await;
                self.update_status_to_running().await;
            }
            ClaudeCodeResponse::AuthComplete { id } => {
                self.write_to_pty("\n").await?;
                self.remove_pending_prompt(&id).await;
                self.update_status_to_running().await;
            }
            ClaudeCodeResponse::Input { id, text } => {
                self.write_to_pty(&format!("{}\n", text)).await?;
                self.remove_pending_prompt(&id).await;
                self.update_status_to_running().await;
            }
            ClaudeCodeResponse::Cancel => {
                self.cancel().await?;
            }
        }
        Ok(())
    }

    /// Write to PTY (internal)
    async fn write_to_pty(&self, input: &str) -> Result<(), String> {
        let mut writer_guard = self.pty_writer.lock().await;
        if let Some(ref mut writer) = *writer_guard {
            writer
                .write_all(input.as_bytes())
                .map_err(|e| format!("Failed to write: {}", e))?;
            writer
                .flush()
                .map_err(|e| format!("Failed to flush: {}", e))?;
            Ok(())
        } else {
            Err("No active PTY session".to_string())
        }
    }

    /// Write raw bytes to PTY (for terminal input)
    pub async fn write_pty_raw(&self, data: &[u8]) -> Result<(), String> {
        let mut writer_guard = self.pty_writer.lock().await;
        if let Some(ref mut writer) = *writer_guard {
            writer
                .write_all(data)
                .map_err(|e| format!("Failed to write: {}", e))?;
            writer
                .flush()
                .map_err(|e| format!("Failed to flush: {}", e))?;
            Ok(())
        } else {
            Err("No active PTY session".to_string())
        }
    }

    /// Resize the PTY
    pub async fn resize_pty(&self, rows: u16, cols: u16) -> Result<(), String> {
        let master_guard = self.pty_master.lock().await;
        if let Some(ref master) = *master_guard {
            eprintln!("[ClaudeCode] Resizing PTY to {}x{}", cols, rows);
            master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| format!("Failed to resize PTY: {}", e))?;
            Ok(())
        } else {
            // No active session, just ignore
            Ok(())
        }
    }

    async fn find_pending_prompt(&self, id: &str) -> Option<PendingPrompt> {
        let prompts = self.pending_prompts.read().await;
        prompts.iter().find(|p| p.id == id).cloned()
    }

    async fn permission_reply(&self, id: &str, allow: bool) -> String {
        let prompt = self.find_pending_prompt(id).await;

        if let Some(PendingPrompt {
            prompt_type: PendingPromptType::Permission { description, .. },
            ..
        }) = prompt
        {
            return Self::select_permission_reply(&description, allow);
        }

        if allow {
            "y\n".to_string()
        } else {
            "n\n".to_string()
        }
    }

    fn select_permission_reply(description: &str, allow: bool) -> String {
        let desc = description.to_lowercase();
        let token = if desc.contains("allow/deny") || desc.contains("[allow/deny]") {
            if allow { "allow" } else { "deny" }
        } else if desc.contains("yes/no") || desc.contains("[yes/no]") {
            if allow { "yes" } else { "no" }
        } else if desc.contains("y/n") || desc.contains("[y/n]") || desc.contains("(y/n)") {
            if allow { "y" } else { "n" }
        } else {
            if allow { "y" } else { "n" }
        };

        format!("{}\n", token)
    }

    async fn remove_pending_prompt(&self, id: &str) {
        let mut prompts = self.pending_prompts.write().await;
        prompts.retain(|p| p.id != id);
    }

    async fn update_status_to_running(&self) {
        let mut status = self.status.write().await;
        if matches!(*status, ClaudeCodeStatus::WaitingForInput { .. }) {
            *status = ClaudeCodeStatus::Running;
        }
    }

    /// Cancel the current session
    pub async fn cancel(&self) -> Result<(), String> {
        // Send Ctrl+C
        let _ = self.write_to_pty("\x03").await;

        // Signal cancellation
        if let Some(tx) = self.cancel_tx.lock().await.take() {
            let _ = tx.send(()).await;
        }

        // Kill child process
        if let Some(mut child) = self.child_process.lock().await.take() {
            let _ = child.kill();
        }

        // Clear writer and master
        {
            let mut writer = self.pty_writer.lock().await;
            *writer = None;
        }
        {
            let mut master = self.pty_master.lock().await;
            *master = None;
        }

        // Clear state
        {
            let mut prompts = self.pending_prompts.write().await;
            prompts.clear();
        }
        {
            let mut status = self.status.write().await;
            *status = ClaudeCodeStatus::Idle;
        }

        Ok(())
    }

    /// Get current status
    pub async fn get_status(&self) -> ClaudeCodeStatus {
        self.status.read().await.clone()
    }
}

impl Default for ClaudeCodeManager {
    fn default() -> Self {
        Self::new()
    }
}
