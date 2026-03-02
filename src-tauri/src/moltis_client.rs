use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use thiserror::Error;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Message as WsMessage,
        client::IntoClientRequest,
        http::{
            HeaderValue,
            header::{AUTHORIZATION, HeaderName},
        },
    },
};
use url::Url;

const DEFAULT_MIN_PROTOCOL: u32 = 3;
const DEFAULT_MAX_PROTOCOL: u32 = 4;
const DEFAULT_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct MoltisClientConfig {
    pub server_url: String,
    pub api_key: Option<String>,
    pub min_protocol: u32,
    pub max_protocol: u32,
    pub timeout: Duration,
}

impl MoltisClientConfig {
    pub fn new(server_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            server_url: server_url.into(),
            api_key,
            min_protocol: DEFAULT_MIN_PROTOCOL,
            max_protocol: DEFAULT_MAX_PROTOCOL,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MoltisClient {
    cfg: MoltisClientConfig,
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct ChatSendResult {
    pub text: String,
}

#[derive(Debug, Error)]
pub enum MoltisClientError {
    #[error("Moltis server URL is empty")]
    EmptyServerUrl,
    #[error("Moltis server URL is invalid: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("Unsupported URL scheme '{0}'. Use http/https/ws/wss.")]
    UnsupportedScheme(String),
    #[error("Invalid header value: {0}")]
    InvalidHeader(#[from] tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue),
    #[error("WebSocket request build failed: {0}")]
    RequestBuild(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON parse failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Operation timed out while waiting for {0}")]
    Timeout(&'static str),
    #[error("WebSocket closed before response arrived")]
    SocketClosed,
    #[error("Connection handshake failed: {0}")]
    Handshake(String),
    #[error("Protocol mismatch: server={server}, client supports {min}..{max}")]
    ProtocolMismatch { server: u32, min: u32, max: u32 },
    #[error("RPC error [{code}]: {message}")]
    Rpc { code: String, message: String },
}

impl MoltisClient {
    pub fn new(cfg: MoltisClientConfig) -> Self {
        Self {
            cfg,
            http: reqwest::Client::new(),
        }
    }

    pub async fn health(&self) -> Result<Value, MoltisClientError> {
        let mut url = self.http_base_url()?;
        url.set_path("/health");
        url.set_query(None);
        url.set_fragment(None);
        let response = self
            .http
            .get(url.as_str())
            .timeout(self.cfg.timeout)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn check_ws_connection(&self) -> Result<Value, MoltisClientError> {
        let mut socket = self.connect().await?;
        // We only need hello-ok payload for connection diagnostics.
        let hello = self.perform_handshake(&mut socket).await?;
        let _ = socket.close(None).await;
        Ok(hello)
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, MoltisClientError> {
        let mut socket = self.connect().await?;
        let _ = self.perform_handshake(&mut socket).await?;

        let request_id = format!("req-{}", uuid::Uuid::new_v4());
        self.send_json(
            &mut socket,
            json!({
                "type": "req",
                "id": request_id,
                "method": method,
                "params": params,
            }),
        )
        .await?;

        let response = self.wait_for_response(&mut socket, &request_id).await?;
        let _ = socket.close(None).await;
        Ok(response)
    }

    pub async fn chat_send_and_wait(
        &self,
        session_key: &str,
        text: &str,
        model: Option<&str>,
    ) -> Result<ChatSendResult, MoltisClientError> {
        let mut socket = self.connect().await?;
        let _ = self.perform_handshake(&mut socket).await?;

        let mut params = json!({
            "text": text,
            "_session_key": session_key,
        });
        if let Some(model_id) = model {
            if !model_id.trim().is_empty() {
                params["model"] = Value::String(model_id.to_string());
            }
        }

        let request_id = format!("chat-send-{}", uuid::Uuid::new_v4());
        self.send_json(
            &mut socket,
            json!({
                "type": "req",
                "id": request_id,
                "method": "chat.send",
                "params": params,
            }),
        )
        .await?;

        let send_payload = self.wait_for_response(&mut socket, &request_id).await?;
        let run_id = send_payload
            .get("runId")
            .and_then(Value::as_str)
            .ok_or_else(|| MoltisClientError::Handshake("chat.send missing runId".to_string()))?
            .to_string();

        let mut accumulated = String::new();
        loop {
            let frame = self.next_json_message(&mut socket, "chat final event").await?;
            if frame.get("type").and_then(Value::as_str) != Some("event") {
                continue;
            }
            if frame.get("event").and_then(Value::as_str) != Some("chat") {
                continue;
            }
            let Some(payload) = frame.get("payload") else {
                continue;
            };
            if let Some(payload_run_id) = payload.get("runId").and_then(Value::as_str) {
                if payload_run_id != run_id {
                    continue;
                }
            }
            if let Some(payload_session) = payload.get("sessionKey").and_then(Value::as_str) {
                if payload_session != session_key {
                    continue;
                }
            }

            let state = payload.get("state").and_then(Value::as_str).unwrap_or("");
            match state {
                "delta" => {
                    if let Some(delta) = payload.get("text").and_then(Value::as_str) {
                        accumulated.push_str(delta);
                    }
                }
                "final" => {
                    let text = payload
                        .get("text")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or(accumulated);
                    let _ = socket.close(None).await;
                    return Ok(ChatSendResult { text });
                }
                "error" => {
                    let message = payload
                        .get("error")
                        .and_then(|err| err.get("detail").or_else(|| err.get("message")))
                        .and_then(Value::as_str)
                        .or_else(|| payload.get("message").and_then(Value::as_str))
                        .unwrap_or("chat failed")
                        .to_string();
                    let _ = socket.close(None).await;
                    return Err(MoltisClientError::Rpc {
                        code: "CHAT_ERROR".to_string(),
                        message,
                    });
                }
                _ => {}
            }
        }
    }

    fn normalize_base_url(&self) -> Result<Url, MoltisClientError> {
        let raw = self.cfg.server_url.trim();
        if raw.is_empty() {
            return Err(MoltisClientError::EmptyServerUrl);
        }
        let candidate = if raw.contains("://") {
            raw.to_string()
        } else {
            format!("http://{raw}")
        };
        let mut url = Url::parse(&candidate)?;
        let scheme = url.scheme().to_string();
        match scheme.as_str() {
            "http" | "https" | "ws" | "wss" => {}
            _ => return Err(MoltisClientError::UnsupportedScheme(scheme)),
        }
        if url.path().is_empty() {
            url.set_path("/");
        }
        Ok(url)
    }

    fn http_base_url(&self) -> Result<Url, MoltisClientError> {
        let mut url = self.normalize_base_url()?;
        match url.scheme() {
            "ws" => {
                let _ = url.set_scheme("http");
            }
            "wss" => {
                let _ = url.set_scheme("https");
            }
            _ => {}
        }
        Ok(url)
    }

    fn websocket_url(&self) -> Result<Url, MoltisClientError> {
        let mut url = self.normalize_base_url()?;
        match url.scheme() {
            "http" => {
                let _ = url.set_scheme("ws");
            }
            "https" => {
                let _ = url.set_scheme("wss");
            }
            _ => {}
        }
        url.set_path("/ws/chat");
        url.set_query(None);
        url.set_fragment(None);
        Ok(url)
    }

    async fn connect(
        &self,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        MoltisClientError,
    > {
        let ws_url = self.websocket_url()?;
        let mut request = ws_url.as_str().into_client_request()?;
        if let Some(key) = self.cfg.api_key.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty()) {
            request.headers_mut().insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {key}"))?,
            );
        }
        // Add a lightweight client marker for server logs/troubleshooting.
        request
            .headers_mut()
            .insert(HeaderName::from_static("x-client-id"), HeaderValue::from_static("kuse-cowork"));

        let (socket, _) = tokio::time::timeout(self.cfg.timeout, connect_async(request))
            .await
            .map_err(|_| MoltisClientError::Timeout("websocket connect"))??;
        Ok(socket)
    }

    async fn perform_handshake(
        &self,
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> Result<Value, MoltisClientError> {
        let connect_id = format!("connect-{}", uuid::Uuid::new_v4());
        let mut params = json!({
            "minProtocol": self.cfg.min_protocol,
            "maxProtocol": self.cfg.max_protocol,
            "client": {
                "id": "kuse-cowork",
                "version": env!("CARGO_PKG_VERSION"),
                "platform": std::env::consts::OS,
                "mode": "operator"
            }
        });
        if let Some(key) = self.cfg.api_key.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty()) {
            params["auth"] = json!({ "api_key": key });
        }

        self.send_json(
            socket,
            json!({
                "type": "req",
                "id": connect_id,
                "method": "connect",
                "params": params,
            }),
        )
        .await?;

        let payload = self.wait_for_response(socket, &connect_id).await?;
        if payload.get("type").and_then(Value::as_str) != Some("hello-ok") {
            return Err(MoltisClientError::Handshake(
                "missing hello-ok payload".to_string(),
            ));
        }
        let protocol = payload
            .get("protocol")
            .and_then(Value::as_u64)
            .map(|v| v as u32)
            .ok_or_else(|| MoltisClientError::Handshake("missing protocol value".to_string()))?;
        if protocol < self.cfg.min_protocol || protocol > self.cfg.max_protocol {
            return Err(MoltisClientError::ProtocolMismatch {
                server: protocol,
                min: self.cfg.min_protocol,
                max: self.cfg.max_protocol,
            });
        }
        Ok(payload)
    }

    async fn send_json(
        &self,
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        value: Value,
    ) -> Result<(), MoltisClientError> {
        let text = serde_json::to_string(&value)?;
        tokio::time::timeout(self.cfg.timeout, socket.send(WsMessage::Text(text.into())))
            .await
            .map_err(|_| MoltisClientError::Timeout("websocket send"))?
            .map_err(MoltisClientError::RequestBuild)?;
        Ok(())
    }

    async fn wait_for_response(
        &self,
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        request_id: &str,
    ) -> Result<Value, MoltisClientError> {
        loop {
            let frame = self.next_json_message(socket, "RPC response").await?;
            if frame.get("type").and_then(Value::as_str) != Some("res") {
                continue;
            }
            if frame.get("id").and_then(Value::as_str) != Some(request_id) {
                continue;
            }
            let ok = frame.get("ok").and_then(Value::as_bool).unwrap_or(false);
            if ok {
                return Ok(frame.get("payload").cloned().unwrap_or(Value::Null));
            }
            let error = frame.get("error").cloned().unwrap_or(Value::Null);
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("RPC_ERROR")
                .to_string();
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("request failed")
                .to_string();
            return Err(MoltisClientError::Rpc { code, message });
        }
    }

    async fn next_json_message(
        &self,
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        waiting_for: &'static str,
    ) -> Result<Value, MoltisClientError> {
        loop {
            let message = tokio::time::timeout(self.cfg.timeout, socket.next())
                .await
                .map_err(|_| MoltisClientError::Timeout(waiting_for))?;
            match message {
                Some(Ok(WsMessage::Text(text))) => return Ok(serde_json::from_str::<Value>(&text)?),
                Some(Ok(WsMessage::Binary(bytes))) => {
                    let text =
                        String::from_utf8(bytes.to_vec()).map_err(|e| MoltisClientError::Handshake(e.to_string()))?;
                    return Ok(serde_json::from_str::<Value>(&text)?);
                }
                Some(Ok(WsMessage::Ping(payload))) => {
                    socket
                        .send(WsMessage::Pong(payload))
                        .await
                        .map_err(MoltisClientError::RequestBuild)?;
                }
                Some(Ok(WsMessage::Pong(_))) => {}
                Some(Ok(WsMessage::Frame(_))) => {}
                Some(Ok(WsMessage::Close(_))) => return Err(MoltisClientError::SocketClosed),
                Some(Err(err)) => return Err(MoltisClientError::RequestBuild(err)),
                None => return Err(MoltisClientError::SocketClosed),
            }
        }
    }
}
