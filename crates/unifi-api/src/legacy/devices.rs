// Legacy API device endpoints
//
// Device management via stat/device (read) and cmd/devmgr (commands).
// Covers listing, adoption, restart, firmware upgrade, and LED locate.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::legacy::client::LegacyClient;
use crate::legacy::models::LegacyDevice;

impl LegacyClient {
    /// List all devices with full statistics.
    ///
    /// `GET /api/s/{site}/stat/device`
    pub async fn list_devices(&self) -> Result<Vec<LegacyDevice>, Error> {
        let url = self.site_url("stat/device");
        debug!("listing devices");
        self.get(url).await
    }

    /// Get a single device by MAC address.
    ///
    /// Filters the device list by MAC. Returns `None` if no device matches.
    pub async fn get_device(&self, mac: &str) -> Result<Option<LegacyDevice>, Error> {
        let url = self.site_url("stat/device");
        let body = json!({ "macs": [mac.to_lowercase()] });
        let devices: Vec<LegacyDevice> = self.post(url, &body).await?;
        Ok(devices.into_iter().next())
    }

    /// Adopt a pending device.
    ///
    /// `POST /api/s/{site}/cmd/devmgr` with `{"cmd": "adopt", "mac": "..."}`
    pub async fn adopt_device(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/devmgr");
        debug!(mac, "adopting device");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "adopt",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Restart a device.
    ///
    /// `POST /api/s/{site}/cmd/devmgr` with `{"cmd": "restart", "mac": "..."}`
    pub async fn restart_device(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/devmgr");
        debug!(mac, "restarting device");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "restart",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Upgrade device firmware.
    ///
    /// If `url` is `Some`, upgrades from that URL (`cmd: "upgrade-external"`).
    /// Otherwise upgrades from Ubiquiti's cloud (`cmd: "upgrade"`).
    pub async fn upgrade_device(&self, mac: &str, firmware_url: Option<&str>) -> Result<(), Error> {
        let api_url = self.site_url("cmd/devmgr");
        debug!(mac, ?firmware_url, "upgrading device firmware");

        let body = match firmware_url {
            Some(fw_url) => json!({
                "cmd": "upgrade-external",
                "mac": mac,
                "url": fw_url,
            }),
            None => json!({
                "cmd": "upgrade",
                "mac": mac,
            }),
        };

        let _: Vec<serde_json::Value> = self.post(api_url, &body).await?;
        Ok(())
    }

    /// Toggle the LED locator on a device.
    ///
    /// `enable: true` sends `set-locate`, `false` sends `unset-locate`.
    pub async fn locate_device(&self, mac: &str, enable: bool) -> Result<(), Error> {
        let url = self.site_url("cmd/devmgr");
        let cmd = if enable { "set-locate" } else { "unset-locate" };
        debug!(mac, cmd, "toggling device locate LED");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": cmd,
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }
}
