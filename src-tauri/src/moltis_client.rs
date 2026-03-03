use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        http::{
            header::{HeaderName, AUTHORIZATION},
            HeaderValue,
        },
        Message as WsMessage,
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
            let frame = self
                .next_json_message(&mut socket, "chat final event")
                .await?;
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
        if let Some(key) = self
            .cfg
            .api_key
            .as_ref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            request.headers_mut().insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {key}"))?,
            );
        }
        // Add a lightweight client marker for server logs/troubleshooting.
        request.headers_mut().insert(
            HeaderName::from_static("x-client-id"),
            HeaderValue::from_static("kuse-cowork"),
        );

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
        if let Some(key) = self
            .cfg
            .api_key
            .as_ref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
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
                Some(Ok(WsMessage::Text(text))) => {
                    return Ok(serde_json::from_str::<Value>(&text)?)
                }
                Some(Ok(WsMessage::Binary(bytes))) => {
                    let text = String::from_utf8(bytes.to_vec())
                        .map_err(|e| MoltisClientError::Handshake(e.to_string()))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio_tungstenite::accept_hdr_async;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

    fn test_client(server_url: String, api_key: Option<&str>) -> MoltisClient {
        let mut cfg = MoltisClientConfig::new(server_url, api_key.map(ToString::to_string));
        cfg.timeout = Duration::from_secs(3);
        MoltisClient::new(cfg)
    }

    async fn spawn_health_server(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 2048];
            let _ = stream.read(&mut buf).await.unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{addr}")
    }

    async fn spawn_ws_server<F, Fut>(handler: F) -> String
    where
        F: FnOnce(tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> Fut
            + Send
            + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            handler(socket).await;
        });
        format!("ws://{addr}")
    }

    async fn ws_read_json(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    ) -> Value {
        match socket.next().await {
            Some(Ok(WsMessage::Text(text))) => serde_json::from_str(&text).unwrap(),
            Some(Ok(WsMessage::Binary(bytes))) => serde_json::from_slice(&bytes).unwrap(),
            other => panic!("unexpected ws message: {other:?}"),
        }
    }

    async fn ws_send_json(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        value: Value,
    ) {
        let text = serde_json::to_string(&value).unwrap();
        socket.send(WsMessage::Text(text.into())).await.unwrap();
    }

    async fn ws_handshake_ok(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        protocol: u32,
    ) {
        let connect = ws_read_json(socket).await;
        let connect_id = connect
            .get("id")
            .and_then(Value::as_str)
            .unwrap()
            .to_string();
        ws_send_json(
            socket,
            json!({
                "type": "res",
                "id": connect_id,
                "ok": true,
                "payload": {
                    "type": "hello-ok",
                    "protocol": protocol
                }
            }),
        )
        .await;
    }

    #[test]
    fn url_helpers_cover_basic_validation_and_conversion() {
        let client = test_client("127.0.0.1:13131".to_string(), None);
        assert_eq!(
            client.normalize_base_url().unwrap().as_str(),
            "http://127.0.0.1:13131/"
        );
        assert_eq!(client.http_base_url().unwrap().scheme(), "http");
        assert_eq!(
            client.websocket_url().unwrap().as_str(),
            "ws://127.0.0.1:13131/ws/chat"
        );

        let secure = test_client("https://example.com".to_string(), None);
        assert_eq!(secure.websocket_url().unwrap().scheme(), "wss");

        let ws = test_client("ws://example.com".to_string(), None);
        assert_eq!(ws.http_base_url().unwrap().scheme(), "http");

        let empty = test_client("   ".to_string(), None);
        assert!(matches!(
            empty.normalize_base_url(),
            Err(MoltisClientError::EmptyServerUrl)
        ));

        let invalid = test_client("ftp://example.com".to_string(), None);
        assert!(matches!(
            invalid.normalize_base_url(),
            Err(MoltisClientError::UnsupportedScheme(s)) if s == "ftp"
        ));
    }

    #[tokio::test]
    async fn health_returns_json_payload() {
        let base = spawn_health_server(r#"{"status":"ok","version":"1.2.3"}"#).await;
        let client = test_client(base, None);
        let health = client.health().await.unwrap();
        assert_eq!(health.get("status").and_then(Value::as_str), Some("ok"));
        assert_eq!(health.get("version").and_then(Value::as_str), Some("1.2.3"));
    }

    #[tokio::test]
    async fn check_ws_connection_sends_expected_headers_and_accepts_protocol() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (hdr_tx, hdr_rx) = oneshot::channel::<(Option<String>, Option<String>)>();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let hdr_sender = Arc::new(std::sync::Mutex::new(Some(hdr_tx)));
            let socket = accept_hdr_async(stream, {
                let hdr_sender = Arc::clone(&hdr_sender);
                move |req: &Request, resp: Response| {
                    let auth = req
                        .headers()
                        .get(AUTHORIZATION)
                        .and_then(|h| h.to_str().ok())
                        .map(ToString::to_string);
                    let client_id = req
                        .headers()
                        .get("x-client-id")
                        .and_then(|h| h.to_str().ok())
                        .map(ToString::to_string);
                    if let Some(tx) = hdr_sender.lock().unwrap().take() {
                        let _ = tx.send((auth, client_id));
                    }
                    Ok(resp)
                }
            })
            .await
            .unwrap();
            let mut socket = socket;
            ws_handshake_ok(&mut socket, 3).await;
        });

        let client = test_client(format!("ws://{addr}"), Some("secret-key"));
        let hello = client.check_ws_connection().await.unwrap();
        assert_eq!(hello.get("protocol").and_then(Value::as_u64), Some(3));

        let (auth, client_id) = hdr_rx.await.unwrap();
        assert_eq!(auth.as_deref(), Some("Bearer secret-key"));
        assert_eq!(client_id.as_deref(), Some("kuse-cowork"));
    }

    #[tokio::test]
    async fn check_ws_connection_rejects_protocol_out_of_range() {
        let ws_url = spawn_ws_server(|mut socket| async move {
            ws_handshake_ok(&mut socket, 99).await;
        })
        .await;

        let client = test_client(ws_url, None);
        assert!(matches!(
            client.check_ws_connection().await,
            Err(MoltisClientError::ProtocolMismatch {
                server: 99,
                min: 3,
                max: 4
            })
        ));
    }

    #[tokio::test]
    async fn call_handles_ping_and_binary_payload_response() {
        let ws_url = spawn_ws_server(|mut socket| async move {
            ws_handshake_ok(&mut socket, 3).await;
            let request = ws_read_json(&mut socket).await;
            let req_id = request
                .get("id")
                .and_then(Value::as_str)
                .unwrap()
                .to_string();
            socket
                .send(WsMessage::Ping(vec![1, 2, 3].into()))
                .await
                .unwrap();

            let frame = json!({
                "type":"res",
                "id": req_id,
                "ok": true,
                "payload": {"ok": true, "answer": 42}
            });
            let bytes = serde_json::to_vec(&frame).unwrap();
            socket.send(WsMessage::Binary(bytes.into())).await.unwrap();
        })
        .await;

        let client = test_client(ws_url, None);
        let payload = client.call("health", json!({})).await.unwrap();
        assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(payload.get("answer").and_then(Value::as_i64), Some(42));
    }

    #[tokio::test]
    async fn call_surfaces_rpc_error_payload() {
        let ws_url = spawn_ws_server(|mut socket| async move {
            ws_handshake_ok(&mut socket, 3).await;
            let request = ws_read_json(&mut socket).await;
            let req_id = request
                .get("id")
                .and_then(Value::as_str)
                .unwrap()
                .to_string();
            ws_send_json(
                &mut socket,
                json!({
                    "type":"res",
                    "id": req_id,
                    "ok": false,
                    "error": {"code":"NOPE","message":"request denied"}
                }),
            )
            .await;
        })
        .await;

        let client = test_client(ws_url, None);
        assert!(matches!(
            client.call("health", json!({})).await,
            Err(MoltisClientError::Rpc { code, message }) if code == "NOPE" && message == "request denied"
        ));
    }

    #[tokio::test]
    async fn chat_send_and_wait_uses_final_text_and_model_override() {
        let ws_url = spawn_ws_server(|mut socket| async move {
            ws_handshake_ok(&mut socket, 3).await;
            let send_req = ws_read_json(&mut socket).await;
            let req_id = send_req.get("id").and_then(Value::as_str).unwrap().to_string();
            assert_eq!(
                send_req
                    .get("params")
                    .and_then(|v| v.get("model"))
                    .and_then(Value::as_str),
                Some("openai::gpt-5")
            );

            ws_send_json(
                &mut socket,
                json!({
                    "type":"res",
                    "id": req_id,
                    "ok": true,
                    "payload": {"runId":"run-1"}
                }),
            )
            .await;
            ws_send_json(
                &mut socket,
                json!({
                    "type":"event",
                    "event":"chat",
                    "payload":{"runId":"run-1","sessionKey":"kuse:abc","state":"delta","text":"Hello "}
                }),
            )
            .await;
            ws_send_json(
                &mut socket,
                json!({
                    "type":"event",
                    "event":"chat",
                    "payload":{"runId":"run-1","sessionKey":"kuse:abc","state":"final","text":"Hello World"}
                }),
            )
            .await;
        })
        .await;

        let client = test_client(ws_url, None);
        let reply = client
            .chat_send_and_wait("kuse:abc", "Hi there", Some("openai::gpt-5"))
            .await
            .unwrap();
        assert_eq!(reply.text, "Hello World");
    }

    #[tokio::test]
    async fn chat_send_and_wait_falls_back_to_accumulated_text_when_final_empty() {
        let ws_url = spawn_ws_server(|mut socket| async move {
            ws_handshake_ok(&mut socket, 3).await;
            let send_req = ws_read_json(&mut socket).await;
            let req_id = send_req.get("id").and_then(Value::as_str).unwrap().to_string();

            ws_send_json(
                &mut socket,
                json!({
                    "type":"res",
                    "id": req_id,
                    "ok": true,
                    "payload": {"runId":"run-2"}
                }),
            )
            .await;
            ws_send_json(
                &mut socket,
                json!({
                    "type":"event",
                    "event":"chat",
                    "payload":{"runId":"run-2","sessionKey":"kuse:def","state":"delta","text":"Part 1 "}
                }),
            )
            .await;
            ws_send_json(
                &mut socket,
                json!({
                    "type":"event",
                    "event":"chat",
                    "payload":{"runId":"run-2","sessionKey":"kuse:def","state":"delta","text":"Part 2"}
                }),
            )
            .await;
            ws_send_json(
                &mut socket,
                json!({
                    "type":"event",
                    "event":"chat",
                    "payload":{"runId":"run-2","sessionKey":"kuse:def","state":"final","text":"   "}
                }),
            )
            .await;
        })
        .await;

        let client = test_client(ws_url, None);
        let reply = client
            .chat_send_and_wait("kuse:def", "Hi", None)
            .await
            .unwrap();
        assert_eq!(reply.text, "Part 1 Part 2");
    }

    #[tokio::test]
    async fn chat_send_and_wait_surfaces_chat_error_details() {
        let ws_url = spawn_ws_server(|mut socket| async move {
            ws_handshake_ok(&mut socket, 3).await;
            let send_req = ws_read_json(&mut socket).await;
            let req_id = send_req
                .get("id")
                .and_then(Value::as_str)
                .unwrap()
                .to_string();

            ws_send_json(
                &mut socket,
                json!({
                    "type":"res",
                    "id": req_id,
                    "ok": true,
                    "payload": {"runId":"run-3"}
                }),
            )
            .await;
            ws_send_json(
                &mut socket,
                json!({
                    "type":"event",
                    "event":"chat",
                    "payload":{
                        "runId":"run-3",
                        "sessionKey":"kuse:ghi",
                        "state":"error",
                        "error":{"detail":"model unavailable"}
                    }
                }),
            )
            .await;
        })
        .await;

        let client = test_client(ws_url, None);
        assert!(matches!(
            client.chat_send_and_wait("kuse:ghi", "Hi", None).await,
            Err(MoltisClientError::Rpc { code, message }) if code == "CHAT_ERROR" && message == "model unavailable"
        ));
    }

    #[tokio::test]
    async fn chat_send_and_wait_requires_run_id_in_chat_send_response() {
        let ws_url = spawn_ws_server(|mut socket| async move {
            ws_handshake_ok(&mut socket, 3).await;
            let send_req = ws_read_json(&mut socket).await;
            let req_id = send_req
                .get("id")
                .and_then(Value::as_str)
                .unwrap()
                .to_string();

            ws_send_json(
                &mut socket,
                json!({
                    "type":"res",
                    "id": req_id,
                    "ok": true,
                    "payload": {"status":"ok"}
                }),
            )
            .await;
        })
        .await;

        let client = test_client(ws_url, None);
        assert!(matches!(
            client.chat_send_and_wait("kuse:xyz", "Hi", None).await,
            Err(MoltisClientError::Handshake(message)) if message.contains("missing runId")
        ));
    }
}
