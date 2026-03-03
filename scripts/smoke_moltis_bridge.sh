#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/src-tauri"

cargo test bridge_helpers_handle_health_connect_and_rpc -- --nocapture
cargo test moltis_connection_status_reports_success_and_validation_errors -- --nocapture
cargo test send_chat_message_via_moltis_with_db_falls_back_on_model_not_found -- --nocapture
