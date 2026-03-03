# Kuse Cowork Quality Gates

Owner: Kuse app team
Last reviewed: 2026-03-03

## Required Gates

- `deps`: `npm install`
- `typecheck`: `npx tsc --noEmit`
- `frontend build`: `npm run build`
- `tauri check`: `cd src-tauri && cargo check`
- `tauri lint`: `cd src-tauri && cargo clippy -- -D warnings`
- `moltis-only mechanical`: `npm run check:moltis-only`
- `moltis runtime harness`: `npm run check:moltis-runtime-harness`

## Merge Criteria

- No TypeScript type regressions in frontend bridge.
- No Rust warnings promoted by clippy gate.
- Settings, chat flow, and task flow still boot in local dev (`npm run tauri:dev`).
- Moltis bridge proves auth-required runtime path (health + ws connect + `chat.send`) and fails without valid auth key.

## Operability Claim Policy

- Do not claim "Moltis works" unless `npm run check:moltis-runtime-harness` passes in the current branch state.
- UI regressions are still possible; runtime harness confirms the desktop backend bridge contract without manual user action.
