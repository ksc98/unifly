// Legacy API system endpoints
//
// Controller-level operations: sysinfo, health dashboard, backup management.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::legacy::client::LegacyClient;

impl LegacyClient {
    /// Get controller system information.
    ///
    /// `GET /api/s/{site}/stat/sysinfo`
    ///
    /// Returns loosely-typed JSON because the field set varies by
    /// platform and firmware version.
    pub async fn get_sysinfo(&self) -> Result<serde_json::Value, Error> {
        let url = self.site_url("stat/sysinfo");
        debug!("fetching sysinfo");
        let mut data: Vec<serde_json::Value> = self.get(url).await?;
        // sysinfo typically returns a single-element array
        Ok(data.pop().unwrap_or(serde_json::Value::Null))
    }

    /// Get site health dashboard metrics.
    ///
    /// `GET /api/s/{site}/stat/health`
    ///
    /// Returns subsystem health entries (wan, lan, wlan, vpn, etc.).
    pub async fn get_health(&self) -> Result<Vec<serde_json::Value>, Error> {
        let url = self.site_url("stat/health");
        debug!("fetching site health");
        self.get(url).await
    }

    /// List available controller backups.
    ///
    /// `POST /api/s/{site}/cmd/backup` with `{"cmd": "list-backups"}`
    pub async fn list_backups(&self) -> Result<Vec<serde_json::Value>, Error> {
        let url = self.site_url("cmd/backup");
        debug!("listing backups");
        self.post(url, &json!({ "cmd": "list-backups" })).await
    }

    /// Create a new controller backup.
    ///
    /// `POST /api/s/{site}/cmd/backup` with `{"cmd": "backup"}`
    pub async fn create_backup(&self) -> Result<(), Error> {
        let url = self.site_url("cmd/backup");
        debug!("creating backup");
        let _: Vec<serde_json::Value> = self.post(url, &json!({ "cmd": "backup" })).await?;
        Ok(())
    }
}
