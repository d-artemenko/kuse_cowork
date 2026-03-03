#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME_DIR="$ROOT/.runtime/moltis-sidecar"
LOG_FILE="$RUNTIME_DIR/moltis-ssh-tunnel.log"
CONTROL_SOCKET="$RUNTIME_DIR/moltis-ssh-tunnel.sock"

SSH_BIN="${SSH_BIN:-ssh}"
SSH_HOST="${MOLTIS_SSH_HOST:-agent}"
LOCAL_BIND="${MOLTIS_LOCAL_BIND:-127.0.0.1}"
LOCAL_PORT="${MOLTIS_LOCAL_PORT:-13131}"
REMOTE_BIND="${MOLTIS_REMOTE_BIND:-127.0.0.1}"
REMOTE_PORT="${MOLTIS_REMOTE_PORT:-13131}"
HEALTH_TIMEOUT_SECONDS="${MOLTIS_SSH_HEALTH_TIMEOUT_SECONDS:-30}"
HEALTH_URL="http://${LOCAL_BIND}:${LOCAL_PORT}/health"

mkdir -p "$RUNTIME_DIR"

if ! command -v "$SSH_BIN" >/dev/null 2>&1; then
  echo "ssh client not found" >&2
  exit 1
fi

tunnel_active() {
  [[ -S "$CONTROL_SOCKET" ]] && "$SSH_BIN" -S "$CONTROL_SOCKET" -O check "$SSH_HOST" >/dev/null 2>&1
}

if curl -fsS --max-time 2 "$HEALTH_URL" >/dev/null 2>&1; then
  echo "Moltis is already reachable at $HEALTH_URL"
  exit 0
fi

if tunnel_active; then
  echo "Existing SSH control tunnel detected; waiting for health..."
else
  rm -f "$CONTROL_SOCKET"
  echo "Starting Moltis SSH tunnel: ${LOCAL_BIND}:${LOCAL_PORT} -> ${SSH_HOST}:${REMOTE_BIND}:${REMOTE_PORT}"
  "$SSH_BIN" \
    -M \
    -S "$CONTROL_SOCKET" \
    -o BatchMode=yes \
    -o ExitOnForwardFailure=yes \
    -o ServerAliveInterval=30 \
    -o ServerAliveCountMax=3 \
    -E "$LOG_FILE" \
    -f \
    -N \
    -L "${LOCAL_BIND}:${LOCAL_PORT}:${REMOTE_BIND}:${REMOTE_PORT}" \
    "$SSH_HOST"
fi

for _ in $(seq 1 "$HEALTH_TIMEOUT_SECONDS"); do
  if curl -fsS --max-time 2 "$HEALTH_URL" >/dev/null 2>&1; then
    echo "Moltis reachable through SSH tunnel at $HEALTH_URL"
    exit 0
  fi
  if ! tunnel_active; then
    echo "SSH tunnel is not active. See $LOG_FILE" >&2
    tail -n 120 "$LOG_FILE" >&2 || true
    exit 1
  fi
  sleep 1
done

echo "Timed out after ${HEALTH_TIMEOUT_SECONDS}s waiting for SSH tunnel health at $HEALTH_URL" >&2
tail -n 120 "$LOG_FILE" >&2 || true
exit 1
