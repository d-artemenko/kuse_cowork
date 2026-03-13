# Kuse Cowork Architecture

Owner: Kuse app team
Last reviewed: 2026-03-02

## System Map

- Frontend (`src/`): SolidJS UI, local stores, Tauri API calls.
- Backend (`src-tauri/src/`):
  - command layer (`commands.rs`)
  - local persistence (`database.rs`)
  - LLM client adapters (`llm_client.rs`)
  - agent loop/tool execution (`agent/*`, `tools/*`)
  - MCP integration (`mcp/*`)

## Invariants

- Tauri command names and payloads must remain backward-compatible with UI bridge.
- Conversation/task persistence remains recoverable after app restart.
- Provider-specific headers (OpenAI org/project, etc.) are optional and scoped.

## Dependency Boundaries

- UI does not call network providers directly in Tauri mode; backend owns provider IO.
- MCP server secrets are stored through backend persistence only.
- Tool execution remains in backend Rust, not browser context.

## Extension Points

- New provider adapters in `llm_client.rs`.
- New command surfaces in `commands.rs` + `src/lib/tauri-api.ts`.
- Future Moltis integration adapter in `src-tauri/src/moltis_client/*`.
