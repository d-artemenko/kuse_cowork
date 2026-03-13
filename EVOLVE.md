# Kuse EVOLVE

Owner: Kuse app team
Last reviewed: 2026-03-13

## Rules

- Do not claim the Moltis bridge works from frontend build or cargo check; require `npm run check:moltis-runtime-harness` and, for live claims, `npm run diagnostics:moltis-live-rpc`.
- Do not reintroduce hidden model/provider fallback paths into the UI after the Moltis bridge became the runtime source of truth.
- Treat stored UI runtime errors as first-class evidence, not as optional debugging detail.
