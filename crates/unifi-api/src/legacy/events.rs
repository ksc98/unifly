// Legacy API event and alarm endpoints
//
// Events (stat/event) and alarms (stat/alarm) with archive support
// via cmd/evtmgr.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::legacy::client::LegacyClient;
use crate::legacy::models::{LegacyAlarm, LegacyEvent};

impl LegacyClient {
    /// List recent events.
    ///
    /// `GET /api/s/{site}/stat/event`
    ///
    /// If `count` is provided, limits the number of events returned
    /// by appending `?_limit={count}` to the request.
    pub async fn list_events(&self, count: Option<u32>) -> Result<Vec<LegacyEvent>, Error> {
        let path = match count {
            Some(n) => format!("stat/event?_limit={n}"),
            None => "stat/event".to_string(),
        };
        let url = self.site_url(&path);
        debug!(?count, "listing events");
        self.get(url).await
    }

    /// List active alarms.
    ///
    /// `GET /api/s/{site}/stat/alarm`
    pub async fn list_alarms(&self) -> Result<Vec<LegacyAlarm>, Error> {
        let url = self.site_url("stat/alarm");
        debug!("listing alarms");
        self.get(url).await
    }

    /// Archive (acknowledge) a specific alarm by its ID.
    ///
    /// `POST /api/s/{site}/cmd/evtmgr` with `{"cmd": "archive-alarm", "_id": "..."}`
    pub async fn archive_alarm(&self, id: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/evtmgr");
        debug!(id, "archiving alarm");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "archive-alarm",
                    "_id": id,
                }),
            )
            .await?;
        Ok(())
    }
}
