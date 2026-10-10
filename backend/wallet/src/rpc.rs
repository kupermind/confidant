//! The real [`LogSource`]: a coordinator's (or any node's) `GET /log` over HTTP. Network code,
//! exercised by the UI smoke test rather than by unit tests.

use confidant_core::BundleRecord;
use serde::Deserialize;

use crate::store::LogSource;

/// `GET {url}/log?from=0` on a coordinator or any node.
pub struct HttpLog {
    url: String,
    http: reqwest::Client,
}

impl HttpLog {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into().trim_end_matches('/').to_owned(),
            http: reqwest::Client::new(),
        }
    }
}

#[derive(Deserialize)]
struct LogResponse {
    records: Vec<BundleRecord>,
}

impl LogSource for HttpLog {
    async fn log(&self) -> Result<Vec<BundleRecord>, String> {
        let r: LogResponse = self
            .http
            .get(format!("{}/log?from=0", self.url))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        Ok(r.records)
    }
}
