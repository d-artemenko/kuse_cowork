import { isTauri, reportUiRuntimeError } from "./tauri-api";

let diagnosticsInstalled = false;

function normalizeReason(reason: unknown): { message: string; stack?: string } {
  if (reason instanceof Error) {
    return {
      message: reason.message || "Unhandled rejection",
      stack: reason.stack,
    };
  }
  if (typeof reason === "string") {
    return { message: reason };
  }
  try {
    return { message: JSON.stringify(reason) };
  } catch {
    return { message: String(reason) };
  }
}

export function installRuntimeDiagnostics() {
  if (diagnosticsInstalled) {
    return;
  }
  diagnosticsInstalled = true;

  if (!isTauri() || typeof window === "undefined") {
    return;
  }

  window.addEventListener("error", (event) => {
    const message = event.message || event.error?.message || "Unhandled window error";
    const context = JSON.stringify({
      filename: event.filename,
      lineno: event.lineno,
      colno: event.colno,
    });
    void reportUiRuntimeError({
      source: "window.error",
      message,
      stack: event.error?.stack,
      context,
    });
  });

  window.addEventListener("unhandledrejection", (event) => {
    const normalized = normalizeReason(event.reason);
    void reportUiRuntimeError({
      source: "window.unhandledrejection",
      message: normalized.message,
      stack: normalized.stack,
    });
  });
}
