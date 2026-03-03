use serde_json::{json, Value};

#[path = "../database.rs"]
mod database;
#[path = "../moltis_client.rs"]
mod moltis_client;

fn normalize_base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("moltis_server_url is empty in app settings".to_string());
    }
    if trimmed.contains("://") {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("http://{trimmed}"))
    }
}

#[tokio::main]
async fn main() {
    let mut payload = json!({
        "ok": false,
    });

    let result: Result<(), String> = async {
        let db = database::Database::new().map_err(|e| e.to_string())?;
        let settings = db.get_settings().map_err(|e| e.to_string())?;
        let server_url = normalize_base_url(&settings.moltis_server_url)?;
        let api_key = settings.moltis_api_key.trim().to_string();
        let api_key_opt = if api_key.is_empty() {
            None
        } else {
            Some(api_key.clone())
        };

        payload["server_url"] = Value::String(server_url.clone());
        payload["sidecar_enabled"] = Value::Bool(settings.moltis_sidecar_enabled);
        payload["auth_mode"] = Value::String(if api_key_opt.is_some() {
            "bearer".to_string()
        } else {
            "none".to_string()
        });

        let client = moltis_client::MoltisClient::new(moltis_client::MoltisClientConfig::new(
            server_url,
            api_key_opt,
        ));

        let health = client.health().await.map_err(|e| e.to_string())?;
        payload["http_health"] = health;

        let ws_hello = client
            .check_ws_connection()
            .await
            .map_err(|e| e.to_string())?;
        let ws_summary = json!({
            "type": ws_hello.get("type").cloned().unwrap_or(Value::Null),
            "protocol": ws_hello.get("protocol").cloned().unwrap_or(Value::Null),
            "server": ws_hello.get("server").cloned().unwrap_or(Value::Null),
            "auth_role": ws_hello
                .get("auth")
                .and_then(|auth| auth.get("role"))
                .cloned()
                .unwrap_or(Value::Null),
            "auth_scopes_count": ws_hello
                .get("auth")
                .and_then(|auth| auth.get("scopes"))
                .and_then(Value::as_array)
                .map(|arr| arr.len() as u64)
                .unwrap_or(0),
        });
        payload["ws_hello"] = ws_summary;

        let rpc_health = client
            .call("health", json!({}))
            .await
            .map_err(|e| e.to_string())?;
        payload["rpc_health"] = rpc_health;

        payload["ok"] = Value::Bool(true);
        Ok(())
    }
    .await;

    if let Err(error) = result {
        payload["error"] = Value::String(error);
        let maybe_server_url = payload
            .get("server_url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let sidecar_enabled = payload
            .get("sidecar_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if sidecar_enabled
            && (maybe_server_url.contains("127.0.0.1")
                || maybe_server_url.contains("localhost")
                || maybe_server_url.contains("[::1]"))
        {
            payload["hint"] = Value::String(
                "sidecar enabled with local URL, but Moltis ws/rpc path is not reachable; run diagnostics:moltis-validate-autonomous"
                    .to_string(),
            );
        }
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
    );
    std::process::exit(if payload.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        0
    } else {
        1
    });
}
