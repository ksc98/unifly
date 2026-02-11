// Integration tests for `IntegrationClient` using wiremock.

use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use unifi_api::integration_types::{
    DeviceDetailsResponse, NetworkCreateUpdate, NetworkDetailsResponse, Page, SiteResponse,
};
use unifi_api::{Error, IntegrationClient};

// ── Helpers ─────────────────────────────────────────────────────────

async fn setup() -> (MockServer, IntegrationClient) {
    let server = MockServer::start().await;
    let client =
        IntegrationClient::from_reqwest(&server.uri(), reqwest::Client::new()).unwrap();
    (server, client)
}

// ── Happy-path tests ────────────────────────────────────────────────

#[tokio::test]
async fn test_list_sites_pagination() {
    let (server, client) = setup().await;

    let site_a = Uuid::new_v4();
    let site_b = Uuid::new_v4();

    let body = json!({
        "offset": 0,
        "limit": 25,
        "count": 2,
        "totalCount": 2,
        "data": [
            { "id": site_a, "name": "Main", "internalReference": "default" },
            { "id": site_b, "name": "Remote", "internalReference": "site2" },
        ]
    });

    Mock::given(method("GET"))
        .and(path("/integration/v1/sites"))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page: Page<SiteResponse> = client.list_sites(0, 25).await.unwrap();

    assert_eq!(page.total_count, 2);
    assert_eq!(page.data.len(), 2);
    assert_eq!(page.data[0].name, "Main");
    assert_eq!(page.data[0].internal_reference, "default");
    assert_eq!(page.data[1].name, "Remote");
    assert_eq!(page.data[1].id, site_b);
}

#[tokio::test]
async fn test_get_device() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let device_id = Uuid::new_v4();

    let body = json!({
        "id": device_id,
        "macAddress": "aa:bb:cc:dd:ee:ff",
        "ipAddress": "192.168.1.10",
        "name": "USW-Pro-24",
        "model": "USPPDUP",
        "state": "ONLINE",
        "supported": true,
        "firmwareVersion": "7.1.26",
        "firmwareUpdatable": false,
        "features": ["switching"],
        "interfaces": {},
        "serialNumber": "SN-1234",
        "shortName": "USW",
        "startupTimestamp": "2024-01-01T00:00:00Z"
    });

    Mock::given(method("GET"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/devices/{device_id}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let device: DeviceDetailsResponse = client.get_device(&site_id, &device_id).await.unwrap();

    assert_eq!(device.id, device_id);
    assert_eq!(device.mac_address, "aa:bb:cc:dd:ee:ff");
    assert_eq!(device.name, "USW-Pro-24");
    assert_eq!(device.model, "USPPDUP");
    assert_eq!(device.serial_number.as_deref(), Some("SN-1234"));
}

#[tokio::test]
async fn test_create_network() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let net_id = Uuid::new_v4();

    let response_body = json!({
        "id": net_id,
        "name": "IoT VLAN",
        "enabled": true,
        "management": "USER_DEFINED",
        "vlanId": 30,
        "default": false,
        "metadata": {},
        "dhcpGuarding": null
    });

    Mock::given(method("POST"))
        .and(path(format!("/integration/v1/sites/{site_id}/networks")))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .mount(&server)
        .await;

    let req = NetworkCreateUpdate {
        name: "IoT VLAN".into(),
        enabled: true,
        management: "USER_DEFINED".into(),
        vlan_id: 30,
        dhcp_guarding: None,
    };

    let resp: NetworkDetailsResponse = client.create_network(&site_id, &req).await.unwrap();

    assert_eq!(resp.id, net_id);
    assert_eq!(resp.name, "IoT VLAN");
    assert_eq!(resp.vlan_id, 30);
    assert!(!resp.default);
}

#[tokio::test]
async fn test_delete_firewall_policy() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let policy_id = Uuid::new_v4();

    Mock::given(method("DELETE"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/firewall/policies/{policy_id}"
        )))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    client
        .delete_firewall_policy(&site_id, &policy_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_pagination_empty_page() {
    let (server, client) = setup().await;

    let body = json!({
        "offset": 0,
        "limit": 25,
        "count": 0,
        "totalCount": 0,
        "data": []
    });

    Mock::given(method("GET"))
        .and(path("/integration/v1/sites"))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let page: Page<SiteResponse> = client.list_sites(0, 25).await.unwrap();

    assert_eq!(page.total_count, 0);
    assert_eq!(page.count, 0);
    assert!(page.data.is_empty());
}

// ── Error tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_error_401_unauthorized() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let result = client.list_sites(0, 25).await;

    assert!(
        matches!(result, Err(Error::InvalidApiKey)),
        "expected InvalidApiKey, got: {result:?}"
    );
}

#[tokio::test]
async fn test_error_404_not_found() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();
    let device_id = Uuid::new_v4();

    Mock::given(method("GET"))
        .and(path(format!(
            "/integration/v1/sites/{site_id}/devices/{device_id}"
        )))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({ "message": "Not found" })),
        )
        .mount(&server)
        .await;

    let result = client.get_device(&site_id, &device_id).await;

    match result {
        Err(Error::Integration {
            status,
            ref message,
            ..
        }) => {
            assert_eq!(status, 404);
            assert_eq!(message, "Not found");
        }
        other => panic!("expected Integration error, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_error_422_validation() {
    let (server, client) = setup().await;

    let site_id = Uuid::new_v4();

    Mock::given(method("POST"))
        .and(path(format!("/integration/v1/sites/{site_id}/networks")))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "message": "Invalid VLAN ID",
            "code": "VALIDATION_ERROR"
        })))
        .mount(&server)
        .await;

    let req = NetworkCreateUpdate {
        name: "Bad VLAN".into(),
        enabled: true,
        management: "USER_DEFINED".into(),
        vlan_id: 9999,
        dhcp_guarding: None,
    };

    let result = client.create_network(&site_id, &req).await;

    match result {
        Err(Error::Integration {
            status,
            ref message,
            ref code,
        }) => {
            assert_eq!(status, 422);
            assert_eq!(message, "Invalid VLAN ID");
            assert_eq!(code.as_deref(), Some("VALIDATION_ERROR"));
        }
        other => panic!("expected Integration 422 error, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_error_500_server_error() {
    let (server, client) = setup().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let result = client.list_sites(0, 25).await;

    match result {
        Err(Error::Integration {
            status, ref code, ..
        }) => {
            assert_eq!(status, 500);
            assert!(code.is_none());
        }
        other => panic!("expected Integration 500 error, got: {other:?}"),
    }
}
