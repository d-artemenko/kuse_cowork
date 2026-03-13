# Kuse Cowork Agent Guide

Owner: Kuse app team
Last reviewed: 2026-03-02

## Quick Map

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [docs/README.md](docs/README.md)
- [src-tauri/src/lib.rs](src-tauri/src/lib.rs): Tauri command registration and app bootstrap.
- [src-tauri/src/commands.rs](src-tauri/src/commands.rs): backend commands used by UI.
- [src/lib/tauri-api.ts](src/lib/tauri-api.ts): frontend bridge to backend commands.

## Working Defaults

- Keep UI and backend contracts in sync (`tauri-api.ts` + Rust command payloads).
- Prefer additive commands over breaking existing command signatures.
- Treat external model/provider integrations as pluggable adapters.
