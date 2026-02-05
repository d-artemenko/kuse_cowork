import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

// Types matching Rust structs
export interface ClaudeCodeRequest {
  prompt: string;
  mcp_servers: string[];
  working_directory?: string;
  /** Run in interactive terminal mode (true) or print mode (false) */
  interactive?: boolean;
  /** Terminal rows for interactive mode */
  rows?: number;
  /** Terminal columns for interactive mode */
  cols?: number;
}

export type ClaudeCodeEvent =
  | { type: "output"; content: string }
  | { type: "pty_data"; data: string }  // Base64 encoded raw PTY data
  | { type: "tool_use"; tool: string; input: unknown }
  | { type: "permission_request"; id: string; tool: string; description: string }
  | { type: "auth_required"; id: string; service: string; url?: string }
  | { type: "question"; id: string; text: string; options: string[] }
  | { type: "done" }
  | { type: "error"; message: string };

export type ClaudeCodeResponse =
  | { type: "allow"; id: string }
  | { type: "deny"; id: string }
  | { type: "input"; id: string; text: string }
  | { type: "auth_complete"; id: string }
  | { type: "cancel" };

export type ClaudeCodeStatus =
  | "idle"
  | "running"
  | { waiting_for_input: { request_id: string } }
  | "completed"
  | "error";

/**
 * Start a Claude Code session and listen for events
 */
export async function startClaudeCode(
  request: ClaudeCodeRequest,
  onEvent: (event: ClaudeCodeEvent) => void
): Promise<{ sessionId: string; unlisten: UnlistenFn }> {
  // Set up event listener first
  const unlisten = await listen<ClaudeCodeEvent>("claude-code-event", (e) => {
    onEvent(e.payload);
  });

  try {
    const sessionId = await invoke<string>("start_claude_code", { request });
    return { sessionId, unlisten };
  } catch (e) {
    unlisten();
    throw e;
  }
}

/**
 * Respond to a Claude Code prompt (permission, auth, or input)
 */
export async function respondClaudeCode(
  response: ClaudeCodeResponse
): Promise<void> {
  return invoke("respond_claude_code", { response });
}

/**
 * Cancel the current Claude Code session
 */
export async function cancelClaudeCode(): Promise<void> {
  return invoke("cancel_claude_code");
}

/**
 * Get the current status of the Claude Code session
 */
export async function getClaudeCodeStatus(): Promise<ClaudeCodeStatus> {
  return invoke("get_claude_code_status");
}

/**
 * Write raw data to the Claude Code PTY (for terminal input)
 * @param data Base64 encoded data to write
 */
export async function writeClaudeCodePty(data: string): Promise<void> {
  return invoke("write_claude_code_pty", { data });
}

/**
 * Resize the Claude Code PTY
 */
export async function resizeClaudeCodePty(rows: number, cols: number): Promise<void> {
  return invoke("resize_claude_code_pty", { rows, cols });
}
