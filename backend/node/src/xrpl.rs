//! The read-only view of the XRP Ledger the node needs ([`Ledger`]) and the pure parsers that turn
//! its answers into what the core needs: a [`DepositRef`] for a shield's deposit, and the pool account's
//! [`AnchorMemo`]s in ledger order.

use std::future::Future;

use confidant_core::memo::{AnchorMemo, MEMO_TYPE as MEMO_ANCHOR};
use confidant_core::DepositRef;
use serde_json::Value;

/// Memo type of a deposit Payment: `MemoData` is the hex of the shield bundle's hash.
pub const MEMO_SHIELD: &str = "confidant/shield";

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("ledger rpc: {0}")]
    Rpc(String),
    #[error("deposit rejected: {0}")]
    Deposit(&'static str),
    #[error("anchor memo on ledger is malformed: {0}")]
    Memo(String),
}

/// What the node needs from the ledger. Implemented by [`JsonRpc`] and by test fakes.
pub trait Ledger: Send + Sync {
    /// The `tx` command for one transaction hash (the full result object).
    fn tx(&self, hash: &str) -> impl Future<Output = Result<Value, LedgerError>> + Send;
    /// Every validated transaction of `account`, oldest first (the `account_tx` result objects).
    fn account_tx(
        &self,
        account: &str,
    ) -> impl Future<Output = Result<Vec<Value>, LedgerError>> + Send;
}

fn tx_json(tx: &Value) -> &Value {
    // rippled API v2 nests the transaction under `tx_json` (`tx` in `account_tx` v1); fall back to
    // the object itself for v1 `tx` answers.
    tx.get("tx_json").or_else(|| tx.get("tx")).unwrap_or(tx)
}

fn first_memo(json: &Value) -> Option<(String, String)> {
    let memo = json.get("Memos")?.as_array()?.first()?.get("Memo")?;
    let kind = hex::decode(memo.get("MemoType")?.as_str()?).ok()?;
    let kind = String::from_utf8(kind).ok()?;
    let data = memo.get("MemoData")?.as_str()?.to_ascii_lowercase();
    Some((kind, data))
}

/// Turns a `tx` result for a deposit into the facts the core enforces. The node applies this to
/// what it read from the ledger itself; nothing here comes from the client or the coordinator.
pub fn deposit_facts(tx: &Value, tx_hash: &str, pool: &str) -> Result<DepositRef, LedgerError> {
    if tx.get("validated").and_then(Value::as_bool) != Some(true) {
        return Err(LedgerError::Deposit("not validated"));
    }
    let json = tx_json(tx);
    if json.get("TransactionType").and_then(Value::as_str) != Some("Payment") {
        return Err(LedgerError::Deposit("not a Payment"));
    }
    if json.get("Destination").and_then(Value::as_str) != Some(pool) {
        return Err(LedgerError::Deposit("destination is not the pool account"));
    }
    let delivered = tx
        .get("meta")
        .and_then(|m| m.get("delivered_amount"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or(LedgerError::Deposit(
            "delivered_amount missing or not an XRP drops amount",
        ))?;
    let (kind, memo_hash) =
        first_memo(json).ok_or(LedgerError::Deposit("no confidant/shield memo"))?;
    if kind != MEMO_SHIELD {
        return Err(LedgerError::Deposit("memo type is not confidant/shield"));
    }
    if memo_hash.len() != 64 || !memo_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(LedgerError::Deposit(
            "memo is not a 32-byte hex bundle hash",
        ));
    }
    Ok(DepositRef {
        tx_hash: tx_hash.to_owned(),
        delivered,
        memo_hash,
    })
}

/// Extracts the pool account's `confidant/anchor` memos, in the order given (ledger order when the
/// list comes from `account_tx` with `forward: true`). Payments *from* the pool only; unvalidated
/// entries are skipped; a malformed memo is an error, not something to skip.
pub fn anchor_memos(txs: &[Value], pool: &str) -> Result<Vec<AnchorMemo>, LedgerError> {
    let mut memos = Vec::new();
    for tx in txs {
        if tx.get("validated").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let json = tx_json(tx);
        if json.get("Account").and_then(Value::as_str) != Some(pool) {
            continue;
        }
        let Some((kind, data)) = first_memo(json) else {
            continue;
        };
        if kind != MEMO_ANCHOR {
            continue;
        }
        memos.push(AnchorMemo::decode_hex(&data).map_err(|e| LedgerError::Memo(e.to_string()))?);
    }
    Ok(memos)
}
