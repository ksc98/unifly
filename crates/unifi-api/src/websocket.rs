//! WebSocket event stream with auto-reconnect.
//!
//! Connects to a UniFi controller's legacy WebSocket endpoint and streams
//! parsed events through a [`tokio::sync::broadcast`] channel. Handles
//! reconnection with exponential backoff + jitter automatically.
//!
//! # Example
//!
//! ```rust,ignore
//! use unifi_api::websocket::{WebSocketHandle, ReconnectConfig};
//! use tokio_util::sync::CancellationToken;
//! use url::Url;
//!
//! let cancel = CancellationToken::new();
//! let ws_url = Url::parse("wss://192.168.1.1/proxy/network/wss/s/default/events")?;
//!
//! let handle = WebSocketHandle::connect(ws_url, ReconnectConfig::default(), cancel.clone(), None).await?;
//! let mut rx = handle.subscribe();
//!
//! while let Ok(event) = rx.recv().await {
//!     println!("{}: {}", event.key, event.message.as_deref().unwrap_or(""));
//! }
//!
//! handle.shutdown();
//! ```

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::{self, ClientRequestBuilder};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::error::Error;

// ── Broadcast channel capacity ───────────────────────────────────────

const EVENT_CHANNEL_CAPACITY: usize = 1024;

// ── UnifiEvent ───────────────────────────────────────────────────────

/// A parsed event from the UniFi WebSocket stream.
///
/// Uses `#[serde(flatten)]` to capture all fields beyond the core set,
/// so nothing from the controller is silently dropped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiEvent {
    /// Event key, e.g. `"EVT_WU_Connected"`, `"EVT_SW_Disconnected"`.
    pub key: String,

    /// Subsystem that emitted the event: `"wlan"`, `"lan"`, `"sta"`, `"gw"`, etc.
    pub subsystem: String,

    /// Site ID this event belongs to.
    pub site_id: String,

    /// Human-readable event message, if present.
    #[serde(default)]
    pub message: Option<String>,

    /// ISO-8601 timestamp from the controller.
    #[serde(default)]
    pub datetime: Option<String>,

    /// All remaining fields the controller sends.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

// ── ReconnectConfig ──────────────────────────────────────────────────

/// Exponential backoff configuration for WebSocket reconnection.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Delay before the first reconnection attempt. Default: 1s.
    pub initial_delay: Duration,

    /// Upper bound on backoff delay. Default: 30s.
    pub max_delay: Duration,

    /// Maximum reconnection attempts before giving up.
    /// `None` means retry forever.
    pub max_retries: Option<u32>,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            max_retries: None,
        }
    }
}

// ── WebSocketHandle ──────────────────────────────────────────────────

/// Handle to a running WebSocket event stream.
///
/// Cheaply cloneable via the inner broadcast sender. Drop all handles
/// and call [`shutdown`](Self::shutdown) to tear down the background task.
pub struct WebSocketHandle {
    event_rx: broadcast::Receiver<Arc<UnifiEvent>>,
    cancel: CancellationToken,
}

impl WebSocketHandle {
    /// Connect to the controller WebSocket and spawn the reconnection loop.
    ///
    /// Returns immediately once the background task is spawned.
    /// The first connection attempt happens asynchronously -- subscribe to
    /// the event receiver to start consuming events.
    pub async fn connect(
        ws_url: Url,
        reconnect: ReconnectConfig,
        cancel: CancellationToken,
        cookie: Option<String>,
    ) -> Result<Self, Error> {
        let (event_tx, event_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let task_cancel = cancel.clone();
        tokio::spawn(async move {
            ws_loop(ws_url, event_tx, reconnect, task_cancel, cookie).await;
        });

        Ok(Self { event_rx, cancel })
    }

    /// Get a new broadcast receiver for the event stream.
    ///
    /// Multiple consumers can subscribe concurrently. If a consumer falls
    /// behind, it receives [`broadcast::error::RecvError::Lagged`].
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<UnifiEvent>> {
        self.event_rx.resubscribe()
    }

    /// Signal the background task to shut down gracefully.
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

// ── Background reconnection loop ─────────────────────────────────────

/// Main loop: connect → read → on error, backoff → reconnect.
async fn ws_loop(
    ws_url: Url,
    event_tx: broadcast::Sender<Arc<UnifiEvent>>,
    reconnect: ReconnectConfig,
    cancel: CancellationToken,
    cookie: Option<String>,
) {
    let mut attempt: u32 = 0;

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            result = connect_and_read(&ws_url, &event_tx, &cancel, cookie.as_deref()) => {
                match result {
                    // Clean disconnect (server close frame or stream ended).
                    // Reset attempt counter and reconnect immediately.
                    Ok(()) => {
                        tracing::info!("WebSocket disconnected cleanly, reconnecting");
                        attempt = 0;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, attempt, "WebSocket error");

                        if let Some(max) = reconnect.max_retries {
                            if attempt >= max {
                                tracing::error!(
                                    max_retries = max,
                                    "WebSocket reconnection limit reached, giving up"
                                );
                                break;
                            }
                        }

                        let delay = calculate_backoff(attempt, &reconnect);
                        tracing::info!(
                            delay_ms = delay.as_millis() as u64,
                            attempt,
                            "Waiting before reconnect"
                        );

                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => break,
                            _ = tokio::time::sleep(delay) => {}
                        }

                        attempt += 1;
                    }
                }
            }
        }
    }

    // Note: tracing after the loop is technically reachable (via break)
    // but the compiler's macro expansion for select! can't prove it.
    #[allow(unreachable_code)]
    { tracing::debug!("WebSocket loop exiting"); }
}

// ── Single connection lifecycle ──────────────────────────────────────

/// Establish a single WebSocket connection, read messages until it drops.
///
/// If `cookie` is provided, it's injected as a `Cookie` header on the
/// WebSocket upgrade request (required for legacy cookie-based auth).
async fn connect_and_read(
    url: &Url,
    event_tx: &broadcast::Sender<Arc<UnifiEvent>>,
    cancel: &CancellationToken,
    cookie: Option<&str>,
) -> Result<(), Error> {
    tracing::info!(url = %url, "Connecting to WebSocket");

    let uri: tungstenite::http::Uri = url
        .as_str()
        .parse()
        .map_err(|e: tungstenite::http::uri::InvalidUri| Error::WebSocketConnect(e.to_string()))?;

    let mut request = ClientRequestBuilder::new(uri);
    if let Some(cookie_val) = cookie {
        request = request.with_header("Cookie", cookie_val);
    }

    let (ws_stream, _response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| Error::WebSocketConnect(e.to_string()))?;

    tracing::info!("WebSocket connected");

    let (_write, mut read) = ws_stream.split();

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok(()),
            frame = read.next() => {
                match frame {
                    Some(Ok(tungstenite::Message::Text(text))) => {
                        parse_and_broadcast(&text, event_tx);
                    }
                    Some(Ok(tungstenite::Message::Ping(_))) => {
                        // tungstenite handles pong replies automatically
                        tracing::trace!("WebSocket ping");
                    }
                    Some(Ok(tungstenite::Message::Close(frame))) => {
                        if let Some(ref cf) = frame {
                            tracing::info!(
                                code = %cf.code,
                                reason = %cf.reason,
                                "WebSocket close frame received"
                            );
                        } else {
                            tracing::info!("WebSocket close frame received (no payload)");
                        }
                        return Ok(());
                    }
                    Some(Err(e)) => {
                        return Err(Error::WebSocketConnect(e.to_string()));
                    }
                    None => {
                        // Stream ended without a close frame
                        tracing::info!("WebSocket stream ended");
                        return Ok(());
                    }
                    _ => {
                        // Binary, Pong, Frame -- ignore
                    }
                }
            }
        }
    }
}

// ── Message parsing ──────────────────────────────────────────────────

/// Raw envelope the controller sends over the WebSocket.
///
/// All messages have the shape `{ "meta": { "rc": "ok", ... }, "data": [...] }`.
#[derive(Debug, Deserialize)]
struct WsEnvelope {
    #[allow(dead_code)]
    meta: WsMeta,
    data: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct WsMeta {
    #[allow(dead_code)]
    rc: String,
    #[serde(default)]
    message: Option<String>,
}

/// Parse a WebSocket text frame and broadcast any events found inside.
fn parse_and_broadcast(
    text: &str,
    event_tx: &broadcast::Sender<Arc<UnifiEvent>>,
) {
    let envelope: WsEnvelope = match serde_json::from_str(text) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(error = %e, "Failed to parse WebSocket envelope");
            return;
        }
    };

    let msg_type = envelope.meta.message.as_deref().unwrap_or("");

    // Only "events" messages contain discrete events with a `key` field.
    // Sync messages ("device:sync", "sta:sync", etc.) are state dumps --
    // we surface them as events too, using the message type as the key.
    for data in envelope.data {
        let event = match msg_type {
            "events" => match serde_json::from_value::<UnifiEvent>(data.clone()) {
                Ok(evt) => evt,
                Err(e) => {
                    tracing::debug!(
                        error = %e,
                        msg_type,
                        "Could not deserialize event, constructing from raw data"
                    );
                    event_from_raw(msg_type, &data)
                }
            },
            // Sync and other message types -- construct a synthetic event
            _ => event_from_raw(msg_type, &data),
        };

        // Ignore send errors -- just means no active subscribers right now
        let _ = event_tx.send(Arc::new(event));
    }
}

/// Build a [`UnifiEvent`] from raw JSON when typed deserialization fails
/// or the message is a sync/unknown type.
fn event_from_raw(msg_type: &str, data: &serde_json::Value) -> UnifiEvent {
    UnifiEvent {
        key: data["key"]
            .as_str()
            .unwrap_or(msg_type)
            .to_string(),
        subsystem: data["subsystem"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        site_id: data["site_id"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        message: data["msg"]
            .as_str()
            .or_else(|| data["message"].as_str())
            .map(String::from),
        datetime: data["datetime"]
            .as_str()
            .map(String::from),
        extra: data.clone(),
    }
}

// ── Backoff calculation ──────────────────────────────────────────────

/// Exponential backoff with jitter.
///
/// `delay = min(initial * 2^attempt, max) + jitter`
///
/// Jitter is +-25% to spread out reconnection storms from multiple clients.
fn calculate_backoff(attempt: u32, config: &ReconnectConfig) -> Duration {
    let base = config
        .initial_delay
        .as_secs_f64()
        * 2.0_f64.powi(attempt as i32);
    let capped = base.min(config.max_delay.as_secs_f64());

    // Deterministic "jitter" seeded from the attempt number.
    // Not cryptographically random, but good enough for backoff spread.
    let jitter_factor = 1.0 + 0.25 * ((attempt as f64 * 7.3).sin());
    let with_jitter = (capped * jitter_factor).max(0.0);

    Duration::from_secs_f64(with_jitter)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_reconnect_config() {
        let config = ReconnectConfig::default();
        assert_eq!(config.initial_delay, Duration::from_secs(1));
        assert_eq!(config.max_delay, Duration::from_secs(30));
        assert!(config.max_retries.is_none());
    }

    #[test]
    fn backoff_increases_exponentially() {
        let config = ReconnectConfig::default();

        let d0 = calculate_backoff(0, &config);
        let d1 = calculate_backoff(1, &config);
        let d2 = calculate_backoff(2, &config);

        // Each step should roughly double (within jitter bounds)
        assert!(d1 > d0, "d1 ({d1:?}) should be greater than d0 ({d0:?})");
        assert!(d2 > d1, "d2 ({d2:?}) should be greater than d1 ({d1:?})");
    }

    #[test]
    fn backoff_caps_at_max_delay() {
        let config = ReconnectConfig {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(10),
            max_retries: None,
        };

        let d10 = calculate_backoff(10, &config);
        // With jitter factor up to 1.25, max effective is 12.5s
        assert!(
            d10 <= Duration::from_secs(13),
            "delay at attempt 10 ({d10:?}) should be capped near max_delay"
        );
    }

    #[test]
    fn parse_event_from_raw_json() {
        let data = serde_json::json!({
            "key": "EVT_WU_Connected",
            "subsystem": "wlan",
            "site_id": "abc123",
            "msg": "User[aa:bb:cc:dd:ee:ff] connected",
            "datetime": "2026-02-10T12:00:00Z",
            "user": "aa:bb:cc:dd:ee:ff",
            "ssid": "MyNetwork"
        });

        let event = event_from_raw("events", &data);
        assert_eq!(event.key, "EVT_WU_Connected");
        assert_eq!(event.subsystem, "wlan");
        assert_eq!(event.site_id, "abc123");
        assert_eq!(event.message.as_deref(), Some("User[aa:bb:cc:dd:ee:ff] connected"));
        assert_eq!(event.datetime.as_deref(), Some("2026-02-10T12:00:00Z"));
    }

    #[test]
    fn parse_sync_event_from_raw_json() {
        let data = serde_json::json!({
            "mac": "aa:bb:cc:dd:ee:ff",
            "state": 1,
            "site_id": "site1"
        });

        let event = event_from_raw("device:sync", &data);
        assert_eq!(event.key, "device:sync");
        assert_eq!(event.subsystem, "unknown");
        assert_eq!(event.site_id, "site1");
    }

    #[test]
    fn deserialize_unifi_event() {
        let json = r#"{
            "key": "EVT_SW_Disconnected",
            "subsystem": "lan",
            "site_id": "default",
            "message": "Switch lost contact",
            "datetime": "2026-02-10T13:00:00Z",
            "sw": "aa:bb:cc:dd:ee:ff",
            "port": 4
        }"#;

        let event: UnifiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.key, "EVT_SW_Disconnected");
        assert_eq!(event.subsystem, "lan");
        assert_eq!(event.site_id, "default");
        assert_eq!(event.message.as_deref(), Some("Switch lost contact"));
        // Extra fields should be captured in `extra`
        assert_eq!(event.extra["sw"], "aa:bb:cc:dd:ee:ff");
        assert_eq!(event.extra["port"], 4);
    }

    #[test]
    fn parse_and_broadcast_events_message() {
        let (tx, mut rx) = broadcast::channel(16);

        let raw = serde_json::json!({
            "meta": { "rc": "ok", "message": "events" },
            "data": [{
                "key": "EVT_WU_Connected",
                "subsystem": "wlan",
                "site_id": "default",
                "msg": "Client connected",
                "user": "aa:bb:cc:dd:ee:ff"
            }]
        });

        parse_and_broadcast(&raw.to_string(), &tx);

        let event = rx.try_recv().unwrap();
        assert_eq!(event.key, "EVT_WU_Connected");
        assert_eq!(event.subsystem, "wlan");
    }

    #[test]
    fn parse_and_broadcast_sync_message() {
        let (tx, mut rx) = broadcast::channel(16);

        let raw = serde_json::json!({
            "meta": { "rc": "ok", "message": "device:sync" },
            "data": [{
                "mac": "aa:bb:cc:dd:ee:ff",
                "state": 1,
                "site_id": "site1"
            }]
        });

        parse_and_broadcast(&raw.to_string(), &tx);

        let event = rx.try_recv().unwrap();
        assert_eq!(event.key, "device:sync");
        assert_eq!(event.site_id, "site1");
    }

    #[test]
    fn parse_and_broadcast_malformed_json() {
        let (tx, mut rx) = broadcast::channel::<Arc<UnifiEvent>>(16);

        parse_and_broadcast("not json at all", &tx);

        // Should not panic, should just log and skip
        assert!(rx.try_recv().is_err());
    }
}
