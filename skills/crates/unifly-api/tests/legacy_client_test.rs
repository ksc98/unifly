#![allow(clippy::unwrap_used)]
// Integration tests for `LegacyClient` using wiremock.

use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use unifly_api::{ControllerPlatform, Error, LegacyClient};

// ── Helpers ─────────────────────────────────────────────────────────

async fn setup() -> (MockServer, LegacyClient) {
    let server = MockServer::start().await;
    let base_url = Url::parse(&server.uri()).unwrap();
    let client = LegacyClient::with_client(
        reqwest::Client::new(),
        base_url,
        "default".into(),
        ControllerPlatform::ClassicController,
    );
    (server, client)
}

fn site_path(suffix: &str) -> String {
    format!("/api/s/default/{suffix}")
}

// ── Authentication tests ────────────────────────────────────────────

#[tokio::test]
async fn test_login_success() {
    let (server, client) = setup().await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let secret: secrecy::SecretString = "test-password".to_string().into();
    client.login("admin", &secret).await.unwrap();
}

#[tokio::test]
async fn test_login_failure() {
    let (server, client) = setup().await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .mount(&server)
        .await;

    let secret: secrecy::SecretString = "wrong-password".to_string().into();
    let result = client.login("admin", &secret).await;

    assert!(
        matches!(result, Err(Error::Authentication { .. })),
        "expected Authentication error, got: {result:?}"
    );
}

// ── Device tests ────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_devices() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "ok" },
        "data": [{
            "_id": "abc123",
            "mac": "aa:bb:cc:dd:ee:ff",
            "type": "usw",
            "name": "Switch-24",
            "model": "US24",
            "adopted": true,
            "state": 1
        }]
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/device")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let devices = client.list_devices().await.unwrap();

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].mac, "aa:bb:cc:dd:ee:ff");
    assert_eq!(devices[0].name.as_deref(), Some("Switch-24"));
    assert_eq!(devices[0].device_type, "usw");
    assert!(devices[0].adopted);
    assert_eq!(devices[0].state, 1);
}

// ── Event tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_events() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "ok" },
        "data": [
            {
                "_id": "evt001",
                "key": "EVT_WU_Connected",
                "msg": "User connected",
                "datetime": "2024-06-15T10:30:00Z",
                "subsystem": "wlan"
            },
            {
                "_id": "evt002",
                "key": "EVT_LU_Disconnected",
                "msg": "User disconnected",
                "datetime": "2024-06-15T10:35:00Z",
                "subsystem": "lan"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/event")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let events = client.list_events(None).await.unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].id, "evt001");
    assert_eq!(events[0].key.as_deref(), Some("EVT_WU_Connected"));
    assert_eq!(events[1].subsystem.as_deref(), Some("lan"));
}

#[tokio::test]
async fn test_list_events_with_limit() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "ok" },
        "data": [{
            "_id": "evt001",
            "key": "EVT_WU_Connected"
        }]
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/event")))
        .and(query_param("_limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let events = client.list_events(Some(5)).await.unwrap();

    assert_eq!(events.len(), 1);
}

// ── Error tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_session_expired() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let result = client.list_devices().await;

    match result {
        Err(Error::Authentication { ref message }) => {
            assert!(
                message.contains("session expired") || message.contains("insufficient permissions"),
                "expected auth error message, got: {message}"
            );
        }
        other => panic!("expected Authentication error, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_legacy_api_error() {
    let (server, client) = setup().await;

    let envelope = json!({
        "meta": { "rc": "error", "msg": "api.err.InvalidObject" },
        "data": []
    });

    Mock::given(method("GET"))
        .and(path(site_path("stat/device")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
        .mount(&server)
        .await;

    let result = client.list_devices().await;

    match result {
        Err(Error::LegacyApi { ref message }) => {
            assert!(
                message.contains("InvalidObject"),
                "expected 'InvalidObject' in message, got: {message}"
            );
        }
        other => panic!("expected LegacyApi error, got: {other:?}"),
    }
}
