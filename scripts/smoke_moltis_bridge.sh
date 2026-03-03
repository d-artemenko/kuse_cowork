#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
bash "$ROOT/scripts/check_moltis_runtime_harness.sh"

cd "$ROOT/src-tauri"
cargo test bridge_helpers_handle_health_connect_and_rpc -- --nocapture
cargo test moltis_connection_status_reports_success_and_validation_errors -- --nocapture
