# Kuse Harness Playbook

Owner: Kuse app team
Last reviewed: 2026-03-13

## Purpose

Keep the desktop app operable and explainable when the user path depends on the Moltis bridge, local Tauri state, and stored runtime diagnostics.

## Contents

- [architecture-guardrails.md](architecture-guardrails.md): Contract boundaries for UI, Tauri backend, and Moltis bridge code.
- [quality-gates.md](quality-gates.md): Required mechanical and runtime checks.
- [result-validation.md](result-validation.md): `PASS|BLOCKED|FAIL` claim policy.
- [autonomy-levels.md](autonomy-levels.md): Escalation thresholds for local vs external-effect work.
- [merge-playbook.md](merge-playbook.md): Commit and verification discipline.
- [feedback-loop.md](feedback-loop.md): Build/verify/observe loop for desktop runtime work.
- [tech-debt-tracker.md](tech-debt-tracker.md): Open harness and product risks.
- [docs-freshness-policy.md](docs-freshness-policy.md): Refresh cadence for docs and validation commands.

## See Also

- Docs index: [../README.md](../README.md)
- Architecture: [../../ARCHITECTURE.md](../../ARCHITECTURE.md)
