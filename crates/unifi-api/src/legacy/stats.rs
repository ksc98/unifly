// Legacy API statistics endpoints
//
// Historical reports (stat/report/) and DPI statistics (stat/sitedpi).
// These endpoints return loosely-typed JSON because the field set varies
// by report type, interval, and firmware version.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::legacy::client::LegacyClient;

impl LegacyClient {
    /// Fetch site-level historical statistics.
    ///
    /// `POST /api/s/{site}/stat/report/{interval}.site`
    ///
    /// The `interval` parameter should be one of: `"5minutes"`, `"hourly"`, `"daily"`.
    /// Returns loosely-typed JSON because the field set varies by report type.
    pub async fn get_site_stats(&self, interval: &str) -> Result<Vec<serde_json::Value>, Error> {
        let path = format!("stat/report/{}.site", interval);
        let url = self.site_url(&path);
        debug!(interval, "fetching site stats");

        // The report endpoint requires a POST with attribute selection.
        // Requesting common attributes; the API ignores unknown ones.
        let body = json!({
            "attrs": ["bytes", "num_sta", "time", "wlan-num_sta", "lan-num_sta"],
        });

        self.post(url, &body).await
    }

    /// Fetch per-device historical statistics.
    ///
    /// `POST /api/s/{site}/stat/report/{interval}.device`
    ///
    /// If `macs` is provided, results are filtered to those devices.
    pub async fn get_device_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, Error> {
        let path = format!("stat/report/{}.device", interval);
        let url = self.site_url(&path);
        debug!(interval, "fetching device stats");

        let mut body = json!({
            "attrs": ["bytes", "num_sta", "time", "rx_bytes", "tx_bytes"],
        });
        if let Some(m) = macs {
            body["macs"] = json!(m);
        }

        self.post(url, &body).await
    }

    /// Fetch per-client historical statistics.
    ///
    /// `POST /api/s/{site}/stat/report/{interval}.user`
    ///
    /// If `macs` is provided, results are filtered to those clients.
    pub async fn get_client_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, Error> {
        let path = format!("stat/report/{}.user", interval);
        let url = self.site_url(&path);
        debug!(interval, "fetching client stats");

        let mut body = json!({
            "attrs": ["bytes", "time", "rx_bytes", "tx_bytes"],
        });
        if let Some(m) = macs {
            body["macs"] = json!(m);
        }

        self.post(url, &body).await
    }

    /// Fetch gateway historical statistics.
    ///
    /// `POST /api/s/{site}/stat/report/{interval}.gw`
    pub async fn get_gateway_stats(&self, interval: &str) -> Result<Vec<serde_json::Value>, Error> {
        let path = format!("stat/report/{}.gw", interval);
        let url = self.site_url(&path);
        debug!(interval, "fetching gateway stats");

        let body = json!({
            "attrs": ["bytes", "time", "wan-tx_bytes", "wan-rx_bytes", "lan-rx_bytes", "lan-tx_bytes"],
        });

        self.post(url, &body).await
    }

    /// Fetch site-wide DPI (Deep Packet Inspection) statistics.
    ///
    /// `GET /api/s/{site}/stat/sitedpi`
    ///
    /// The `group_by` parameter selects the DPI grouping: `"by-app"` or `"by-cat"`.
    pub async fn get_dpi_stats(&self, group_by: &str) -> Result<Vec<serde_json::Value>, Error> {
        let endpoint = match group_by {
            "by-cat" => "stat/sitedpi",
            _ => "stat/stadpi",
        };
        let url = self.site_url(endpoint);
        debug!(group_by, "fetching DPI stats");
        self.get(url).await
    }
}
