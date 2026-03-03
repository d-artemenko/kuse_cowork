#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "Checking current Moltis live status..."
if ! python3 scripts/check_moltis_live.py; then
  echo "Live check failed; attempting SSH tunnel bootstrap..."
  if ! bash scripts/start_moltis_ssh_tunnel.sh; then
    echo "SSH tunnel bootstrap failed; attempting local sidecar bootstrap..."
    bash scripts/start_local_moltis_sidecar.sh
  fi

  echo "Re-checking Moltis live status after bootstrap..."
  python3 scripts/check_moltis_live.py
else
  echo "Moltis HTTP health already reachable with current app settings."
fi

echo "Validating live ws/rpc path..."
npm run diagnostics:moltis-live-rpc
echo "Autonomous Moltis validation passed."
