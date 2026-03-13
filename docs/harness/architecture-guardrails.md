# Architecture Guardrails

Owner: Kuse app team
Last reviewed: 2026-03-13

## Intent

Keep `kuse_cowork` understandable as three explicit layers: Solid UI, Tauri command surface, and Moltis bridge/runtime helpers.

## Hard Guardrails

- Keep UI/backend contracts synchronized through `src/lib/tauri-api.ts` and `src-tauri/src/commands.rs`.
- Route Moltis communication through the Tauri backend bridge; do not reintroduce direct frontend runtime fallbacks to old provider/model clients.
- Preserve runtime diagnostics capture for UI exceptions and rejected promises.
- Treat Moltis settings as persisted runtime state, not hidden UI-only defaults.

## Review Questions

- Does the change keep Moltis bridge ownership in the backend rather than the frontend?
- Does the change preserve diagnostic visibility when the UI path fails?
- Would `npm run check:moltis-only` still prove the same architectural boundary after this change?

## See Also

- Harness index: [README.md](README.md)
- Architecture: [../../ARCHITECTURE.md](../../ARCHITECTURE.md)
- Quality gates: [quality-gates.md](quality-gates.md)
