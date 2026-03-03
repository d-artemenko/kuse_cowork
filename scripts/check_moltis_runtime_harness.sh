#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/src-tauri"

cargo test bridge_helpers_propagate_auth_for_health_connect_and_chat -- --nocapture
cargo test moltis_connection_status_reports_unauthorized_without_api_key -- --nocapture
cargo test send_chat_message_via_moltis_with_db_falls_back_on_model_not_found -- --nocapture
cargo test send_task_message_via_moltis_ignores_legacy_hidden_model_setting -- --nocapture
cargo test store_ui_runtime_error_validates_required_fields_and_truncates -- --nocapture
cargo test ui_runtime_error_round_trip_and_limit -- --nocapture
