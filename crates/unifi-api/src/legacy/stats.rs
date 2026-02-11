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
    pub async fn get_site_stats(
        &self,
        interval: &str,
    ) -> Result<Vec<serde_json::Value>, Error> {
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

    /// Fetch site-wide DPI (Deep Packet Inspection) statistics.
    ///
    /// `GET /api/s/{site}/stat/sitedpi`
    pub async fn get_dpi_stats(&self) -> Result<Vec<serde_json::Value>, Error> {
        let url = self.site_url("stat/sitedpi");
        debug!("fetching DPI stats");
        self.get(url).await
    }
}
