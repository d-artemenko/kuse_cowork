# Tech Debt Tracker

Owner: Kuse app team
Last reviewed: 2026-03-13

## Entries

| ID | Area | Issue | Owner | Opened | Last Touched | Next Action | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| KUSE-001 | Product integration | Desktop still lacks the full Telegram mirror + reply routing worker against server Moltis. | App | 2026-03-02 | 2026-03-13 | Implement sync adapter and replay-safe cursor persistence. | Open |
| KUSE-002 | Harness | Live bridge proof depends on developer-local DB settings and reachable Moltis endpoint; there is no hermetic fixture for CI live diagnostics. | App | 2026-03-13 | 2026-03-13 | Add reproducible local fixture for `diagnostics:moltis-live-rpc` or explicit CI-safe fallback artifact. | Open |
| KUSE-003 | Diagnostics | UI runtime error storage is inspectable locally but not exported as a CI artifact when runtime harness tests fail. | App | 2026-03-13 | 2026-03-13 | Persist `diagnostics:ui-errors` output as workflow artifact on failure. | Open |

## See Also

- Harness index: [README.md](README.md)
- Docs freshness: [docs-freshness-policy.md](docs-freshness-policy.md)
