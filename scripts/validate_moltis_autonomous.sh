#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "Checking current Moltis live status..."
if python3 scripts/check_moltis_live.py; then
  echo "Moltis already reachable with current app settings."
  exit 0
fi

echo "Live check failed; attempting SSH tunnel bootstrap..."
if ! bash scripts/start_moltis_ssh_tunnel.sh; then
  echo "SSH tunnel bootstrap failed; attempting local sidecar bootstrap..."
  bash scripts/start_local_moltis_sidecar.sh
fi

echo "Re-checking Moltis live status after bootstrap..."
python3 scripts/check_moltis_live.py
echo "Autonomous Moltis validation passed."
