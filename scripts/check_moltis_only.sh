#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0

check_absent() {
  local label="$1"
  local pattern="$2"
  shift 2

  if rg -n "$pattern" "$@" >/tmp/moltis_only_matches.txt; then
    echo "[FAIL] $label"
    cat /tmp/moltis_only_matches.txt
    fail=1
  else
    echo "[OK] $label"
  fi
}

check_absent "No web ai-client runtime fallback" 'import\("\./ai-client"\)' src
check_absent "No UI calls to sendChatMessage" 'sendChatMessage\(' src/components src/stores src/App.tsx
check_absent "No UI calls to sendChatWithTools" 'sendChatWithTools\(' src/components src/stores src/App.tsx
check_absent "Settings does not expose model/provider selector" 'ModelSelector|API Configuration|Test Connection' src/components/Settings.tsx

if [[ $fail -ne 0 ]]; then
  echo "Moltis-only check failed"
  exit 1
fi

echo "Moltis-only check passed"
