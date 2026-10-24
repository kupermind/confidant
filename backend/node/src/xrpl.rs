//! The read-only view of the XRP Ledger the node needs ([`Ledger`]) and the pure parsers that turn
//! its answers into what the core needs: a [`DepositRef`] for a shield's deposit, and the pool account's
//! [`AnchorMemo`]s in ledger order.

use std::future::Future;

use confidant_core::memo::{AnchorMemo, MEMO_TYPE as MEMO_ANCHOR};
use confidant_core::DepositRef;
use serde_json::Value;

/// Memo type of a deposit Payment: `MemoData` is the hex of the shield bundle's hash.
pub const MEMO_SHIELD: &str = "confidant/shield";
/// `lsfDisableMaster` in `AccountRoot.Flags`.
pub const LSF_DISABLE_MASTER: u64 = 0x0010_0000;

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("ledger rpc: {0}")]
    Rpc(String),
    #[error("deposit rejected: {0}")]
    Deposit(&'static str),
    #[error("anchor memo on ledger is malformed: {0}")]
    Memo(String),
    #[error("anchor rejected: {0}")]
    Anchor(&'static str),
    #[error("pool custody on the ledger is not the configured quorum: {0}")]
    Custody(String),
    #[error("ledger history incomplete: {0}")]
    History(&'static str),
    #[error("fee sweep rejected: {0}")]
    Sweep(&'static str),
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
    /// The `account_info` command with `signer_lists: true` (the full result object).
    fn account_info(
        &self,
        account: &str,
    ) -> impl Future<Output = Result<Value, LedgerError>> + Send;
    /// Index of the latest validated ledger (for the screening standby).
    fn validated_ledger_index(&self) -> impl Future<Output = Result<u64, LedgerError>> + Send;
    /// `close_time` (seconds since the Ripple epoch) of the validated ledger `index` (the `ledger`
    /// command) — immutable once validated, so every node derives the same value from it.
    fn ledger_close_time(
        &self,
        index: u64,
    ) -> impl Future<Output = Result<u64, LedgerError>> + Send;
    /// Every Credential ledger object held by `account` on the validated ledger (`account_objects`
    /// with `type: credential`) — issued to it, or by it.
    fn account_credentials(
        &self,
        account: &str,
    ) -> impl Future<Output = Result<Vec<Value>, LedgerError>> + Send;
    /// The Credential ledger entry for `(subject, issuer, credential_type)` on the validated ledger
    /// (`ledger_entry` with `credential`): `Some(node)`, or `None` when there is no such entry.
    fn credential(
        &self,
        subject: &str,
        issuer: &str,
        credential_type: &str,
    ) -> impl Future<Output = Result<Option<Value>, LedgerError>> + Send;
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

/// The screening facts of a deposit, read from the same `tx` answer: who sent it and in which
/// ledger it validated. Nothing here comes from the client or the coordinator.
pub fn deposit_source(tx: &Value) -> Result<(String, u64), LedgerError> {
    let account = tx_json(tx)
        .get("Account")
        .and_then(Value::as_str)
        .ok_or(LedgerError::Deposit("no sender account"))?
        .to_owned();
    let ledger_index = tx
        .get("ledger_index")
        .and_then(Value::as_u64)
        .ok_or(LedgerError::Deposit("no ledger index"))?;
    Ok((account, ledger_index))
}

/// The account's XRP balance in drops from an `account_info` answer.
pub fn account_balance(info: &Value) -> Result<u64, LedgerError> {
    info.get("account_data")
        .and_then(|a| a.get("Balance"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| LedgerError::Rpc("account_info without an XRP Balance".into()))
}

/// A fee sweep as the ledger shows it: a validated XRP Payment from the pool to the treasury.
/// Returns the delivered drops.
pub fn sweep_facts(tx: &Value, pool: &str, treasury: &str) -> Result<u64, LedgerError> {
    if tx.get("validated").and_then(Value::as_bool) != Some(true) {
        return Err(LedgerError::Sweep("not validated"));
    }
    let json = tx_json(tx);
    if json.get("TransactionType").and_then(Value::as_str) != Some("Payment") {
        return Err(LedgerError::Sweep("not a Payment"));
    }
    if json.get("Account").and_then(Value::as_str) != Some(pool) {
        return Err(LedgerError::Sweep("not sent by the pool account"));
    }
    let dest = json
        .get("Destination")
        .and_then(Value::as_str)
        .unwrap_or("");
    if crate::address::canonical(dest).as_deref() != Some(treasury) {
        return Err(LedgerError::Sweep("destination is not the treasury"));
    }
    tx.get("meta")
        .and_then(|m| m.get("delivered_amount"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or(LedgerError::Sweep(
            "delivered_amount missing or not an XRP drops amount",
        ))
}

/// Every validated sweep (pool → treasury Payment) in an `account_tx` listing: `(hash, drops)`.
pub fn sweeps_in(txs: &[Value], pool: &str, treasury: &str) -> Vec<(String, u64)> {
    txs.iter()
        .filter_map(|tx| {
            let hash = tx
                .get("hash")
                .or_else(|| tx_json(tx).get("hash"))
                .and_then(Value::as_str)?
                .to_owned();
            sweep_facts(tx, pool, treasury)
                .ok()
                .map(|drops| (hash, drops))
        })
        .collect()
}

/// Turns a `tx` result for a pool anchor Payment into its memo: validated, sent by the pool,
/// carrying a `confidant/anchor` memo. The node applies this to what it read from the ledger
/// itself when a coordinator claims a root is anchored.
pub fn anchor_fact(tx: &Value, pool: &str) -> Result<AnchorMemo, LedgerError> {
    if tx.get("validated").and_then(Value::as_bool) != Some(true) {
        return Err(LedgerError::Anchor("not validated"));
    }
    let json = tx_json(tx);
    if json.get("Account").and_then(Value::as_str) != Some(pool) {
        return Err(LedgerError::Anchor("not sent by the pool account"));
    }
    let (kind, data) = first_memo(json).ok_or(LedgerError::Anchor("no confidant/anchor memo"))?;
    if kind != MEMO_ANCHOR {
        return Err(LedgerError::Anchor("memo type is not confidant/anchor"));
    }
    AnchorMemo::decode_hex(&data).map_err(|e| LedgerError::Memo(e.to_string()))
}

/// The scan must begin where the pool account's history begins: its first entry is the
/// transaction that created the account (an `AccountRoot` `CreatedNode` for `pool`). A server with
/// partial history would otherwise hand back a suffix whose memos happen to match a prefix of the
/// state, and a node must not serve from that.
pub fn ensure_full_history(txs: &[Value], pool: &str) -> Result<(), LedgerError> {
    let first = txs.first().ok_or(LedgerError::History(
        "account_tx returned nothing for the pool account",
    ))?;
    let created = first
        .get("meta")
        .and_then(|m| m.get("AffectedNodes"))
        .and_then(Value::as_array)
        .is_some_and(|nodes| {
            nodes.iter().any(|n| {
                let c = &n["CreatedNode"];
                c.get("LedgerEntryType").and_then(Value::as_str) == Some("AccountRoot")
                    && c.get("NewFields")
                        .and_then(|f| f.get("Account"))
                        .and_then(Value::as_str)
                        == Some(pool)
            })
        });
    if !created {
        return Err(LedgerError::History(
            "account_tx does not start at the pool account's creation (server history incomplete?)",
        ));
    }
    Ok(())
}

/// The pool account's custody as the node expects it: exactly `expected` signers (any order, each
/// with weight 1), `SignerQuorum == quorum`, and the master key disabled. A node must not serve
/// against a pool whose on-ledger custody differs from the quorum it believes in.
pub fn check_custody_info(
    info: &Value,
    expected: &[String],
    quorum: u64,
) -> Result<(), LedgerError> {
    let flags = info
        .get("account_data")
        .and_then(|a| a.get("Flags"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if flags & LSF_DISABLE_MASTER == 0 {
        return Err(LedgerError::Custody("master key is not disabled".into()));
    }
    // API v2 puts `signer_lists` at the top level, v1 under `account_data`.
    let list = info
        .get("signer_lists")
        .or_else(|| info.get("account_data").and_then(|a| a.get("signer_lists")))
        .and_then(Value::as_array)
        .and_then(|l| l.first())
        .ok_or_else(|| LedgerError::Custody("no SignerList on the pool account".into()))?;
    let q = list
        .get("SignerQuorum")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if q != quorum {
        return Err(LedgerError::Custody(format!(
            "SignerQuorum is {q}, expected {quorum}"
        )));
    }
    let entries = list
        .get("SignerEntries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut on_ledger = Vec::new();
    for e in &entries {
        let e = &e["SignerEntry"];
        if e.get("SignerWeight").and_then(Value::as_u64) != Some(1) {
            return Err(LedgerError::Custody("every signer weight must be 1".into()));
        }
        on_ledger.push(
            e.get("Account")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        );
    }
    let mut want = expected.to_vec();
    on_ledger.sort();
    want.sort();
    if on_ledger != want {
        return Err(LedgerError::Custody(format!(
            "signers on ledger {on_ledger:?} differ from the configured {want:?}"
        )));
    }
    Ok(())
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
