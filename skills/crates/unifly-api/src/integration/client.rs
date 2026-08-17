// Hand-crafted async HTTP client for the UniFi Network Integration API (v10.1.84).
//
// Base path: /integration/v1/
// Auth: X-API-KEY header

use std::future::Future;

use reqwest::header::{HeaderMap, HeaderValue};
use secrecy::ExposeSecret;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::debug;
use url::Url;
use uuid::Uuid;

use super::types;
use crate::Error;

// ── Error response shape from the Integration API ────────────────────

#[derive(serde::Deserialize)]
struct ErrorResponse {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

// ── Client ───────────────────────────────────────────────────────────

/// Async client for the UniFi Integration API.
///
/// Uses API-key authentication and communicates via JSON REST endpoints
/// under `/integration/v1/`.
pub struct IntegrationClient {
    http: reqwest::Client,
    base_url: Url,
}

impl IntegrationClient {
    // ── Constructors ─────────────────────────────────────────────────

    /// Build from an API key, transport config, and detected platform.
    ///
    /// Injects `X-API-KEY` as a default header on every request.
    /// On UniFi OS the base path is `/proxy/network/integration/`;
    /// on standalone controllers it's just `/integration/`.
    pub fn from_api_key(
        base_url: &str,
        api_key: &secrecy::SecretString,
        transport: &crate::TransportConfig,
        platform: crate::ControllerPlatform,
    ) -> Result<Self, Error> {
        let mut headers = HeaderMap::new();
        let mut key_value =
            HeaderValue::from_str(api_key.expose_secret()).map_err(|e| Error::Authentication {
                message: format!("invalid API key header value: {e}"),
            })?;
        key_value.set_sensitive(true);
        headers.insert("X-API-KEY", key_value);

        let http = transport.build_client_with_headers(headers)?;
        let base_url = Self::normalize_base_url(base_url, platform)?;

        Ok(Self { http, base_url })
    }

    /// Wrap an existing `reqwest::Client` (caller manages auth headers).
    pub fn from_reqwest(
        base_url: &str,
        http: reqwest::Client,
        platform: crate::ControllerPlatform,
    ) -> Result<Self, Error> {
        let base_url = Self::normalize_base_url(base_url, platform)?;
        Ok(Self { http, base_url })
    }

    /// Build the base URL with correct platform prefix + `/integration/`.
    ///
    /// UniFi OS: `https://host/proxy/network/integration/`
    /// Standalone: `https://host/integration/`
    fn normalize_base_url(raw: &str, platform: crate::ControllerPlatform) -> Result<Url, Error> {
        let mut url = Url::parse(raw)?;

        // Strip trailing slash for uniform handling
        let path = url.path().trim_end_matches('/').to_owned();

        if path.ends_with("/integration") {
            url.set_path(&format!("{path}/"));
        } else {
            let prefix = platform.integration_prefix();
            url.set_path(&format!("{path}{prefix}/"));
        }

        Ok(url)
    }

    // ── URL builder ──────────────────────────────────────────────────

    /// Join a relative path (e.g. `"v1/sites"`) onto the base URL.
    fn url(&self, path: &str) -> Url {
        // base_url always ends with `/integration/`, so joining `v1/…` works.
        self.base_url
            .join(path)
            .expect("path should be valid relative URL")
    }

    // ── HTTP verbs ───────────────────────────────────────────────────

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        let url = self.url(path);
        debug!("GET {url}");

        let resp = self.http.get(url).send().await?;
        self.handle_response(resp).await
    }

    async fn get_with_params<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T, Error> {
        let url = self.url(path);
        debug!("GET {url} params={params:?}");

        let resp = self.http.get(url).query(params).send().await?;
        self.handle_response(resp).await
    }

    async fn post<T: DeserializeOwned, B: Serialize + Sync>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, Error> {
        let url = self.url(path);
        debug!("POST {url}");

        let resp = self.http.post(url).json(body).send().await?;
        self.handle_response(resp).await
    }

    async fn post_no_response<B: Serialize + Sync>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(), Error> {
        let url = self.url(path);
        debug!("POST {url}");

        let resp = self.http.post(url).json(body).send().await?;
        self.handle_empty(resp).await
    }

    async fn put<T: DeserializeOwned, B: Serialize + Sync>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, Error> {
        let url = self.url(path);
        debug!("PUT {url}");

        let resp = self.http.put(url).json(body).send().await?;
        self.handle_response(resp).await
    }

    async fn patch<T: DeserializeOwned, B: Serialize + Sync>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, Error> {
        let url = self.url(path);
        debug!("PATCH {url}");

        let resp = self.http.patch(url).json(body).send().await?;
        self.handle_response(resp).await
    }

    async fn delete(&self, path: &str) -> Result<(), Error> {
        let url = self.url(path);
        debug!("DELETE {url}");

        let resp = self.http.delete(url).send().await?;
        self.handle_empty(resp).await
    }

    async fn delete_with_response<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        let url = self.url(path);
        debug!("DELETE {url}");

        let resp = self.http.delete(url).send().await?;
        self.handle_response(resp).await
    }

    async fn delete_with_params<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T, Error> {
        let url = self.url(path);
        debug!("DELETE {url} params={params:?}");

        let resp = self.http.delete(url).query(params).send().await?;
        self.handle_response(resp).await
    }

    // ── Response handling ────────────────────────────────────────────

    async fn handle_response<T: DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> Result<T, Error> {
        let status = resp.status();
        if status.is_success() {
            let body = resp.text().await?;
            serde_json::from_str(&body).map_err(|e| {
                let preview = &body[..body.len().min(200)];
                Error::Deserialization {
                    message: format!("{e} (body preview: {preview:?})"),
                    body,
                }
            })
        } else {
            Err(self.parse_error(status, resp).await)
        }
    }

    async fn handle_empty(&self, resp: reqwest::Response) -> Result<(), Error> {
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(self.parse_error(status, resp).await)
        }
    }

    async fn parse_error(&self, status: reqwest::StatusCode, resp: reqwest::Response) -> Error {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Error::InvalidApiKey;
        }

        let raw = resp.text().await.unwrap_or_default();

        if let Ok(err) = serde_json::from_str::<ErrorResponse>(&raw) {
            Error::Integration {
                status: status.as_u16(),
                message: err.message.unwrap_or_else(|| status.to_string()),
                code: err.code,
            }
        } else {
            Error::Integration {
                status: status.as_u16(),
                message: if raw.is_empty() {
                    status.to_string()
                } else {
                    raw
                },
                code: None,
            }
        }
    }

    // ── Pagination helper ────────────────────────────────────────────

    /// Collect all pages into a single `Vec<T>`.
    pub async fn paginate_all<T, F, Fut>(&self, limit: i32, fetch: F) -> Result<Vec<T>, Error>
    where
        F: Fn(i64, i32) -> Fut,
        Fut: Future<Output = Result<types::Page<T>, Error>>,
    {
        let mut all = Vec::new();
        let mut offset: i64 = 0;

        loop {
            let page = fetch(offset, limit).await?;
            let received = page.data.len();
            all.extend(page.data);

            let limit_usize = usize::try_from(limit).unwrap_or(0);
            if received < limit_usize
                || i64::try_from(all.len()).unwrap_or(i64::MAX) >= page.total_count
            {
                break;
            }

            offset += i64::try_from(received).unwrap_or(i64::MAX);
        }

        Ok(all)
    }

    // ━━ Public API ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // ── System Info ──────────────────────────────────────────────────

    pub async fn get_info(&self) -> Result<types::ApplicationInfoResponse, Error> {
        self.get("v1/info").await
    }

    // ── Sites ────────────────────────────────────────────────────────

    pub async fn list_sites(
        &self,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::SiteResponse>, Error> {
        self.get_with_params(
            "v1/sites",
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    // ── Devices ──────────────────────────────────────────────────────

    pub async fn list_devices(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::DeviceResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/devices"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_device(
        &self,
        site_id: &Uuid,
        device_id: &Uuid,
    ) -> Result<types::DeviceDetailsResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/devices/{device_id}"))
            .await
    }

    pub async fn get_device_statistics(
        &self,
        site_id: &Uuid,
        device_id: &Uuid,
    ) -> Result<types::DeviceStatisticsResponse, Error> {
        self.get(&format!(
            "v1/sites/{site_id}/devices/{device_id}/statistics/latest"
        ))
        .await
    }

    pub async fn adopt_device(
        &self,
        site_id: &Uuid,
        mac: &str,
        ignore_device_limit: bool,
    ) -> Result<types::DeviceDetailsResponse, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body<'a> {
            mac_address: &'a str,
            ignore_device_limit: bool,
        }

        self.post(
            &format!("v1/sites/{site_id}/devices"),
            &Body {
                mac_address: mac,
                ignore_device_limit,
            },
        )
        .await
    }

    pub async fn remove_device(&self, site_id: &Uuid, device_id: &Uuid) -> Result<(), Error> {
        self.delete(&format!("v1/sites/{site_id}/devices/{device_id}"))
            .await
    }

    pub async fn device_action(
        &self,
        site_id: &Uuid,
        device_id: &Uuid,
        action: &str,
    ) -> Result<(), Error> {
        #[derive(Serialize)]
        struct Body<'a> {
            action: &'a str,
        }

        self.post_no_response(
            &format!("v1/sites/{site_id}/devices/{device_id}/actions"),
            &Body { action },
        )
        .await
    }

    pub async fn port_action(
        &self,
        site_id: &Uuid,
        device_id: &Uuid,
        port_idx: u32,
        action: &str,
    ) -> Result<(), Error> {
        #[derive(Serialize)]
        struct Body<'a> {
            action: &'a str,
        }

        self.post_no_response(
            &format!("v1/sites/{site_id}/devices/{device_id}/interfaces/ports/{port_idx}/actions"),
            &Body { action },
        )
        .await
    }

    pub async fn list_pending_devices(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::PendingDeviceResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/devices/pending"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn list_device_tags(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::DeviceTagResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/devices/tags"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    // ── Clients ──────────────────────────────────────────────────────

    pub async fn list_clients(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::ClientResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/clients"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_client(
        &self,
        site_id: &Uuid,
        client_id: &Uuid,
    ) -> Result<types::ClientDetailsResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/clients/{client_id}"))
            .await
    }

    pub async fn client_action(
        &self,
        site_id: &Uuid,
        client_id: &Uuid,
        action: &str,
    ) -> Result<types::ClientActionResponse, Error> {
        #[derive(Serialize)]
        struct Body<'a> {
            action: &'a str,
        }

        self.post(
            &format!("v1/sites/{site_id}/clients/{client_id}/actions"),
            &Body { action },
        )
        .await
    }

    // ── Networks ─────────────────────────────────────────────────────

    pub async fn list_networks(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::NetworkResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/networks"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_network(
        &self,
        site_id: &Uuid,
        network_id: &Uuid,
    ) -> Result<types::NetworkDetailsResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/networks/{network_id}"))
            .await
    }

    pub async fn create_network(
        &self,
        site_id: &Uuid,
        body: &types::NetworkCreateUpdate,
    ) -> Result<types::NetworkDetailsResponse, Error> {
        self.post(&format!("v1/sites/{site_id}/networks"), body)
            .await
    }

    pub async fn update_network(
        &self,
        site_id: &Uuid,
        network_id: &Uuid,
        body: &types::NetworkCreateUpdate,
    ) -> Result<types::NetworkDetailsResponse, Error> {
        self.put(&format!("v1/sites/{site_id}/networks/{network_id}"), body)
            .await
    }

    pub async fn delete_network(&self, site_id: &Uuid, network_id: &Uuid) -> Result<(), Error> {
        self.delete(&format!("v1/sites/{site_id}/networks/{network_id}"))
            .await
    }

    pub async fn get_network_references(
        &self,
        site_id: &Uuid,
        network_id: &Uuid,
    ) -> Result<types::NetworkReferencesResponse, Error> {
        self.get(&format!(
            "v1/sites/{site_id}/networks/{network_id}/references"
        ))
        .await
    }

    // ── WiFi Broadcasts ──────────────────────────────────────────────

    pub async fn list_wifi_broadcasts(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::WifiBroadcastResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/wifi/broadcasts"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_wifi_broadcast(
        &self,
        site_id: &Uuid,
        broadcast_id: &Uuid,
    ) -> Result<types::WifiBroadcastDetailsResponse, Error> {
        self.get(&format!(
            "v1/sites/{site_id}/wifi/broadcasts/{broadcast_id}"
        ))
        .await
    }

    pub async fn create_wifi_broadcast(
        &self,
        site_id: &Uuid,
        body: &types::WifiBroadcastCreateUpdate,
    ) -> Result<types::WifiBroadcastDetailsResponse, Error> {
        self.post(&format!("v1/sites/{site_id}/wifi/broadcasts"), body)
            .await
    }

    pub async fn update_wifi_broadcast(
        &self,
        site_id: &Uuid,
        broadcast_id: &Uuid,
        body: &types::WifiBroadcastCreateUpdate,
    ) -> Result<types::WifiBroadcastDetailsResponse, Error> {
        self.put(
            &format!("v1/sites/{site_id}/wifi/broadcasts/{broadcast_id}"),
            body,
        )
        .await
    }

    pub async fn delete_wifi_broadcast(
        &self,
        site_id: &Uuid,
        broadcast_id: &Uuid,
    ) -> Result<(), Error> {
        self.delete(&format!(
            "v1/sites/{site_id}/wifi/broadcasts/{broadcast_id}"
        ))
        .await
    }

    // ── Firewall Policies ────────────────────────────────────────────

    pub async fn list_firewall_policies(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::FirewallPolicyResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/firewall/policies"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_firewall_policy(
        &self,
        site_id: &Uuid,
        policy_id: &Uuid,
    ) -> Result<types::FirewallPolicyResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/firewall/policies/{policy_id}"))
            .await
    }

    pub async fn create_firewall_policy(
        &self,
        site_id: &Uuid,
        body: &types::FirewallPolicyCreateUpdate,
    ) -> Result<types::FirewallPolicyResponse, Error> {
        self.post(&format!("v1/sites/{site_id}/firewall/policies"), body)
            .await
    }

    pub async fn update_firewall_policy(
        &self,
        site_id: &Uuid,
        policy_id: &Uuid,
        body: &types::FirewallPolicyCreateUpdate,
    ) -> Result<types::FirewallPolicyResponse, Error> {
        self.put(
            &format!("v1/sites/{site_id}/firewall/policies/{policy_id}"),
            body,
        )
        .await
    }

    pub async fn patch_firewall_policy(
        &self,
        site_id: &Uuid,
        policy_id: &Uuid,
        body: &types::FirewallPolicyPatch,
    ) -> Result<types::FirewallPolicyResponse, Error> {
        self.patch(
            &format!("v1/sites/{site_id}/firewall/policies/{policy_id}"),
            body,
        )
        .await
    }

    pub async fn delete_firewall_policy(
        &self,
        site_id: &Uuid,
        policy_id: &Uuid,
    ) -> Result<(), Error> {
        self.delete(&format!("v1/sites/{site_id}/firewall/policies/{policy_id}"))
            .await
    }

    pub async fn get_firewall_policy_ordering(
        &self,
        site_id: &Uuid,
    ) -> Result<types::FirewallPolicyOrdering, Error> {
        self.get(&format!("v1/sites/{site_id}/firewall/policies/ordering"))
            .await
    }

    pub async fn set_firewall_policy_ordering(
        &self,
        site_id: &Uuid,
        body: &types::FirewallPolicyOrdering,
    ) -> Result<types::FirewallPolicyOrdering, Error> {
        self.put(
            &format!("v1/sites/{site_id}/firewall/policies/ordering"),
            body,
        )
        .await
    }

    // ── Firewall Zones ───────────────────────────────────────────────

    pub async fn list_firewall_zones(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::FirewallZoneResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/firewall/zones"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_firewall_zone(
        &self,
        site_id: &Uuid,
        zone_id: &Uuid,
    ) -> Result<types::FirewallZoneResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/firewall/zones/{zone_id}"))
            .await
    }

    pub async fn create_firewall_zone(
        &self,
        site_id: &Uuid,
        body: &types::FirewallZoneCreateUpdate,
    ) -> Result<types::FirewallZoneResponse, Error> {
        self.post(&format!("v1/sites/{site_id}/firewall/zones"), body)
            .await
    }

    pub async fn update_firewall_zone(
        &self,
        site_id: &Uuid,
        zone_id: &Uuid,
        body: &types::FirewallZoneCreateUpdate,
    ) -> Result<types::FirewallZoneResponse, Error> {
        self.put(
            &format!("v1/sites/{site_id}/firewall/zones/{zone_id}"),
            body,
        )
        .await
    }

    pub async fn delete_firewall_zone(&self, site_id: &Uuid, zone_id: &Uuid) -> Result<(), Error> {
        self.delete(&format!("v1/sites/{site_id}/firewall/zones/{zone_id}"))
            .await
    }

    // ── ACL Rules ────────────────────────────────────────────────────

    pub async fn list_acl_rules(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::AclRuleResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/acl-rules"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_acl_rule(
        &self,
        site_id: &Uuid,
        rule_id: &Uuid,
    ) -> Result<types::AclRuleResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/acl-rules/{rule_id}"))
            .await
    }

    pub async fn create_acl_rule(
        &self,
        site_id: &Uuid,
        body: &types::AclRuleCreateUpdate,
    ) -> Result<types::AclRuleResponse, Error> {
        self.post(&format!("v1/sites/{site_id}/acl-rules"), body)
            .await
    }

    pub async fn update_acl_rule(
        &self,
        site_id: &Uuid,
        rule_id: &Uuid,
        body: &types::AclRuleCreateUpdate,
    ) -> Result<types::AclRuleResponse, Error> {
        self.put(&format!("v1/sites/{site_id}/acl-rules/{rule_id}"), body)
            .await
    }

    pub async fn delete_acl_rule(&self, site_id: &Uuid, rule_id: &Uuid) -> Result<(), Error> {
        self.delete(&format!("v1/sites/{site_id}/acl-rules/{rule_id}"))
            .await
    }

    pub async fn get_acl_rule_ordering(
        &self,
        site_id: &Uuid,
    ) -> Result<types::AclRuleOrdering, Error> {
        self.get(&format!("v1/sites/{site_id}/acl-rules/ordering"))
            .await
    }

    pub async fn set_acl_rule_ordering(
        &self,
        site_id: &Uuid,
        body: &types::AclRuleOrdering,
    ) -> Result<types::AclRuleOrdering, Error> {
        self.put(&format!("v1/sites/{site_id}/acl-rules/ordering"), body)
            .await
    }

    // ── DNS Policies ─────────────────────────────────────────────────

    pub async fn list_dns_policies(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::DnsPolicyResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/dns/policies"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_dns_policy(
        &self,
        site_id: &Uuid,
        dns_id: &Uuid,
    ) -> Result<types::DnsPolicyResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/dns/policies/{dns_id}"))
            .await
    }

    pub async fn create_dns_policy(
        &self,
        site_id: &Uuid,
        body: &types::DnsPolicyCreateUpdate,
    ) -> Result<types::DnsPolicyResponse, Error> {
        self.post(&format!("v1/sites/{site_id}/dns/policies"), body)
            .await
    }

    pub async fn update_dns_policy(
        &self,
        site_id: &Uuid,
        dns_id: &Uuid,
        body: &types::DnsPolicyCreateUpdate,
    ) -> Result<types::DnsPolicyResponse, Error> {
        self.put(&format!("v1/sites/{site_id}/dns/policies/{dns_id}"), body)
            .await
    }

    pub async fn delete_dns_policy(&self, site_id: &Uuid, dns_id: &Uuid) -> Result<(), Error> {
        self.delete(&format!("v1/sites/{site_id}/dns/policies/{dns_id}"))
            .await
    }

    // ── Traffic Matching Lists ───────────────────────────────────────

    pub async fn list_traffic_matching_lists(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::TrafficMatchingListResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/traffic-matching-lists"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_traffic_matching_list(
        &self,
        site_id: &Uuid,
        list_id: &Uuid,
    ) -> Result<types::TrafficMatchingListResponse, Error> {
        self.get(&format!(
            "v1/sites/{site_id}/traffic-matching-lists/{list_id}"
        ))
        .await
    }

    pub async fn create_traffic_matching_list(
        &self,
        site_id: &Uuid,
        body: &types::TrafficMatchingListCreateUpdate,
    ) -> Result<types::TrafficMatchingListResponse, Error> {
        self.post(&format!("v1/sites/{site_id}/traffic-matching-lists"), body)
            .await
    }

    pub async fn update_traffic_matching_list(
        &self,
        site_id: &Uuid,
        list_id: &Uuid,
        body: &types::TrafficMatchingListCreateUpdate,
    ) -> Result<types::TrafficMatchingListResponse, Error> {
        self.put(
            &format!("v1/sites/{site_id}/traffic-matching-lists/{list_id}"),
            body,
        )
        .await
    }

    pub async fn delete_traffic_matching_list(
        &self,
        site_id: &Uuid,
        list_id: &Uuid,
    ) -> Result<(), Error> {
        self.delete(&format!(
            "v1/sites/{site_id}/traffic-matching-lists/{list_id}"
        ))
        .await
    }

    // ── Hotspot Vouchers ─────────────────────────────────────────────

    pub async fn list_vouchers(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::VoucherResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/hotspot/vouchers"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_voucher(
        &self,
        site_id: &Uuid,
        voucher_id: &Uuid,
    ) -> Result<types::VoucherResponse, Error> {
        self.get(&format!("v1/sites/{site_id}/hotspot/vouchers/{voucher_id}"))
            .await
    }

    pub async fn create_vouchers(
        &self,
        site_id: &Uuid,
        body: &types::VoucherCreateRequest,
    ) -> Result<Vec<types::VoucherResponse>, Error> {
        self.post(&format!("v1/sites/{site_id}/hotspot/vouchers"), body)
            .await
    }

    pub async fn delete_voucher(
        &self,
        site_id: &Uuid,
        voucher_id: &Uuid,
    ) -> Result<types::VoucherDeletionResults, Error> {
        self.delete_with_response(&format!("v1/sites/{site_id}/hotspot/vouchers/{voucher_id}"))
            .await
    }

    pub async fn purge_vouchers(
        &self,
        site_id: &Uuid,
        filter: &str,
    ) -> Result<types::VoucherDeletionResults, Error> {
        self.delete_with_params(
            &format!("v1/sites/{site_id}/hotspot/vouchers"),
            &[("filter", filter.to_owned())],
        )
        .await
    }

    // ── VPN (read-only) ──────────────────────────────────────────────

    pub async fn list_vpn_servers(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::VpnServerResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/vpn/servers"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn list_vpn_tunnels(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::VpnTunnelResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/vpn/tunnels"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    // ── WAN (read-only) ──────────────────────────────────────────────

    pub async fn list_wans(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::WanResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/wans"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    // ── DPI (read-only) ──────────────────────────────────────────────

    pub async fn list_dpi_categories(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::DpiCategoryResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/dpi/categories"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn list_dpi_applications(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::DpiApplicationResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/dpi/applications"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    // ── RADIUS (read-only) ───────────────────────────────────────────

    pub async fn list_radius_profiles(
        &self,
        site_id: &Uuid,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::RadiusProfileResponse>, Error> {
        self.get_with_params(
            &format!("v1/sites/{site_id}/radius/profiles"),
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    // ── Countries (no site scope) ────────────────────────────────────

    pub async fn list_countries(
        &self,
        offset: i64,
        limit: i32,
    ) -> Result<types::Page<types::CountryResponse>, Error> {
        self.get_with_params(
            "v1/countries",
            &[("offset", offset.to_string()), ("limit", limit.to_string())],
        )
        .await
    }
}
