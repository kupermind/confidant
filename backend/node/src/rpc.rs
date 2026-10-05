//! The real [`Ledger`]: JSON-RPC over HTTPS to a rippled / clio server. Network code, exercised
//! by the testnet harness rather than by unit tests (the parsers it feeds are unit-tested).

use serde_json::{json, Value};

use crate::xrpl::{Ledger, LedgerError};

/// JSON-RPC over HTTPS to a rippled / clio server (e.g. `https://s.altnet.rippletest.net:51234`).
pub struct JsonRpc {
    url: String,
    http: reqwest::Client,
}

impl JsonRpc {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            http: reqwest::Client::new(),
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, LedgerError> {
        let body = json!({ "method": method, "params": [params] });
        let resp: Value = self
            .http
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| LedgerError::Rpc(e.to_string()))?
            .json()
            .await
            .map_err(|e| LedgerError::Rpc(e.to_string()))?;
        let result = resp
            .get("result")
            .cloned()
            .ok_or_else(|| LedgerError::Rpc("no result".into()))?;
        if result.get("status").and_then(Value::as_str) == Some("error") {
            let msg = result
                .get("error_message")
                .or_else(|| result.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(LedgerError::Rpc(msg.to_owned()));
        }
        Ok(result)
    }
}

impl Ledger for JsonRpc {
    async fn tx(&self, hash: &str) -> Result<Value, LedgerError> {
        self.call("tx", json!({ "transaction": hash })).await
    }

    async fn account_tx(&self, account: &str) -> Result<Vec<Value>, LedgerError> {
        let mut out = Vec::new();
        let mut marker: Option<Value> = None;
        loop {
            let mut params = json!({
                "account": account,
                "ledger_index_min": -1,
                "ledger_index_max": -1,
                "forward": true,
                "limit": 200,
            });
            if let Some(m) = &marker {
                params["marker"] = m.clone();
            }
            let result = self.call("account_tx", params).await?;
            if let Some(txs) = result.get("transactions").and_then(Value::as_array) {
                out.extend(txs.iter().cloned());
            }
            match result.get("marker") {
                Some(m) if !m.is_null() => marker = Some(m.clone()),
                _ => break,
            }
        }
        Ok(out)
    }
}
