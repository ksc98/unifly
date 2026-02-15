// Legacy API site endpoints
//
// Site listing is controller-scoped (not site-scoped), using
// `/api/self/sites` rather than the usual `/api/s/{site}/...` pattern.

use tracing::debug;

use crate::error::Error;
use crate::legacy::client::LegacyClient;
use crate::legacy::models::LegacySite;

impl LegacyClient {
    /// List all sites visible to the authenticated user.
    ///
    /// `GET /api/self/sites` (controller-level, not site-scoped)
    pub async fn list_sites(&self) -> Result<Vec<LegacySite>, Error> {
        let url = self.api_url("self/sites");
        debug!("listing sites");
        self.get(url).await
    }
}
