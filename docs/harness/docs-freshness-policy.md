# Docs Freshness Policy

Owner: Kuse app team
Last reviewed: 2026-03-13

## Review Cadence

- Weekly: `docs/harness/*` and Moltis bridge diagnostics commands.
- On bridge contract changes: `ARCHITECTURE.md`, `docs/harness/quality-gates.md`, and `docs/harness/result-validation.md`.
- Before merge of runtime-affecting changes: revalidate `npm run check:moltis-only` and `npm run check:moltis-runtime-harness`.

## Staleness Signals

- Missing owner or outdated review date.
- Commands that no longer reflect the shipped bridge/runtime path.
- Docs that still describe pre-Moltis fallback behavior after the code removed it.

## Cleanup Rules

- Archive stale execution plans after completion.
- Update docs in the same change set as bridge or diagnostics behavior changes.
- Keep `AGENTS.md` short and route detail into harness/docs nodes.

## See Also

- Harness index: [README.md](README.md)
- Tech debt tracker: [tech-debt-tracker.md](tech-debt-tracker.md)
- Execution plans: [../exec-plans/README.md](../exec-plans/README.md)
