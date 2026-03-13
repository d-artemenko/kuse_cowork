# Autonomy Levels

Owner: Kuse app team
Last reviewed: 2026-03-13

## Level 1: Deterministic Local Changes

Proceed without confirmation for docs, additive tests, UI-only refactors, and non-breaking harness work.

## Level 2: Contract-Sensitive Changes

State assumptions explicitly for Tauri command payload changes, settings schema changes, and Moltis bridge behavior changes.

## Level 3: Runtime-Affecting Changes

Pause if the change can alter local sidecar lifecycle, persisted app settings semantics, or user-visible chat/task routing.

## Level 4: External-Effect Changes

Do not run autonomously when the action touches remote Moltis credentials, production endpoints, or destructive local data operations.

## See Also

- Harness index: [README.md](README.md)
- Quality gates: [quality-gates.md](quality-gates.md)
- Merge playbook: [merge-playbook.md](merge-playbook.md)
