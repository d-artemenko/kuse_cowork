# Kuse Result Validation

Owner: Kuse app team
Last reviewed: 2026-03-13

## Purpose

Translate bridge and UI claims into explicit `PASS`, `BLOCKED`, or `FAIL` outcomes with executable evidence.

## Required Evidence

- Architectural boundary claims: `npm run check:moltis-only`
- Deterministic bridge/runtime claims: `npm run check:moltis-runtime-harness`
- Live bridge claims: `npm run diagnostics:moltis-live-rpc`
- Hidden frontend failure sweep: `npm run diagnostics:ui-errors`

## Status Rules

- `PASS`: the relevant gate ran and passed.
- `BLOCKED`: the live proof depends on absent DB settings, unreachable Moltis runtime, or another missing external precondition.
- `FAIL`: the gate ran and found a regression.

## Claim Rules

- Never say the Moltis bridge works from typecheck/build alone.
- Never close a UI runtime incident without checking stored runtime diagnostics.
- If live proof is blocked, record the blocker in the debt tracker or task log instead of downgrading the wording.
