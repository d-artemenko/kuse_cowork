use regex::Regex;
use serde_json::Value;

/// Represents parsed output from Claude Code CLI
#[derive(Debug, Clone)]
pub enum ParsedOutput {
    /// Regular text output
    Text(String),

    /// Tool usage detected
    ToolUse { name: String, input: Value },

    /// Permission request (e.g., "Allow claude to run bash command? [y/n]")
    PermissionRequest { tool: String, description: String },

    /// Authentication required for an MCP service
    AuthRequired { service: String, url: Option<String> },

    /// Question requiring user selection
    Question { text: String, options: Vec<String> },

    /// Session completed
    Done,

    /// Error message
    Error(String),
}

/// Parser for Claude Code CLI output
pub struct OutputParser {
    // Patterns for detecting different output types
    permission_pattern: Regex,
    auth_pattern: Regex,
    url_pattern: Regex,
    tool_use_pattern: Regex,
    error_pattern: Regex,
    done_pattern: Regex,
    question_pattern: Regex,
    press_enter_pattern: Regex,
}

impl OutputParser {
    pub fn new() -> Self {
        Self {
            // Match permission prompts like:
            // "Allow claude to run bash command? [y/n]"
            // "Claude wants to write to file.txt [allow/deny]"
            // "Allow Bash tool? [y/n/always]"
            permission_pattern: Regex::new(
                r"(?i)(Allow|Claude wants to|Do you want to allow|Approve).*(bash|write|read|edit|execute|file|command|tool).*\[(y/n|allow|deny|yes|no)"
            ).unwrap(),

            // Match auth prompts like:
            // "Please authenticate with Vercel"
            // "OAuth required for Fly.io"
            // "Login to Vercel to continue"
            auth_pattern: Regex::new(
                r"(?i)(authenticate|oauth|login|sign in|authorization).*(vercel|fly\.io|flyio|github)"
            ).unwrap(),

            // Extract URLs from output
            url_pattern: Regex::new(
                r#"https?://[^\s\])<>"']+"#
            ).unwrap(),

            // Detect tool use output
            tool_use_pattern: Regex::new(
                r"(?i)(Using tool|Calling|Executing|Running):\s*(\w+)"
            ).unwrap(),

            // Error patterns
            error_pattern: Regex::new(
                r"(?i)^(error|failed|exception|fatal):"
            ).unwrap(),

            // Session complete patterns - must be standalone phrases
            done_pattern: Regex::new(
                r"(?i)^(Session (completed|ended|finished)|Goodbye!?|Task complete\.?)$"
            ).unwrap(),

            // Question patterns with options
            question_pattern: Regex::new(
                r"(?i)(\?)\s*\[([^\]]+)\]"
            ).unwrap(),

            // Prompts like "Press Enter to continue"
            press_enter_pattern: Regex::new(
                r"(?i)press\s+enter|hit\s+enter|enter\s+to\s+continue"
            ).unwrap(),
        }
    }

    /// Check if a line looks like an interactive prompt (may not end with newline)
    pub fn is_prompt(&self, line: &str) -> bool {
        let line = line.trim();
        if line.is_empty() {
            return false;
        }

        // Check common prompt patterns
        self.permission_pattern.is_match(line)
            || self.auth_pattern.is_match(line)
            || self.question_pattern.is_match(line)
            || self.press_enter_pattern.is_match(line)
            || line.ends_with("?")
            || line.ends_with("[y/n]")
            || line.ends_with("[Y/n]")
            || line.ends_with("(y/n)")
            || line.contains("Press Enter")
            || line.contains("press enter")
    }

    /// Check if a line indicates the CLI is ready for input
    pub fn is_ready_line(&self, line: &str) -> bool {
        let line = line.trim();
        if line.is_empty() {
            return false;
        }

        if line.starts_with('❯') || line.starts_with('>') {
            return true;
        }

        let lower = line.to_lowercase();
        lower.contains("what would you like")
            || lower.contains("enter your prompt")
            || lower.contains("type /help")
            || lower.contains("type /")
    }

    /// Parse a line of output from Claude Code CLI
    pub fn parse_line(&self, line: &str) -> Option<ParsedOutput> {
        let line = line.trim();

        if line.is_empty() {
            return None;
        }

        // Check for permission requests first (highest priority)
        if self.permission_pattern.is_match(line) {
            let tool = self.extract_tool_name(line);
            return Some(ParsedOutput::PermissionRequest {
                tool,
                description: line.to_string(),
            });
        }

        // Check for authentication requests
        if self.auth_pattern.is_match(line) {
            let service = self.extract_service_name(line);
            let url = self.url_pattern.find(line).map(|m| m.as_str().to_string());
            return Some(ParsedOutput::AuthRequired { service, url });
        }

        // Check for "press enter" style prompts
        if self.press_enter_pattern.is_match(line) {
            return Some(ParsedOutput::Question {
                text: line.to_string(),
                options: Vec::new(),
            });
        }

        // Check for errors
        if self.error_pattern.is_match(line) {
            return Some(ParsedOutput::Error(line.to_string()));
        }

        // Check for completion
        if self.done_pattern.is_match(line) && !line.contains("?") {
            return Some(ParsedOutput::Done);
        }

        // Check for tool use
        if let Some(captures) = self.tool_use_pattern.captures(line) {
            if let Some(tool_name) = captures.get(2) {
                return Some(ParsedOutput::ToolUse {
                    name: tool_name.as_str().to_string(),
                    input: Value::Null,
                });
            }
        }

        // Check for questions with options
        if let Some(captures) = self.question_pattern.captures(line) {
            if let Some(options_str) = captures.get(2) {
                let options: Vec<String> = options_str
                    .as_str()
                    .split('/')
                    .map(|s| s.trim().to_string())
                    .collect();

                // Get the question text (everything before the options)
                let question_end = line.find('[').unwrap_or(line.len());
                let text = line[..question_end].trim().to_string();

                return Some(ParsedOutput::Question { text, options });
            }
        }

        // Default to regular text output
        Some(ParsedOutput::Text(line.to_string()))
    }

    /// Extract tool name from a permission request line
    fn extract_tool_name(&self, line: &str) -> String {
        let line_lower = line.to_lowercase();

        // Common tool names to look for
        let tools = [
            "bash", "write", "read", "edit", "execute", "file",
            "glob", "grep", "mkdir", "rm", "mv", "cp", "curl",
            "npm", "node", "python", "git",
        ];

        for tool in tools.iter() {
            if line_lower.contains(tool) {
                return tool.to_string();
            }
        }

        // Default to "unknown"
        "unknown".to_string()
    }

    /// Extract service name from an auth request line
    fn extract_service_name(&self, line: &str) -> String {
        let line_lower = line.to_lowercase();

        if line_lower.contains("vercel") {
            "vercel".to_string()
        } else if line_lower.contains("fly.io") || line_lower.contains("flyio") {
            "flyio".to_string()
        } else if line_lower.contains("github") {
            "github".to_string()
        } else {
            "unknown".to_string()
        }
    }
}

impl Default for OutputParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_permission_request() {
        let parser = OutputParser::new();

        let line = "Allow claude to run bash command? [y/n]";
        if let Some(ParsedOutput::PermissionRequest { tool, description }) = parser.parse_line(line) {
            assert_eq!(tool, "bash");
            assert!(description.contains("bash"));
        } else {
            panic!("Expected PermissionRequest");
        }
    }

    #[test]
    fn test_parse_auth_required() {
        let parser = OutputParser::new();

        let line = "Please authenticate with Vercel: https://vercel.com/auth/cli";
        if let Some(ParsedOutput::AuthRequired { service, url }) = parser.parse_line(line) {
            assert_eq!(service, "vercel");
            assert!(url.is_some());
            assert!(url.unwrap().contains("vercel.com"));
        } else {
            panic!("Expected AuthRequired");
        }
    }

    #[test]
    fn test_parse_regular_text() {
        let parser = OutputParser::new();

        let line = "Creating new file src/main.rs";
        if let Some(ParsedOutput::Text(text)) = parser.parse_line(line) {
            assert_eq!(text, "Creating new file src/main.rs");
        } else {
            panic!("Expected Text");
        }
    }
}
