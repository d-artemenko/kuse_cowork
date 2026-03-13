# Feedback Loop

Owner: Kuse app team
Last reviewed: 2026-03-13

Use a tight loop the user can actually experience:

`intent -> implement -> mechanical gates -> live diagnostics -> inspect stored errors -> record verdict`

## What This Means In Practice

- Run `npm run check:moltis-only` for architecture boundaries.
- Run `npm run check:moltis-runtime-harness` for the deterministic backend/UI regression surface.
- Run `npm run diagnostics:moltis-live` or `npm run diagnostics:moltis-live-rpc` before strong bridge operability claims.
- Inspect `npm run diagnostics:ui-errors` before closing incidents that could have hidden frontend failures.

## See Also

- Harness index: [README.md](README.md)
- Quality gates: [quality-gates.md](quality-gates.md)
- Merge playbook: [merge-playbook.md](merge-playbook.md)
- Autonomy levels: [autonomy-levels.md](autonomy-levels.md)
