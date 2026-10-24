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

    async fn account_info(&self, account: &str) -> Result<Value, LedgerError> {
        self.call(
            "account_info",
            json!({ "account": account, "ledger_index": "validated", "signer_lists": true }),
        )
        .await
    }

    async fn credential(
        &self,
        subject: &str,
        issuer: &str,
        credential_type: &str,
    ) -> Result<Option<Value>, LedgerError> {
        let r = self
            .call(
                "ledger_entry",
                json!({
                    "credential": { "subject": subject, "issuer": issuer, "credential_type": credential_type },
                    "ledger_index": "validated",
                }),
            )
            .await;
        match r {
            Ok(v) => Ok(v.get("node").cloned()),
            // No such entry is an ordinary answer, not a failure (rippled: code `entryNotFound`,
            // message "Entry not found."; the gate then refuses with 403, not 502).
            Err(LedgerError::Rpc(m)) if crate::credential::is_entry_not_found(&m) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn account_credentials(&self, account: &str) -> Result<(Vec<Value>, bool), LedgerError> {
        let r = self
            .call(
                "account_objects",
                json!({ "account": account, "type": "credential", "ledger_index": "validated", "limit": 400 }),
            )
            .await?;
        let objects = r
            .get("account_objects")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| {
                LedgerError::Rpc("account_objects: no account_objects in the answer".into())
            })?;
        Ok((objects, r.get("marker").is_some()))
    }

    async fn ledger_close_time(&self, index: u64) -> Result<u64, LedgerError> {
        let r = self
            .call("ledger", json!({ "ledger_index": index }))
            .await?;
        r.get("ledger")
            .and_then(|l| l.get("close_time"))
            .and_then(Value::as_u64)
            .ok_or_else(|| LedgerError::Rpc(format!("ledger {index}: no close_time in the answer")))
    }

    async fn validated_ledger_index(&self) -> Result<u64, LedgerError> {
        let r = self
            .call("ledger", json!({ "ledger_index": "validated" }))
            .await?;
        // API v2: `ledger_index` (number) at the top level; v1: a string under `ledger`.
        r.get("ledger_index")
            .and_then(Value::as_u64)
            .or_else(|| {
                r.get("ledger")
                    .and_then(|l| l.get("ledger_index"))
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
            })
            .ok_or_else(|| LedgerError::Rpc("ledger: no ledger_index".into()))
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
