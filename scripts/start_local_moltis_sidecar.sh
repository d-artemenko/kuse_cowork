#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MOLTIS_REPO="${MOLTIS_REPO:-$ROOT/../moltis}"
RUNTIME_DIR="$ROOT/.runtime/moltis-sidecar"
PID_FILE="$RUNTIME_DIR/moltis.pid"
LOG_FILE="$RUNTIME_DIR/moltis.log"
CFG_DIR="$RUNTIME_DIR/config"
DATA_DIR="$RUNTIME_DIR/data"
RUSTUP_BIN="${RUSTUP_BIN:-$HOME/.cargo/bin/rustup}"
HEALTH_TIMEOUT_SECONDS="${MOLTIS_HEALTH_TIMEOUT_SECONDS:-600}"
MOLTIS_CARGO_FEATURES="${MOLTIS_CARGO_FEATURES:-lightweight}"

mkdir -p "$RUNTIME_DIR" "$CFG_DIR" "$DATA_DIR"

if [[ ! -d "$MOLTIS_REPO" ]]; then
  echo "Moltis repo not found: $MOLTIS_REPO" >&2
  exit 1
fi

if ! command -v cmake >/dev/null 2>&1; then
  echo "cmake is required to build local Moltis sidecar" >&2
  if command -v brew >/dev/null 2>&1; then
    echo "Install it with: brew install cmake" >&2
  fi
  exit 1
fi

if [[ ! -x "$RUSTUP_BIN" ]]; then
  echo "rustup not found at $RUSTUP_BIN; cannot run Moltis with pinned nightly toolchain" >&2
  exit 1
fi

TOOLCHAIN_FILE="$MOLTIS_REPO/rust-toolchain.toml"
if [[ ! -f "$TOOLCHAIN_FILE" ]]; then
  echo "Missing toolchain file: $TOOLCHAIN_FILE" >&2
  exit 1
fi

TOOLCHAIN="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' "$TOOLCHAIN_FILE" | head -n1)"
if [[ -z "$TOOLCHAIN" ]]; then
  echo "Failed to read Rust toolchain channel from $TOOLCHAIN_FILE" >&2
  exit 1
fi

if ! "$RUSTUP_BIN" toolchain list | grep -q "^${TOOLCHAIN}\b"; then
  echo "Installing required Rust toolchain: $TOOLCHAIN"
  "$RUSTUP_BIN" toolchain install "$TOOLCHAIN"
fi

RUSTC_BIN="$("$RUSTUP_BIN" which --toolchain "$TOOLCHAIN" rustc)"
RUSTDOC_BIN="$("$RUSTUP_BIN" which --toolchain "$TOOLCHAIN" rustdoc)"
CARGO_BIN="$("$RUSTUP_BIN" which --toolchain "$TOOLCHAIN" cargo)"

if [[ ! -x "$RUSTC_BIN" || ! -x "$RUSTDOC_BIN" || ! -x "$CARGO_BIN" ]]; then
  echo "Failed to resolve nightly rust binaries for toolchain $TOOLCHAIN" >&2
  exit 1
fi

if [[ -f "$PID_FILE" ]]; then
  old_pid="$(cat "$PID_FILE" 2>/dev/null || true)"
  if [[ -n "${old_pid}" ]] && kill -0 "$old_pid" 2>/dev/null; then
    echo "Local Moltis sidecar already running (pid=$old_pid)"
    exit 0
  fi
fi

echo "Starting local Moltis sidecar from $MOLTIS_REPO (features=$MOLTIS_CARGO_FEATURES) ..."
(
  cd "$MOLTIS_REPO"
  nohup env \
    PATH="$(dirname "$RUSTC_BIN"):$PATH" \
    RUSTC="$RUSTC_BIN" \
    RUSTDOC="$RUSTDOC_BIN" \
    MOLTIS_CONFIG_DIR="$CFG_DIR" \
    MOLTIS_DATA_DIR="$DATA_DIR" \
    "$CARGO_BIN" run --bin moltis --no-default-features --features "$MOLTIS_CARGO_FEATURES" -- --bind 127.0.0.1 --port 13131 --no-tls \
    >"$LOG_FILE" 2>&1 &
  echo $! >"$PID_FILE"
)

pid="$(cat "$PID_FILE")"
echo "Spawned Moltis process pid=$pid"
echo "Waiting for health endpoint..."

for _ in $(seq 1 "$HEALTH_TIMEOUT_SECONDS"); do
  if curl -fsS --max-time 2 "http://127.0.0.1:13131/health" >/dev/null 2>&1; then
    echo "Moltis sidecar is healthy at http://127.0.0.1:13131"
    exit 0
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "Moltis process exited early. See $LOG_FILE" >&2
    tail -n 120 "$LOG_FILE" >&2 || true
    exit 1
  fi
  sleep 1
done

echo "Timed out after ${HEALTH_TIMEOUT_SECONDS}s waiting for Moltis health. See $LOG_FILE" >&2
exit 1
