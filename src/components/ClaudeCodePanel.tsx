import { Component, createSignal, createEffect, onMount, onCleanup } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import "./ClaudeCodePanel.css";

interface ClaudeCodeEvent {
  type: string;
  data?: string;
  content?: string;
  message?: string;
}

export interface ClaudeCodeTriggerData {
  prompt: string;
  working_directory?: string;
  mcp_servers?: string[];
}

interface Props {
  onClose?: () => void;
  triggerData?: ClaudeCodeTriggerData | null;
  onSessionComplete?: (result: string) => void;
  onContinueWithAgent?: (result: string) => void | Promise<void>;
}

const ClaudeCodePanel: Component<Props> = (props) => {
  let terminalRef: HTMLDivElement | undefined;
  let terminal: Terminal | undefined;
  let fitAddon: FitAddon | undefined;
  let unlistenFn: UnlistenFn | undefined;
  let resizeObserver: ResizeObserver | undefined;

  const [isRunning, setIsRunning] = createSignal(false);
  const [isReady, setIsReady] = createSignal(false);
  const [lastTriggerPrompt, setLastTriggerPrompt] = createSignal<string | null>(null);
  const [pendingPrompt, setPendingPrompt] = createSignal<string | null>(null);
  const [claudeReady, setClaudeReady] = createSignal(false);

  const startSession = async (initialPrompt?: string) => {
    if (isRunning()) return;

    // Fit terminal first to get correct dimensions
    if (fitAddon) {
      fitAddon.fit();
    }

    const rows = terminal?.rows || 24;
    const cols = terminal?.cols || 80;

    setIsRunning(true);
    terminal?.clear();

    // Set up event listener
    unlistenFn = await listen<ClaudeCodeEvent>("claude-code-event", (e) => {
      const event = e.payload;

      if (event.type === "pty_data" && event.data && terminal) {
        try {
          const bytes = Uint8Array.from(atob(event.data), c => c.charCodeAt(0));
          const text = new TextDecoder().decode(bytes);
          terminal.write(text);

          // Check if Claude is ready for input (look for the ">" prompt or input area)
          // Claude Code shows "> " when ready for input
          if (!claudeReady() && (text.includes("> ") || text.includes("❯") || text.includes("How can I help"))) {
            console.log("[ClaudeCode] Claude ready for input detected");
            setClaudeReady(true);

            // Send pending prompt if any
            const prompt = pendingPrompt();
            if (prompt) {
              console.log("[ClaudeCode] Sending pending prompt:", prompt.slice(0, 50));
              setPendingPrompt(null);
              // Small delay to ensure Claude is fully ready
              setTimeout(() => {
                writeToTerminal(prompt + "\n");
              }, 100);
            }
          }
        } catch (err) {
          console.error("Failed to decode PTY data:", err);
        }
      } else if (event.type === "done") {
        setIsRunning(false);
        setClaudeReady(false);
        terminal?.writeln("\r\n\x1b[90m--- Session ended. Press Enter to start a new session ---\x1b[0m");
        props.onSessionComplete?.("");
      } else if (event.type === "error") {
        setIsRunning(false);
        setClaudeReady(false);
        terminal?.writeln(`\r\n\x1b[91mError: ${event.message}\x1b[0m`);
      }
    });

    try {
      // Start session with correct terminal dimensions
      await invoke("start_claude_code", {
        request: {
          prompt: "",
          mcp_servers: [],
          working_directory: props.triggerData?.working_directory,
          interactive: true,
          rows,
          cols,
        }
      });

      // If there's an initial prompt, store it as pending
      // It will be sent when Claude shows it's ready for input
      if (initialPrompt) {
        console.log("[ClaudeCode] Storing pending prompt:", initialPrompt.slice(0, 50));
        setPendingPrompt(initialPrompt);
      }
    } catch (e) {
      setIsRunning(false);
      const errorMsg = e instanceof Error ? e.message : String(e);
      terminal?.writeln(`\x1b[91mFailed to start: ${errorMsg}\x1b[0m`);
      unlistenFn?.();
    }
  };

  // Auto-start when triggerData is provided and terminal is ready
  createEffect(() => {
    const data = props.triggerData;
    const ready = isReady();
    const running = isRunning();
    const lastPrompt = lastTriggerPrompt();

    console.log("[ClaudeCode] Effect check:", {
      hasData: !!data,
      prompt: data?.prompt?.slice(0, 50),
      ready,
      running,
      lastPrompt: lastPrompt?.slice(0, 50)
    });

    // Only auto-start if:
    // 1. We have trigger data with a prompt
    // 2. Terminal is ready
    // 3. Not already running
    // 4. This is a NEW prompt (different from the last one we processed)
    if (data && data.prompt && ready && !running && data.prompt !== lastPrompt) {
      console.log("[ClaudeCode] Auto-starting with prompt:", data.prompt.slice(0, 100));
      setLastTriggerPrompt(data.prompt);
      startSession(data.prompt);
    }
  });

  const writeToTerminal = async (data: string) => {
    if (!isRunning()) return;

    try {
      const bytes = new TextEncoder().encode(data);
      const base64 = btoa(String.fromCharCode(...bytes));
      await invoke("write_claude_code_pty", { data: base64 });
    } catch (e) {
      console.error("Failed to write to PTY:", e);
    }
  };

  const handleResize = () => {
    if (fitAddon && terminal) {
      fitAddon.fit();
      invoke("resize_claude_code_pty", {
        rows: terminal.rows,
        cols: terminal.cols,
      }).catch(console.error);
    }
  };

  const handleCancel = async () => {
    try {
      await invoke("cancel_claude_code");
      setIsRunning(false);
      terminal?.writeln("\r\n\x1b[93m--- Session cancelled ---\x1b[0m");
    } catch (e) {
      console.error("Failed to cancel:", e);
    }
  };

  onMount(() => {
    if (!terminalRef) return;

    terminal = new Terminal({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: '"SF Mono", "Menlo", "Monaco", "Cascadia Code", "Fira Code", monospace',
      theme: {
        background: "#0d1117",
        foreground: "#c9d1d9",
        cursor: "#f0f6fc",
        cursorAccent: "#0d1117",
        selectionBackground: "#3fb95040",
        black: "#484f58",
        red: "#ff7b72",
        green: "#3fb950",
        yellow: "#d29922",
        blue: "#58a6ff",
        magenta: "#bc8cff",
        cyan: "#39c5cf",
        white: "#b1bac4",
        brightBlack: "#6e7681",
        brightRed: "#ffa198",
        brightGreen: "#56d364",
        brightYellow: "#e3b341",
        brightBlue: "#79c0ff",
        brightMagenta: "#d2a8ff",
        brightCyan: "#56d4dd",
        brightWhite: "#f0f6fc",
      },
      allowProposedApi: true,
    });

    fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.loadAddon(new WebLinksAddon());

    terminal.open(terminalRef);

    // Initial fit after a short delay to ensure container is sized
    setTimeout(() => {
      fitAddon?.fit();
    }, 50);

    // Handle user input
    terminal.onData((data) => {
      if (!isRunning()) {
        // If not running and user presses Enter, start session
        if (data === "\r") {
          startSession();
        }
      } else {
        writeToTerminal(data);
      }
    });

    // Watch for container resize
    resizeObserver = new ResizeObserver(() => {
      handleResize();
    });
    resizeObserver.observe(terminalRef);

    // Welcome message (only if no trigger data)
    if (!props.triggerData?.prompt) {
      terminal.writeln("\x1b[1;36m╭─────────────────────────────────────────╮\x1b[0m");
      terminal.writeln("\x1b[1;36m│\x1b[0m     \x1b[1mClaude Code Interactive Terminal\x1b[0m     \x1b[1;36m│\x1b[0m");
      terminal.writeln("\x1b[1;36m╰─────────────────────────────────────────╯\x1b[0m");
      terminal.writeln("");
      terminal.writeln("\x1b[90mPress Enter to start a session...\x1b[0m");
      terminal.writeln("");
    }

    // Mark as ready (triggers auto-start if triggerData exists)
    setIsReady(true);
  });

  onCleanup(() => {
    resizeObserver?.disconnect();
    unlistenFn?.();
    terminal?.dispose();

    if (isRunning()) {
      invoke("cancel_claude_code").catch(console.error);
    }
  });

  return (
    <div class="claude-code-panel">
      <div class="claude-code-header">
        <div class="header-left">
          <div class={`status-dot ${isRunning() ? "active" : "idle"}`} title={isRunning() ? "Session Active" : "Idle"} />
          <h2 class="claude-code-title">Claude Code</h2>
        </div>
        <div class="header-actions">
          {isRunning() && (
            <button class="header-btn cancel" onClick={handleCancel} title="Stop Session">
              <span class="btn-icon">⏹</span> Stop
            </button>
          )}
          {props.onClose && (
            <button class="header-btn close" onClick={props.onClose} title="Close Panel">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          )}
        </div>
      </div>
      <div class="claude-terminal" ref={terminalRef} />
    </div>
  );
};

export default ClaudeCodePanel;
