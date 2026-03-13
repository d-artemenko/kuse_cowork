# Merge Playbook

Owner: Kuse app team
Last reviewed: 2026-03-13

## Goal

Ship small desktop/runtime slices while preserving proof for the Moltis bridge path.

## Practices

- Keep UI, backend, and docs changes in the same slice when they modify the same runtime path.
- Include the exact harness commands run for the touched scope.
- Treat `BLOCKED` live diagnostics as unfinished operability proof, not as merge-ready success.
- Record unresolved runtime/harness gaps in the debt tracker before merge.

## See Also

- Harness index: [README.md](README.md)
- Tech debt tracker: [tech-debt-tracker.md](tech-debt-tracker.md)
- Quality gates: [quality-gates.md](quality-gates.md)
