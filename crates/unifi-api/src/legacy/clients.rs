// Legacy API client (station) endpoints
//
// Client management via stat/sta (read) and cmd/stamgr (commands).
// Covers listing, blocking, kicking, forgetting, and guest authorization.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::legacy::client::LegacyClient;
use crate::legacy::models::LegacyClientEntry;

impl LegacyClient {
    /// List all currently connected clients (stations).
    ///
    /// `GET /api/s/{site}/stat/sta`
    pub async fn list_clients(&self) -> Result<Vec<LegacyClientEntry>, Error> {
        let url = self.site_url("stat/sta");
        debug!("listing connected clients");
        self.get(url).await
    }

    /// Block a client by MAC address.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with `{"cmd": "block-sta", "mac": "..."}`
    pub async fn block_client(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, "blocking client");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "block-sta",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Unblock a client by MAC address.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with `{"cmd": "unblock-sta", "mac": "..."}`
    pub async fn unblock_client(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, "unblocking client");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "unblock-sta",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Disconnect (kick) a client.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with `{"cmd": "kick-sta", "mac": "..."}`
    pub async fn kick_client(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, "kicking client");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "kick-sta",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Forget (permanently remove) a client by MAC address.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with `{"cmd": "forget-sta", "macs": [...]}`
    pub async fn forget_client(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, "forgetting client");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "forget-sta",
                    "macs": [mac],
                }),
            )
            .await?;
        Ok(())
    }

    /// Authorize a guest client on the hotspot portal.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with guest authorization parameters.
    ///
    /// - `mac`: Client MAC address
    /// - `minutes`: Authorization duration in minutes
    /// - `up_kbps`: Optional upload bandwidth limit (Kbps)
    /// - `down_kbps`: Optional download bandwidth limit (Kbps)
    /// - `quota_mb`: Optional data transfer quota (MB)
    pub async fn authorize_guest(
        &self,
        mac: &str,
        minutes: u32,
        up_kbps: Option<u32>,
        down_kbps: Option<u32>,
        quota_mb: Option<u32>,
    ) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, minutes, "authorizing guest");

        let mut body = json!({
            "cmd": "authorize-guest",
            "mac": mac,
            "minutes": minutes,
        });

        let obj = body.as_object_mut().expect("json! macro always produces an object");
        if let Some(up) = up_kbps {
            obj.insert("up".into(), json!(up));
        }
        if let Some(down) = down_kbps {
            obj.insert("down".into(), json!(down));
        }
        if let Some(quota) = quota_mb {
            obj.insert("bytes".into(), json!(quota));
        }

        let _: Vec<serde_json::Value> = self.post(url, &body).await?;
        Ok(())
    }
}
