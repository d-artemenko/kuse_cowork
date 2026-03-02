import { Component, For, Show, createSignal } from "solid-js";
import { useChat } from "../stores/chat";
import { useSettings } from "../stores/settings";
import {
  sendChatMessage,
  sendChatWithTools,
  sendChatMessageViaMoltis,
  ChatEvent,
  isTauri,
} from "../lib/tauri-api";
import "./Chat.css";

interface ToolExecution {
  id: number;
  tool: string;
  input: Record<string, unknown>;
  result?: string;
  success?: boolean;
  status: "running" | "completed" | "error";
}

const Chat: Component = () => {
  const {
    activeConversation,
    activeConversationId,
    messages,
    createConversation,
    addLocalMessage,
    updateLastMessage,
    refreshConversations,
    isLoading,
    setIsLoading,
  } = useChat();
  const { settings, isConfigured, toggleSettings } = useSettings();

  const [input, setInput] = createSignal("");
  const [enableTools, setEnableTools] = createSignal(true);
  const [projectPath, setProjectPath] = createSignal("");
  const [toolExecutions, setToolExecutions] = createSignal<ToolExecution[]>([]);
  const [showProjectInput, setShowProjectInput] = createSignal(false);
  const [useMoltisGateway, setUseMoltisGateway] = createSignal(false);
  let messagesEnd: HTMLDivElement | undefined;

  const scrollToBottom = () => {
    messagesEnd?.scrollIntoView({ behavior: "smooth" });
  };

  // const formatToolInput = (input: Record<string, unknown>): string => {
  //   const entries = Object.entries(input);
  //   if (entries.length === 0) return "";
  //   return entries
  //     .map(([key, value]) => {
  //       const strValue = typeof value === "string" ? value : JSON.stringify(value);
  //       const truncated = strValue.length > 60 ? strValue.slice(0, 60) + "..." : strValue;
  //       return `${key}: ${truncated}`;
  //     })
  //     .join(", ");
  // };

  const handleChatEvent = (event: ChatEvent) => {
    console.log("Chat event:", event);
    switch (event.type) {
      case "text":
        updateLastMessage(event.content);
        scrollToBottom();
        break;
      case "tool_start":
        setToolExecutions((prev) => [
          ...prev,
          {
            id: Date.now(),
            tool: event.tool,
            input: event.input,
            status: "running",
          },
        ]);
        scrollToBottom();
        break;
      case "tool_end":
        setToolExecutions((prev) => {
          const updated = [...prev];
          const last = updated.findLast((t: ToolExecution) => t.tool === event.tool && t.status === "running");
          if (last) {
            last.result = event.result;
            last.success = event.success;
            last.status = event.success ? "completed" : "error";
          }
          return updated;
        });
        scrollToBottom();
        break;
      case "done":
        updateLastMessage(event.final_text);
        scrollToBottom();
        break;
    }
  };

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const text = input().trim();
    if (!text || isLoading()) return;

    let convId = activeConversationId();
    if (!convId) {
      const conv = await createConversation();
      if (!conv) return;
      convId = conv.id;
    }

    setInput("");
    setToolExecutions([]); // Reset tool executions
    addLocalMessage("user", text);
    addLocalMessage("assistant", "");
    setIsLoading(true);
    scrollToBottom();

    try {
      // Route to Moltis gateway when enabled.
      if (isTauri() && useMoltisGateway()) {
        const fullText = await sendChatMessageViaMoltis(convId, text);
        updateLastMessage(fullText);
        scrollToBottom();
      } else if (enableTools() && isTauri()) {
        // Use enhanced chat with tools if enabled and in Tauri
        await sendChatWithTools(
          {
            conversation_id: convId,
            content: text,
            project_path: projectPath() || undefined,
            enable_tools: true,
          },
          handleChatEvent
        );
      } else {
        // Fall back to simple chat
        await sendChatMessage(convId, text, (streamedText) => {
          updateLastMessage(streamedText);
          scrollToBottom();
        });
      }
      // Refresh conversations to get updated title
      await refreshConversations();
    } catch (error) {
      console.error("Chat error:", error);
      let errorMsg = "Unknown error";
      if (error instanceof Error) {
        errorMsg = error.message;
      } else if (typeof error === "object" && error !== null) {
        // Tauri CommandError format
        errorMsg = (error as { message?: string }).message || JSON.stringify(error);
      } else if (typeof error === "string") {
        errorMsg = error;
      }
      updateLastMessage(`Error: ${errorMsg}`);
    } finally {
      setIsLoading(false);
      setToolExecutions([]); // Clear tool executions after completion
      scrollToBottom();
    }
  };

  return (
    <div class="chat">
      <Show
        when={isConfigured()}
        fallback={
          <div class="chat-setup">
            <h2>Welcome to Kuse Cowork</h2>
            <p>Configure your API key to get started</p>
            <button onClick={toggleSettings}>Open Settings</button>
          </div>
        }
      >
        <div class="messages">
          <Show
            when={activeConversation()}
            fallback={
              <div class="empty-chat">
                <h2>Start a new conversation</h2>
                <p>Type a message below or click "New Chat" in the sidebar</p>
              </div>
            }
          >
            <For each={messages()}>
              {(msg) => (
                <div class={`message ${msg.role}`}>
                  <div class="message-role">
                    {msg.role === "user" ? "You" : "Claude"}
                  </div>
                  <div class="message-content">
                    {msg.content || (
                      <span class="typing-indicator">
                        <span></span>
                        <span></span>
                        <span></span>
                      </span>
                    )}
                  </div>
                </div>
              )}
            </For>

            {/* Tool executions display */}
            <Show when={toolExecutions().length > 0}>
              <div class="tool-executions-inline">
                <For each={toolExecutions()}>
                  {(tool) => (
                    <div class={`tool-chip ${tool.status}`}>
                      <span class="tool-chip-name">{tool.tool}</span>
                      <span class="tool-chip-status">
                        {tool.status === "running" && "..."}
                        {tool.status === "completed" && "✓"}
                        {tool.status === "error" && "✗"}
                      </span>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </Show>
          <div ref={messagesEnd} />
        </div>

        <div class="chat-controls">
          <Show when={isTauri()}>
            <div class="tools-toggle">
              <label class="toggle-label">
                <input
                  type="checkbox"
                  checked={enableTools()}
                  onChange={(e) => setEnableTools(e.currentTarget.checked)}
                  disabled={isLoading() || useMoltisGateway()}
                />
                <span class="toggle-text">Tools</span>
              </label>
              <label class="toggle-label">
                <input
                  type="checkbox"
                  checked={useMoltisGateway()}
                  onChange={(e) => {
                    const enabled = e.currentTarget.checked;
                    setUseMoltisGateway(enabled);
                    if (enabled) {
                      setEnableTools(false);
                    }
                  }}
                  disabled={isLoading() || !settings().moltisServerUrl.trim()}
                />
                <span class="toggle-text">Use Moltis</span>
              </label>
              <Show when={enableTools()}>
                <button
                  type="button"
                  class="project-toggle"
                  onClick={() => setShowProjectInput(!showProjectInput())}
                >
                  {showProjectInput() ? "Hide Path" : "Set Path"}
                </button>
              </Show>
            </div>
            <Show when={enableTools() && showProjectInput()}>
              <div class="project-path-row">
                <input
                  type="text"
                  value={projectPath()}
                  onInput={(e) => setProjectPath(e.currentTarget.value)}
                  placeholder="Project path (optional): /path/to/project"
                  disabled={isLoading()}
                />
              </div>
            </Show>
          </Show>
        </div>

        <form class="input-form" onSubmit={handleSubmit}>
          <textarea
            value={input()}
            onInput={(e) => setInput(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleSubmit(e);
              }
            }}
            placeholder={enableTools() ? "Ask me to read files, run commands, or search code..." : "Type your message..."}
            disabled={isLoading()}
            rows={3}
          />
          <button type="submit" disabled={isLoading() || !input().trim()}>
            {isLoading() ? (toolExecutions().length > 0 ? "Working..." : "Sending...") : "Send"}
          </button>
        </form>
      </Show>
    </div>
  );
};

export default Chat;
