# Kuse Cowork Quality Gates

Owner: Kuse app team
Last reviewed: 2026-03-13

## Required Gates

- `deps`: `npm install`
- `typecheck`: `npx tsc --noEmit`
- `frontend build`: `npm run build`
- `tauri check`: `cd src-tauri && cargo check`
- `tauri lint`: `cd src-tauri && cargo clippy -- -D warnings`
- `moltis-only mechanical`: `npm run check:moltis-only`
- `moltis runtime harness`: `npm run check:moltis-runtime-harness`

## CI Enforcement

- GitHub Actions must run `npm run check:moltis-only` on the Node job.
- GitHub Actions must run `bash scripts/check_moltis_runtime_harness.sh` on the Rust job.

## Merge Criteria

- No TypeScript type regressions in frontend bridge.
- No Rust warnings promoted by clippy gate.
- Settings, chat flow, and task flow still boot in local dev (`npm run tauri:dev`).
- Moltis bridge proves auth-required runtime path (health + ws connect + `chat.send`) and fails without valid auth key.
- Legacy hidden model settings cannot leak into Moltis RPC payload in task/chat runtime paths.
- UI runtime exceptions and rejected promises are persisted into local diagnostics storage for harness inspection.

## Operability Claim Policy

- `PASS`: required mechanical gate and required live diagnostic both passed for the touched path.
- `BLOCKED`: live Moltis proof is unavailable because the local DB/settings or reachable endpoint are missing.
- `FAIL`: a mechanical or live gate ran and exposed a regression.
- Do not claim "Moltis works" unless both `npm run check:moltis-runtime-harness` and `npm run diagnostics:moltis-live-rpc` pass in the current branch state.
- Before closing UI-only incidents, inspect stored runtime diagnostics via `list_ui_runtime_errors` (or direct DB query) to confirm no hidden frontend failures remain.
- For live incidents on a developer machine, run `npm run diagnostics:moltis-live` to validate the current app DB settings against real `/health` reachability and auth behavior.
- For full live path validation (`/health` + `/ws/chat` handshake + RPC), run `npm run diagnostics:moltis-live-rpc`.
- If live check fails with local sidecar mode, run `npm run diagnostics:moltis-validate-autonomous`; it first tries SSH bootstrap (`npm run diagnostics:moltis-start-ssh`) and then local source bootstrap (`npm run diagnostics:moltis-start-local`) before re-checking reachability.
