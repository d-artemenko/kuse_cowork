import { Component, For, Show, createSignal } from "solid-js";
import { useChat } from "../stores/chat";
import { useSettings } from "../stores/settings";
import { sendChatMessageViaMoltis, isTauri, reportUiRuntimeError } from "../lib/tauri-api";
import "./Chat.css";

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
  let messagesEnd: HTMLDivElement | undefined;

  const scrollToBottom = () => {
    messagesEnd?.scrollIntoView({ behavior: "smooth" });
  };

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const text = input().trim();
    if (!text || isLoading()) return;
    if (!isTauri()) {
      updateLastMessage("Error: Chat is available only in the desktop app");
      return;
    }

    let convId = activeConversationId();
    if (!convId) {
      const conv = await createConversation();
      if (!conv) return;
      convId = conv.id;
    }

    setInput("");
    addLocalMessage("user", text);
    addLocalMessage("assistant", "");
    setIsLoading(true);
    scrollToBottom();

    try {
      const fullText = await sendChatMessageViaMoltis(convId, text);
      updateLastMessage(fullText);
      await refreshConversations();
    } catch (error) {
      let errorMsg = "Unknown error";
      if (error instanceof Error) {
        errorMsg = error.message;
      } else if (typeof error === "object" && error !== null) {
        errorMsg = (error as { message?: string }).message || JSON.stringify(error);
      } else if (typeof error === "string") {
        errorMsg = error;
      }
      void reportUiRuntimeError({
        source: "chat.send",
        message: errorMsg,
      });
      updateLastMessage(`Error: ${errorMsg}`);
    } finally {
      setIsLoading(false);
      scrollToBottom();
    }
  };

  return (
    <div class="chat">
      <Show
        when={isConfigured()}
        fallback={
          <div class="chat-setup">
            <h2>Moltis connection required</h2>
            <p>Configure Moltis server URL and API key in settings to send messages.</p>
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
                    {msg.role === "user" ? "You" : "Assistant"}
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
          </Show>
          <div ref={messagesEnd} />
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
            placeholder="Type your message..."
            disabled={isLoading() || !settings().moltisServerUrl.trim()}
            rows={3}
          />
          <button
            type="submit"
            disabled={isLoading() || !input().trim() || !settings().moltisServerUrl.trim()}
          >
            {isLoading() ? "Sending..." : "Send"}
          </button>
        </form>
      </Show>
    </div>
  );
};

export default Chat;
