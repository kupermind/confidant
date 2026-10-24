//! Node logic and HTTP surface against a fake ledger: deposit facts, submissions, endorse
//! (re-lookup + idempotency), rebuild from the ledger, a lagging node, error mapping.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use confidant_core::memo::MEMO_TYPE;
use confidant_core::record::bundle_hash;
use confidant_core::{DepositRef, Error as CoreError, Submission, Verifier, Wallet};
use confidant_node::{
    anchor_memos, canonical, deposit_facts, deposit_source, http, is_entry_not_found, ripple_now,
    CredentialError, CredentialGate, FeeLedger, FeePolicy, Ledger, LedgerError, Node, NodeError,
    Screening, LSF_ACCEPTED, LSF_DISABLE_MASTER, MEMO_SHIELD,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const POOL: &str = "rPoolAccount";

/// A ledger the tests control: `tx` answers by hash, `account_tx` is the list of the pool's txs.
#[derive(Default, Clone)]
struct FakeLedger {
    txs: Arc<Mutex<BTreeMap<String, Value>>>,
    account: Arc<Mutex<Vec<Value>>>,
    info: Arc<Mutex<Value>>,
    /// Latest validated ledger index the fake reports (0 = "unknown", the call errors).
    validated: Arc<Mutex<u64>>,
    /// Credential entries keyed `subject|issuer|type`, how often they were asked for, and a switch
    /// that makes the lookup fail like an unreachable RPC.
    credentials: Arc<Mutex<BTreeMap<String, Value>>>,
    credential_calls: Arc<Mutex<u64>>,
    credential_fail: Arc<Mutex<bool>>,
}

impl FakeLedger {
    /// A Credential ledger entry for `(subject, issuer, credential_type)`.
    fn put_credential(
        &self,
        subject: &str,
        issuer: &str,
        credential_type: &str,
        accepted: bool,
        expiration: Option<u64>,
    ) {
        let mut e = json!({ "LedgerEntryType": "Credential", "Subject": subject, "Issuer": issuer, "CredentialType": credential_type,
            "Flags": if accepted { LSF_ACCEPTED } else { 0 } });
        if let Some(x) = expiration {
            e["Expiration"] = json!(x);
        }
        self.credentials
            .lock()
            .unwrap()
            .insert(format!("{subject}|{issuer}|{credential_type}"), e);
    }
    fn clear_credentials(&self) {
        self.credentials.lock().unwrap().clear();
    }
    fn credential_calls(&self) -> u64 {
        *self.credential_calls.lock().unwrap()
    }
    fn fail_credentials(&self, on: bool) {
        *self.credential_fail.lock().unwrap() = on;
    }
    fn put_tx(&self, hash: &str, v: Value) {
        self.txs.lock().unwrap().insert(hash.to_owned(), v);
    }
    /// A pool anchor Payment answer to `tx`, from `account`, carrying `memo_hex` as its anchor memo.
    fn put_anchor(&self, hash: &str, account: &str, memo_hex: &str, validated: bool) {
        self.put_tx(hash, json!({
            "validated": validated,
            "tx_json": { "Account": account, "TransactionType": "Payment", "Destination": "rSink",
                "Memos": [{ "Memo": { "MemoType": hex::encode(MEMO_TYPE), "MemoData": memo_hex.to_uppercase() } }] }
        }));
    }
    /// The pool account's creation (the faucet Payment) — the first entry of a complete history.
    fn created(&self) {
        self.account.lock().unwrap().push(json!({
            "validated": true,
            "tx_json": { "Account": "rFaucet", "TransactionType": "Payment", "Destination": POOL },
            "meta": { "AffectedNodes": [{ "CreatedNode": { "LedgerEntryType": "AccountRoot", "NewFields": { "Account": POOL } } }] }
        }));
    }
    fn set_info(&self, v: Value) {
        *self.info.lock().unwrap() = v;
    }
    fn set_validated(&self, n: u64) {
        *self.validated.lock().unwrap() = n;
    }
    fn push_anchor(&self, memo_hex: &str) {
        self.account.lock().unwrap().push(json!({
            "validated": true,
            "tx_json": { "Account": POOL, "TransactionType": "Payment", "Destination": "rSink",
                "Memos": [{ "Memo": { "MemoType": hex::encode(MEMO_TYPE), "MemoData": memo_hex.to_uppercase() } }] }
        }));
    }
}

impl Ledger for FakeLedger {
    async fn tx(&self, hash: &str) -> Result<Value, LedgerError> {
        self.txs
            .lock()
            .unwrap()
            .get(hash)
            .cloned()
            .ok_or_else(|| LedgerError::Rpc(format!("txnNotFound {hash}")))
    }
    async fn account_tx(&self, _account: &str) -> Result<Vec<Value>, LedgerError> {
        Ok(self.account.lock().unwrap().clone())
    }
    async fn account_info(&self, account: &str) -> Result<Value, LedgerError> {
        let v = self.info.lock().unwrap().clone();
        if v.is_null() {
            return Err(LedgerError::Rpc(format!("actNotFound {account}")));
        }
        Ok(v)
    }
    async fn validated_ledger_index(&self) -> Result<u64, LedgerError> {
        let n = *self.validated.lock().unwrap();
        if n == 0 {
            return Err(LedgerError::Rpc("ledger: unavailable".into()));
        }
        Ok(n)
    }
    async fn credential(
        &self,
        subject: &str,
        issuer: &str,
        credential_type: &str,
    ) -> Result<Option<Value>, LedgerError> {
        *self.credential_calls.lock().unwrap() += 1;
        if *self.credential_fail.lock().unwrap() {
            return Err(LedgerError::Rpc("ledger_entry: unavailable".into()));
        }
        Ok(self
            .credentials
            .lock()
            .unwrap()
            .get(&format!("{subject}|{issuer}|{credential_type}"))
            .cloned())
    }
}

/// An `account_info` answer (API v2 shape) with the given SignerList and master-key state.
fn custody(signers: &[&str], quorum: u64, master_disabled: bool) -> Value {
    json!({
        "account_data": { "Account": POOL, "Flags": if master_disabled { LSF_DISABLE_MASTER } else { 0 }, "Sequence": 7 },
        "signer_lists": [{ "SignerQuorum": quorum,
            "SignerEntries": signers.iter().map(|s| json!({ "SignerEntry": { "Account": s, "SignerWeight": 1 } })).collect::<Vec<_>>() }]
    })
}

#[tokio::test]
async fn node_refuses_to_serve_unless_the_pool_custody_is_the_configured_quorum() {
    let ledger = FakeLedger::default();
    let signers: Vec<String> = ["rS1", "rS2", "rS3"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(matches!(
        Node::<FakeLedger>::check_custody(&ledger, POOL, &signers, 2).await,
        Err(NodeError::Ledger(LedgerError::Rpc(_)))
    ));
    ledger.set_info(custody(&["rS1", "rS2", "rS3"], 2, true));
    Node::<FakeLedger>::check_custody(&ledger, POOL, &signers, 2)
        .await
        .unwrap();
    // Order does not matter; the v1 shape (signer_lists under account_data) is read too.
    ledger.set_info(custody(&["rS3", "rS1", "rS2"], 2, true));
    Node::<FakeLedger>::check_custody(&ledger, POOL, &signers, 2)
        .await
        .unwrap();
    let mut v1 = json!({ "account_data": { "Flags": LSF_DISABLE_MASTER } });
    v1["account_data"]["signer_lists"] =
        custody(&["rS1", "rS2", "rS3"], 2, true)["signer_lists"].clone();
    ledger.set_info(v1);
    Node::<FakeLedger>::check_custody(&ledger, POOL, &signers, 2)
        .await
        .unwrap();
    // Every deviation refuses: master enabled, other quorum, other/missing/extra signer, weight ≠ 1, no list.
    let mut heavy = custody(&["rS1", "rS2", "rS3"], 2, true);
    heavy["signer_lists"][0]["SignerEntries"][0]["SignerEntry"]["SignerWeight"] = json!(2);
    for (info, what) in [
        (
            custody(&["rS1", "rS2", "rS3"], 2, false),
            "master key is not disabled",
        ),
        (
            custody(&["rS1", "rS2", "rS3"], 3, true),
            "SignerQuorum is 3, expected 2",
        ),
        (
            custody(&["rS1", "rS2", "rEve"], 2, true),
            "differ from the configured",
        ),
        (
            custody(&["rS1", "rS2"], 2, true),
            "differ from the configured",
        ),
        (
            custody(&["rS1", "rS2", "rS3", "rS4"], 2, true),
            "differ from the configured",
        ),
        (heavy, "weight must be 1"),
        (
            json!({ "account_data": { "Flags": LSF_DISABLE_MASTER } }),
            "no SignerList",
        ),
    ] {
        ledger.set_info(info);
        match Node::<FakeLedger>::check_custody(&ledger, POOL, &signers, 2).await {
            Err(NodeError::Ledger(LedgerError::Custody(m))) => {
                assert!(m.contains(what), "{m:?} should mention {what:?}")
            }
            other => panic!("expected a custody refusal ({what}), got {other:?}"),
        }
    }
}

fn deposit_tx(dest: &str, drops: &str, memo_hash: &str) -> Value {
    json!({
        "validated": true,
        "ledger_index": 100,
        "tx_json": { "TransactionType": "Payment", "Account": "rAlice", "Destination": dest,
            "Memos": [{ "Memo": { "MemoType": hex::encode(MEMO_SHIELD), "MemoData": memo_hash.to_uppercase() } }] },
        "meta": { "delivered_amount": drops }
    })
}

fn node(ledger: &FakeLedger) -> Node<FakeLedger> {
    Node::new(Verifier::new(), ledger.clone(), POOL, None)
}

#[test]
fn deposit_facts_enforce_the_ledger_shape() {
    let h = "a".repeat(64);
    let ok = deposit_facts(&deposit_tx(POOL, "25000000", &h), "T1", POOL).unwrap();
    assert_eq!(ok.delivered, 25_000_000);
    assert_eq!(ok.memo_hash, h);
    assert_eq!(ok.tx_hash, "T1");
    let mut v = deposit_tx(POOL, "1", &h);
    v["validated"] = json!(false);
    assert!(matches!(
        deposit_facts(&v, "T", POOL),
        Err(LedgerError::Deposit("not validated"))
    ));
    let mut v = deposit_tx(POOL, "1", &h);
    v["tx_json"]["TransactionType"] = json!("OfferCreate");
    assert!(matches!(
        deposit_facts(&v, "T", POOL),
        Err(LedgerError::Deposit("not a Payment"))
    ));
    assert!(matches!(
        deposit_facts(&deposit_tx("rSomeoneElse", "1", &h), "T", POOL),
        Err(LedgerError::Deposit("destination is not the pool account"))
    ));
    let mut v = deposit_tx(POOL, "1", &h);
    v["meta"]["delivered_amount"] = json!({ "currency": "USD", "value": "1" });
    assert!(
        matches!(deposit_facts(&v, "T", POOL), Err(LedgerError::Deposit(m)) if m.contains("XRP"))
    );
    // A string that is not a drops integer is refused, never read as 0.
    for bad in ["25.5", "abc", "", "-1"] {
        let mut v = deposit_tx(POOL, bad, &h);
        assert!(
            matches!(deposit_facts(&v, "T", POOL), Err(LedgerError::Deposit(m)) if m.contains("XRP")),
            "delivered_amount {bad:?} must be refused"
        );
        v["meta"]
            .as_object_mut()
            .unwrap()
            .remove("delivered_amount");
        assert!(
            matches!(deposit_facts(&v, "T", POOL), Err(LedgerError::Deposit(m)) if m.contains("XRP"))
        );
    }
    let mut v = deposit_tx(POOL, "1", &h);
    v["tx_json"]["Memos"] = json!([]);
    assert!(matches!(
        deposit_facts(&v, "T", POOL),
        Err(LedgerError::Deposit("no confidant/shield memo"))
    ));
    let mut v = deposit_tx(POOL, "1", &h);
    v["tx_json"]["Memos"][0]["Memo"]["MemoType"] = json!(hex::encode("something/else"));
    assert!(matches!(
        deposit_facts(&v, "T", POOL),
        Err(LedgerError::Deposit("memo type is not confidant/shield"))
    ));
    assert!(matches!(
        deposit_facts(&deposit_tx(POOL, "1", "abcd"), "T", POOL),
        Err(LedgerError::Deposit(
            "memo is not a 32-byte hex bundle hash"
        ))
    ));
    // v1-shaped answer (fields at the top level) is accepted too.
    let flat = json!({ "validated": true, "TransactionType": "Payment", "Destination": POOL,
        "Memos": [{ "Memo": { "MemoType": hex::encode(MEMO_SHIELD), "MemoData": h } }],
        "meta": { "delivered_amount": "7" } });
    assert_eq!(deposit_facts(&flat, "T", POOL).unwrap().delivered, 7);
}

#[test]
fn anchor_memos_skip_foreign_and_unvalidated_and_reject_malformed() {
    let l = FakeLedger::default();
    let mut v = Verifier::new();
    let w = Wallet::new();
    let p = w.build_shield(5).unwrap();
    let dep = DepositRef {
        tx_hash: "D".into(),
        delivered: 5,
        memo_hash: p.hash_hex(),
    };
    v.submit_shield(&p.record, &dep).unwrap();
    let memo = v.anchor_memo(0).unwrap();
    l.push_anchor(&memo.encode_hex().unwrap());
    // Noise: a payment from someone else, an unvalidated one, a different memo type.
    l.account.lock().unwrap().push(json!({ "validated": true, "tx_json": { "Account": "rOther", "Memos": [{ "Memo": { "MemoType": hex::encode(MEMO_TYPE), "MemoData": "00" } }] } }));
    l.account.lock().unwrap().push(json!({ "validated": false, "tx_json": { "Account": POOL, "Memos": [{ "Memo": { "MemoType": hex::encode(MEMO_TYPE), "MemoData": "00" } }] } }));
    l.account.lock().unwrap().push(json!({ "validated": true, "tx_json": { "Account": POOL, "Memos": [{ "Memo": { "MemoType": hex::encode("other"), "MemoData": "00" } }] } }));
    l.account
        .lock()
        .unwrap()
        .push(json!({ "validated": true, "tx_json": { "Account": POOL } }));
    let memos = anchor_memos(&l.account.lock().unwrap(), POOL).unwrap();
    assert_eq!(memos, vec![memo]);
    // A malformed anchor memo from the pool account is an error, not skipped.
    l.account.lock().unwrap().push(json!({ "validated": true, "tx_json": { "Account": POOL, "Memos": [{ "Memo": { "MemoType": hex::encode(MEMO_TYPE), "MemoData": "0000" } }] } }));
    assert!(matches!(
        anchor_memos(&l.account.lock().unwrap(), POOL),
        Err(LedgerError::Memo(_))
    ));
}

#[tokio::test]
async fn node_accepts_shields_only_against_ledger_facts_and_endorses_idempotently() {
    let ledger = FakeLedger::default();
    let coord = node(&ledger);
    let signer = node(&ledger);
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();

    // Shield: the node reads the deposit itself.
    let p = alice.build_shield(100).unwrap();
    ledger.put_tx("DEP1", deposit_tx(POOL, "100", &p.hash_hex()));
    assert!(matches!(
        coord.submit_shield(&p.record, "MISSING").await,
        Err(NodeError::Ledger(LedgerError::Rpc(_)))
    ));
    ledger.put_tx("SHORT", deposit_tx(POOL, "99", &p.hash_hex()));
    assert!(matches!(
        coord.submit_shield(&p.record, "SHORT").await,
        Err(NodeError::Core(CoreError::ValueBalance { .. }))
    ));
    let acc = coord.submit_shield(&p.record, "DEP1").await.unwrap();
    assert_eq!(acc.accepted.bundle_index, 0);
    assert!(
        !acc.memo_hex.is_empty(),
        "memo_hex is what the quorum signs"
    );
    assert!(coord.state().consumed_deposits == 1);

    // The signer endorses with the coordinator's claimed facts being WRONG: it re-reads the ledger.
    let lying = Submission::Shield {
        record: p.record.clone(),
        deposit: DepositRef {
            tx_hash: "DEP1".into(),
            delivered: 1,
            memo_hash: "00".repeat(32),
        },
    };
    let e = signer.endorse(&lying, &acc.memo_hex).await.unwrap();
    assert_eq!(e.accepted.new_root, acc.accepted.new_root);
    assert_eq!(signer.state().root, coord.state().root);
    // Idempotent retry: same answer, nothing re-applied.
    let again = signer.endorse(&lying, &acc.memo_hex).await.unwrap();
    assert_eq!(again.accepted.bundle_index, 0);
    assert_eq!(again.accepted.new_root, e.accepted.new_root);
    assert_eq!(signer.state().bundle_count, 1);
    // A different memo for the same bundle is NOT the idempotent case: refused.
    let mut wrong = acc.memo_hex.clone();
    wrong.replace_range(
        4..6,
        if &acc.memo_hex[4..6] == "00" {
            "01"
        } else {
            "00"
        },
    );
    assert!(signer.endorse(&lying, &wrong).await.is_err());
    assert_eq!(signer.state().bundle_count, 1);
    // The same bundle under another op, with an external context that happens to equal the
    // deposit hash, is not the idempotent case either: refused, nothing reconstructed.
    assert!(signer
        .endorse(
            &Submission::Unshield {
                record: p.record.clone(),
                destination: "DEP1".into(),
            },
            &acc.memo_hex,
        )
        .await
        .is_err());
    assert_eq!(signer.state().bundle_count, 1);

    // Transfer and unshield through both nodes; the payout comes from the node's own state.
    alice.sync(&coord.log(0)).unwrap();
    let t = alice
        .build_transfer(&[confidant_core::TransferOutput {
            recipient: bob.info().address,
            amount: 60,
        }])
        .unwrap();
    let acc_t = coord.submit_transfer(&t.record).unwrap();
    signer
        .endorse(
            &Submission::Transfer {
                record: t.record.clone(),
            },
            &acc_t.memo_hex,
        )
        .await
        .unwrap();
    // A DIFFERENT transfer presented with the applied transfer's memo (same op, same empty
    // external) is not the idempotent case: refused, the signer's state unchanged.
    alice.sync(&coord.log(0)).unwrap();
    let t2 = alice
        .build_transfer(&[confidant_core::TransferOutput {
            recipient: bob.info().address,
            amount: 10,
        }])
        .unwrap();
    assert!(signer
        .endorse(
            &Submission::Transfer {
                record: t2.record.clone(),
            },
            &acc_t.memo_hex,
        )
        .await
        .is_err());
    assert_eq!(signer.state().bundle_count, 2);
    bob.sync(&coord.log(0)).unwrap();
    let u = bob.build_unshield(25, "rBob").unwrap();
    let acc_u = coord.submit_unshield(&u.record, "rBob").unwrap();
    assert_eq!(acc_u.accepted.payout.as_ref().unwrap().amount, 25);
    let e = signer
        .endorse(
            &Submission::Unshield {
                record: u.record.clone(),
                destination: "rBob".into(),
            },
            &acc_u.memo_hex,
        )
        .await
        .unwrap();
    assert_eq!(e.accepted.payout.unwrap().destination, "rBob");
    // Idempotent retry of the unshield returns the payout again.
    let again = signer
        .endorse(
            &Submission::Unshield {
                record: u.record.clone(),
                destination: "rBob".into(),
            },
            &acc_u.memo_hex,
        )
        .await
        .unwrap();
    assert_eq!(again.accepted.payout.unwrap().amount, 25);
    // A re-pointed payout is refused by the node (sighash), not just by the sidecar.
    assert!(matches!(
        signer
            .endorse(
                &Submission::Unshield {
                    record: u.record.clone(),
                    destination: "rAttacker".into()
                },
                &acc_u.memo_hex
            )
            .await,
        Err(NodeError::Core(CoreError::MemoMismatch { .. }))
    ));
    assert_eq!(signer.state().root, coord.state().root);
    assert_eq!(coord.log(2).len(), 1);
    // Marking a root anchored needs the anchor transaction itself, read from the ledger:
    // validated, sent by the pool, carrying a confidant/anchor memo with exactly that root.
    let root = coord.state().root;
    ledger.put_anchor("ANCH", POOL, &acc_u.memo_hex, true);
    assert_eq!(
        coord.anchored(&root, "ANCH").await.unwrap().anchored_root,
        Some(root.clone())
    );
    assert!(matches!(
        coord.anchored(&root, "NOPE").await,
        Err(NodeError::Ledger(LedgerError::Rpc(_)))
    ));
    ledger.put_anchor("EVE", "rEve", &acc_u.memo_hex, true);
    assert!(matches!(
        coord.anchored(&root, "EVE").await,
        Err(NodeError::Ledger(LedgerError::Anchor(
            "not sent by the pool account"
        )))
    ));
    ledger.put_anchor("OLD", POOL, &acc_t.memo_hex, true);
    assert!(matches!(
        coord.anchored(&root, "OLD").await,
        Err(NodeError::Ledger(LedgerError::Anchor(
            "anchor memo does not carry the claimed root"
        )))
    ));
    ledger.put_anchor("PEND", POOL, &acc_u.memo_hex, false);
    assert!(matches!(
        coord.anchored(&root, "PEND").await,
        Err(NodeError::Ledger(LedgerError::Anchor("not validated")))
    ));
    let mut dep = deposit_tx(POOL, "1", &"a".repeat(64));
    dep["tx_json"]["Account"] = json!(POOL);
    ledger.put_tx("DEPMEMO", dep);
    assert!(matches!(
        coord.anchored(&root, "DEPMEMO").await,
        Err(NodeError::Ledger(LedgerError::Anchor(
            "memo type is not confidant/anchor"
        )))
    ));
    ledger.put_tx(
        "NOMEMO",
        json!({ "validated": true, "tx_json": { "Account": POOL, "TransactionType": "Payment" } }),
    );
    assert!(matches!(
        coord.anchored(&root, "NOMEMO").await,
        Err(NodeError::Ledger(LedgerError::Anchor(
            "no confidant/anchor memo"
        )))
    ));
    ledger.put_anchor("JUNK", POOL, "zz", true);
    assert!(matches!(
        coord.anchored(&root, "JUNK").await,
        Err(NodeError::Ledger(LedgerError::Memo(_)))
    ));
    // A memo carrying a root this node never computed: the ledger check passes, the verifier refuses.
    let mut alien = confidant_core::memo::AnchorMemo::decode_hex(&acc_u.memo_hex).unwrap();
    alien.root = [7u8; 32];
    ledger.put_anchor("ALIEN", POOL, &alien.encode_hex().unwrap(), true);
    assert!(matches!(
        coord.anchored(&hex::encode([7u8; 32]), "ALIEN").await,
        Err(NodeError::Core(CoreError::UnknownAnchor))
    ));
    assert_eq!(coord.reveal(&bob.info().viewing_key).unwrap().balance, 35);
}

#[tokio::test]
async fn new_node_rebuilds_from_ledger_memos_and_refuses_bad_ledgers() {
    let ledger = FakeLedger::default();
    let coord = node(&ledger);
    let mut alice = Wallet::new();
    let p = alice.build_shield(100).unwrap();
    ledger.put_tx("DEP1", deposit_tx(POOL, "100", &p.hash_hex()));
    let a0 = coord.submit_shield(&p.record, "DEP1").await.unwrap();
    alice.sync(&coord.log(0)).unwrap();
    let u = alice.build_unshield(40, "rOut").unwrap();
    let a1 = coord.submit_unshield(&u.record, "rOut").unwrap();
    ledger.created();
    ledger.push_anchor(&a0.memo_hex);
    ledger.push_anchor(&a1.memo_hex);
    let mirror = coord.log(0);

    let rebuilt =
        Node::<FakeLedger>::rebuild_from_ledger(&ledger, POOL, &mirror, &FeePolicy::off())
            .await
            .unwrap()
            .0;
    assert_eq!(rebuilt.state().root, coord.state().root);
    assert_eq!(rebuilt.state().anchored_root, Some(coord.state().root));
    assert!(rebuilt.is_deposit_consumed("DEP1"));

    // Lagging node: it has only the shield; asked to endorse the unshield it refuses, then it
    // catches up from the ledger and agrees.
    let lagging = node(&ledger);
    lagging
        .endorse(
            &Submission::Shield {
                record: p.record.clone(),
                deposit: DepositRef {
                    tx_hash: "DEP1".into(),
                    delivered: 0,
                    memo_hash: String::new(),
                },
            },
            &a0.memo_hex,
        )
        .await
        .unwrap();
    let ledger2 = FakeLedger {
        txs: ledger.txs.clone(),
        account: ledger.account.clone(),
        info: ledger.info.clone(),
        validated: ledger.validated.clone(),
        ..FakeLedger::default()
    };
    let caught =
        Node::<FakeLedger>::rebuild_from_ledger(&ledger2, POOL, &mirror, &FeePolicy::off())
            .await
            .unwrap()
            .0;
    assert_eq!(caught.state().root, coord.state().root);
    assert_eq!(lagging.state().bundle_count, 1);

    // Ledger says a different root than the mirror produces ⇒ refuse to serve.
    let bad = FakeLedger::default();
    bad.created();
    bad.txs
        .lock()
        .unwrap()
        .extend(ledger.txs.lock().unwrap().clone());
    let mut m = confidant_core::memo::AnchorMemo::decode_hex(&a1.memo_hex).unwrap();
    m.root[0] ^= 1;
    bad.push_anchor(&a0.memo_hex);
    bad.push_anchor(&m.encode_hex().unwrap());
    assert!(matches!(
        Node::<FakeLedger>::rebuild_from_ledger(&bad, POOL, &mirror, &FeePolicy::off()).await,
        Err(NodeError::Core(CoreError::MemoMismatch {
            what: "root",
            ..
        }))
    ));
    // Deposit missing from the ledger ⇒ the shield is unbacked ⇒ refuse.
    let nodep = FakeLedger::default();
    nodep.created();
    nodep.push_anchor(&a0.memo_hex);
    assert!(matches!(
        Node::<FakeLedger>::rebuild_from_ledger(&nodep, POOL, &mirror, &FeePolicy::off()).await,
        Err(NodeError::Ledger(LedgerError::Rpc(_)))
    ));
    // Deposit delivered less than the shield ⇒ refuse.
    let short = FakeLedger::default();
    short.created();
    short.put_tx("DEP1", deposit_tx(POOL, "99", &p.hash_hex()));
    short.push_anchor(&a0.memo_hex);
    assert!(matches!(
        Node::<FakeLedger>::rebuild_from_ledger(&short, POOL, &mirror, &FeePolicy::off()).await,
        Err(NodeError::Core(CoreError::ValueBalance { .. }))
    ));
    // Mirror missing a bundle ⇒ refuse.
    assert!(matches!(
        Node::<FakeLedger>::rebuild_from_ledger(&ledger, POOL, &mirror[..1], &FeePolicy::off())
            .await,
        Err(NodeError::Core(CoreError::MirrorMissing(_)))
    ));
    // A pool with only its creation on the ledger ⇒ empty verifier. A scan that does not start at
    // the pool's creation (partial-history server, or nothing at all) ⇒ refuse to serve, even when
    // the memos it does show would rebuild fine.
    let fresh = FakeLedger::default();
    fresh.created();
    let empty = Node::<FakeLedger>::rebuild_from_ledger(&fresh, POOL, &[], &FeePolicy::off())
        .await
        .unwrap()
        .0;
    assert_eq!(empty.state().bundle_count, 0);
    assert!(matches!(
        Node::<FakeLedger>::rebuild_from_ledger(
            &FakeLedger::default(),
            POOL,
            &[],
            &FeePolicy::off()
        )
        .await,
        Err(NodeError::Ledger(LedgerError::History(_)))
    ));
    let suffix = FakeLedger::default();
    suffix
        .txs
        .lock()
        .unwrap()
        .extend(ledger.txs.lock().unwrap().clone());
    suffix.push_anchor(&a0.memo_hex);
    suffix.push_anchor(&a1.memo_hex);
    assert!(matches!(
        Node::<FakeLedger>::rebuild_from_ledger(&suffix, POOL, &mirror, &FeePolicy::off()).await,
        Err(NodeError::Ledger(LedgerError::History(_)))
    ));
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    let req = match body {
        Some(b) => req.body(Body::from(b.to_string())).unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    // axum's own rejections (malformed JSON body) are plain text; wrap them so callers can
    // still inspect the status.
    let v: Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({ "error": String::from_utf8_lossy(&bytes) }));
    (status, v)
}

#[tokio::test]
async fn http_surface_maps_errors_and_round_trips() {
    let ledger = FakeLedger::default();
    let app = http::router(Arc::new(node(&ledger)));
    let (s, v) = call(&app, "GET", "/state", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["bundleCount"], 0);
    let (s, v) = call(&app, "GET", "/log?from=0", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["records"].as_array().unwrap().len(), 0);

    let alice = Wallet::new();
    let p = alice.build_shield(10).unwrap();
    // Deposit not on the ledger ⇒ 502; deposit with the wrong amount ⇒ 409 (core refusal).
    let (s, v) = call(
        &app,
        "POST",
        "/submit/shield",
        Some(json!({ "record": p.record, "depositTxHash": "NOPE" })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{v}");
    ledger.put_tx("D", deposit_tx(POOL, "9", &p.hash_hex()));
    let (s, _) = call(
        &app,
        "POST",
        "/submit/shield",
        Some(json!({ "record": p.record, "depositTxHash": "D" })),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    ledger.put_tx("D", deposit_tx("rElse", "10", &p.hash_hex()));
    let (s, _) = call(
        &app,
        "POST",
        "/submit/shield",
        Some(json!({ "record": p.record, "depositTxHash": "D" })),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    ledger.put_tx("D", deposit_tx(POOL, "10", &p.hash_hex()));
    let (s, v) = call(
        &app,
        "POST",
        "/submit/shield",
        Some(json!({ "record": p.record, "depositTxHash": "D" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let memo_hex = v["memoHex"]
        .as_str()
        .unwrap_or_else(|| panic!("no memoHex in {v}"))
        .to_owned();
    assert_eq!(v["bundleIndex"], 0);
    assert_eq!(
        v["bundleHash"],
        json!(hex::encode(bundle_hash(&p.record).unwrap()))
    );

    // Endorse through HTTP (idempotent on this same node), bad memo hex ⇒ 400, missing field ⇒ 422/400.
    let sub = json!({ "op": "shield", "record": p.record, "deposit": { "txHash": "D", "delivered": 0, "memoHash": "" } });
    let (s, v) = call(
        &app,
        "POST",
        "/endorse",
        Some(json!({ "submission": sub, "proposedMemoHex": memo_hex })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _) = call(
        &app,
        "POST",
        "/endorse",
        Some(json!({ "submission": sub, "proposedMemoHex": "zz" })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &app,
        "POST",
        "/endorse",
        Some(json!({ "submission": { "op": "transfer" } })),
    )
    .await;
    assert!(s.is_client_error());

    // Transfer with insufficient funds never reaches here (wallet refuses); a zero-amount hop
    // bundle cannot be built, so exercise the mapping with a replay instead ⇒ 409.
    let (s, _) = call(
        &app,
        "POST",
        "/submit/transfer",
        Some(json!({ "record": p.record })),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = call(
        &app,
        "POST",
        "/submit/unshield",
        Some(json!({ "record": p.record, "destination": "" })),
    )
    .await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);

    let root = call(&app, "GET", "/state", None).await.1["root"]
        .as_str()
        .unwrap()
        .to_owned();
    // /anchored: the anchor tx must be on the ledger (502 otherwise) and carry the root (422).
    let (s, _) = call(
        &app,
        "POST",
        "/anchored",
        Some(json!({ "root": root, "anchorTxHash": "ANCH" })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    ledger.put_anchor("ANCH", POOL, &memo_hex, true);
    let (s, v) = call(
        &app,
        "POST",
        "/anchored",
        Some(json!({ "root": root, "anchorTxHash": "ANCH" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["anchoredRoot"], json!(root));
    let (s, _) = call(
        &app,
        "POST",
        "/anchored",
        Some(json!({ "root": "00".repeat(32), "anchorTxHash": "ANCH" })),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, v) = call(
        &app,
        "POST",
        "/audit/reveal",
        Some(json!({ "viewingKey": alice.info().viewing_key })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["balance"], 10);
    let (s, _) = call(
        &app,
        "POST",
        "/audit/reveal",
        Some(json!({ "viewingKey": "zz" })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v) = call(&app, "GET", "/log?from=1", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["records"].as_array().unwrap().len(), 0);
    assert_eq!(v["bundleCount"], 1);
}

#[tokio::test]
async fn node_persists_its_state_and_maps_io_errors() {
    use axum::response::IntoResponse;
    let ledger = FakeLedger::default();
    let dir = std::env::temp_dir().join(format!("confidant-node-test-{}", std::process::id()));
    let path = dir.join("n.verifier-state.json");
    let n = Node::new(Verifier::new(), ledger.clone(), POOL, Some(path.clone()));
    assert_eq!(n.pool(), POOL);
    let alice = Wallet::new();
    let p = alice.build_shield(3).unwrap();
    ledger.put_tx("D", deposit_tx(POOL, "3", &p.hash_hex()));
    let acc = n.submit_shield(&p.record, "D").await.unwrap();
    // The state file is written after every accepted op and restores to the same verifier.
    let text = std::fs::read_to_string(&path).unwrap();
    let snap: confidant_core::VerifierSnapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(snap.records.len(), 1);
    assert_eq!(
        Verifier::restore(&snap).unwrap().state().root,
        n.state().root
    );
    assert_eq!(n.snapshot().records.len(), 1);
    ledger.put_anchor("A", POOL, &acc.memo_hex, true);
    n.anchored(&n.state().root, "A").await.unwrap();
    assert_eq!(
        serde_json::from_str::<confidant_core::VerifierSnapshot>(
            &std::fs::read_to_string(&path).unwrap()
        )
        .unwrap()
        .anchored_root,
        Some(n.state().root)
    );
    std::fs::remove_dir_all(&dir).ok();
    // A state path that cannot be written surfaces as an Io error → 500.
    let bad = Node::new(
        Verifier::new(),
        ledger.clone(),
        POOL,
        Some(std::path::PathBuf::from(
            "/proc/confidant/nope/x.verifier-state.json",
        )),
    );
    let err = bad.submit_transfer(&p.record).unwrap_err();
    // (a shield record as a transfer is refused before persisting; use a replay-free path instead)
    assert!(matches!(err, NodeError::Core(_)));
    let io = http::ApiError(NodeError::Io("disk".into())).into_response();
    assert_eq!(io.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let bad_rpc =
        http::ApiError(NodeError::Ledger(LedgerError::Rpc("down".into()))).into_response();
    assert_eq!(bad_rpc.status(), StatusCode::BAD_GATEWAY);
}

/// The boundary screening gate: listed depositors and destinations are refused by every node on
/// its own list (submit and endorse alike), a deposit younger than the standby is refused until the
/// ledger catches up, and nothing is refused when the gate is off.
#[tokio::test]
async fn screening_gate_refuses_listed_parties_and_fresh_deposits_fail_closed() {
    // Policy parsing: lines with comments, or a JSON array.
    let dir = std::env::temp_dir().join(format!("confidant-screening-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("list.txt"),
        "# demo list\nrAlice  # the depositor\n\nrEve\n",
    )
    .unwrap();
    std::fs::write(dir.join("list.json"), "[\"rEve\", \" rMallory \"]").unwrap();
    let txt = Screening::from_file(&dir.join("list.txt"), 0).unwrap();
    assert!(txt.is_listed("rAlice") && txt.is_listed("rEve") && !txt.is_listed("rBob"));
    assert_eq!(txt.listed_count(), 2);
    let js = Screening::from_file(&dir.join("list.json"), 7).unwrap();
    assert!(js.is_listed("rEve") && js.is_listed("rMallory") && js.standby_ledgers() == 7);
    std::fs::write(dir.join("bad.json"), "[1, 2]").unwrap();
    assert!(Screening::from_file(&dir.join("bad.json"), 0).is_err());
    assert!(Screening::from_file(&dir.join("missing.txt"), 0).is_err());
    std::fs::remove_dir_all(&dir).ok();
    assert!(!Screening::off().is_active());
    assert!(Screening::new(Vec::<String>::new(), 1).is_active());

    // deposit_source reads the sender and the ledger from the tx answer, nothing else.
    let d = deposit_tx(POOL, "100", &"a".repeat(64));
    assert_eq!(deposit_source(&d).unwrap(), ("rAlice".to_owned(), 100));
    let mut no_acc = d.clone();
    no_acc["tx_json"].as_object_mut().unwrap().remove("Account");
    assert!(matches!(
        deposit_source(&no_acc),
        Err(LedgerError::Deposit("no sender account"))
    ));
    let mut no_ledger = d.clone();
    no_ledger.as_object_mut().unwrap().remove("ledger_index");
    assert!(matches!(
        deposit_source(&no_ledger),
        Err(LedgerError::Deposit("no ledger index"))
    ));

    let ledger = FakeLedger::default();
    let open = node(&ledger); // no policy: accepts everything the core accepts
    let strict = Node::new(Verifier::new(), ledger.clone(), POOL, None)
        .with_screening(Screening::new(["rAlice".to_owned(), "rEve".to_owned()], 0));
    assert!(strict.screening().is_active());
    let mut alice = Wallet::new();
    let p = alice.build_shield(100).unwrap();
    ledger.put_tx("DEP1", deposit_tx(POOL, "100", &p.hash_hex()));
    // Listed depositor: refused at submit and at endorse, with the account named; the open node accepts.
    match strict.submit_shield(&p.record, "DEP1").await {
        Err(NodeError::Screened(m)) => assert!(m.contains("rAlice"), "{m}"),
        other => panic!("expected a screening refusal, got {other:?}"),
    }
    let acc = open.submit_shield(&p.record, "DEP1").await.unwrap();
    assert!(matches!(
        strict
            .endorse(
                &Submission::Shield {
                    record: p.record.clone(),
                    deposit: DepositRef {
                        tx_hash: "DEP1".into(),
                        delivered: 100,
                        memo_hash: p.hash_hex()
                    }
                },
                &acc.memo_hex
            )
            .await,
        Err(NodeError::Screened(_))
    ));
    assert_eq!(
        strict.state().bundle_count,
        0,
        "a refused shield leaves the state untouched"
    );

    // Listed destination: refused at submit and at endorse on a node that holds the note.
    alice.sync(&open.log(0)).unwrap();
    let u = alice.build_unshield(40, "rEve").unwrap();
    let lenient = Node::new(Verifier::new(), ledger.clone(), POOL, None)
        .with_screening(Screening::new(["rEve".to_owned()], 0));
    lenient
        .endorse(
            &Submission::Shield {
                record: p.record.clone(),
                deposit: DepositRef {
                    tx_hash: "DEP1".into(),
                    delivered: 0,
                    memo_hash: String::new(),
                },
            },
            &acc.memo_hex,
        )
        .await
        .unwrap();
    assert!(matches!(
        lenient.submit_unshield(&u.record, "rEve"),
        Err(NodeError::Screened(_))
    ));
    let acc_u = open.submit_unshield(&u.record, "rEve").unwrap();
    assert!(matches!(
        lenient
            .endorse(
                &Submission::Unshield {
                    record: u.record.clone(),
                    destination: "rEve".into()
                },
                &acc_u.memo_hex
            )
            .await,
        Err(NodeError::Screened(_))
    ));
    assert_eq!(lenient.state().bundle_count, 1);

    // Standby: the deposit validated in ledger 100; with a 5-ledger standby it is refused until
    // the validated ledger reaches 105, and an unavailable ledger index refuses too (fail closed).
    let mut bob = Wallet::new();
    let p2 = bob.build_shield(50).unwrap();
    ledger.put_tx("DEP2", deposit_tx(POOL, "50", &p2.hash_hex()));
    let waiting = Node::new(Verifier::new(), ledger.clone(), POOL, None)
        .with_screening(Screening::new(Vec::<String>::new(), 5));
    assert!(matches!(
        waiting.submit_shield(&p2.record, "DEP2").await,
        Err(NodeError::Ledger(LedgerError::Rpc(_)))
    ));
    ledger.set_validated(104);
    match waiting.submit_shield(&p2.record, "DEP2").await {
        Err(NodeError::Screened(m)) => assert!(m.contains("standby") && m.contains("105"), "{m}"),
        other => panic!("expected a standby refusal, got {other:?}"),
    }
    ledger.set_validated(105);
    waiting.submit_shield(&p2.record, "DEP2").await.unwrap();
    bob.sync(&waiting.log(0)).unwrap();
    assert_eq!(bob.balance(), 50);

    // HTTP: a screening refusal is 403, distinct from the ledger-facts 422.
    let app = http::router(Arc::new(
        Node::new(Verifier::new(), ledger.clone(), POOL, None)
            .with_screening(Screening::new(["rAlice".to_owned()], 0)),
    ));
    let (s, v) = call(
        &app,
        "POST",
        "/submit/shield",
        Some(json!({ "record": p.record, "depositTxHash": "DEP1" })),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(v["error"].as_str().unwrap().contains("screening"));
    let (s, _) = call(
        &app,
        "POST",
        "/submit/unshield",
        Some(json!({ "record": u.record, "destination": "rAlice" })),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

/// Address canonicalization (vectors generated with xrpl.js `classicAddressToXAddress`): every
/// spelling of a listed party resolves to the one classic address the list is compared in.
#[test]
fn screening_canonicalizes_classic_and_x_addresses() {
    let vectors = [
        (
            "rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpf",
            "XVLhHMPHU98es4dbozjVtdWzVrDjtV5fdx1mHp98tDMoQXb",
            "XVLhHMPHU98es4dbozjVtdWzVrDjtVFpRb8iCFUkMtRyVii",
            "TVE26TYGhfLC7tQDno7G8dGtxSkYQn49b3qD26PK7FcGSKE",
            "TVE26TYGhfLC7tQDno7G8dGtxSkYQnXoy6kSDh6rZzApc69",
        ),
        (
            "rnXyVQzgxZe7TR1EPzTkGj2jxH4LMJYh66",
            "X75KjMo4pw9UQ9H7hbpCZUDfskq2PL3MEZS3AoCF1J86KTL",
            "X75KjMo4pw9UQ9H7hbpCZUDfskq2PLNGsUMNLb58LzRZpGB",
            "T7di9TxLtf6DqChnLPmSTByrXU44tu8PUjPbE7uMTvc3fM8",
            "T7di9TxLtf6DqChnLPmSTByrXU44tuxsbJJo1iDsZ39RgWa",
        ),
        (
            "rrrrrrrrrrrrrrrrrrrrrhoLvTp",
            "X7TYFRtYHMcHtT2qNycMwgXzFbcRvEgLY6WDzQKYkjCp8GS",
            "X7TYFRtYHMcHtT2qNycMwgXzFbcRvE8ELLRKPU4WN4GdcBY",
            "T7PnNYsX9KwKKyMSW8M3hgHuJs9NRqL4V1TgJnZnMNYFgg8",
            "T7PnNYsX9KwKKyMSW8M3hgHuJs9NRqWptS4tvSimSStGN7n",
        ),
    ];
    for (classic, x_main, x_main_tag, x_test, x_test_tag) in vectors {
        assert_eq!(
            canonical(classic).as_deref(),
            Some(classic),
            "classic round-trips"
        );
        for other in [x_main, x_main_tag, x_test, x_test_tag] {
            assert_eq!(
                canonical(other).as_deref(),
                Some(classic),
                "{other} → {classic}"
            );
        }
        assert_eq!(
            canonical(format!("  {classic}  ").trim()).as_deref(),
            Some(classic)
        );
    }
    // Malformed spellings never canonicalize: bad checksum, wrong length, foreign alphabet, empty.
    assert_eq!(canonical("rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpg"), None);
    assert_eq!(canonical("rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QY"), None);
    assert_eq!(canonical("0GWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpf"), None);
    assert_eq!(canonical(""), None);
    assert_eq!(canonical("rrr"), None);
    // A valid checksum is not enough: an X-shaped payload with a foreign prefix or an unknown
    // tag flag is refused, while the same AccountID under the mainnet prefix canonicalizes.
    assert_eq!(
        canonical("XVLhHMPHU98es4dbozjVtdWzVrDjtV5fdx1mHp98tDMoQXc"),
        None
    );
    assert_eq!(
        canonical("XWnVd9AKTnhnt1GMCVYyuheajMsFPonVqYz5qHtcDZSFYQU"),
        None,
        "foreign prefix 0x0545"
    );
    assert_eq!(
        canonical("X7mSinhEWd7qFaYLZpcFyck2NrXqyhWSxs59wH7whfkyYbx"),
        None,
        "tag flag 2"
    );
    assert_eq!(
        canonical("X7mSinhEWd7qFaYLZpcFyck2NrXqyhL8Z78uoaeGQG7EVWe").as_deref(),
        Some("raVZNVnDrTHaYvifmAFuKcvdkKAAnrnBxL")
    );

    // The gate: a classic entry catches every X spelling, an X entry catches the classic one.
    let listed_classic = Screening::new(["rnXyVQzgxZe7TR1EPzTkGj2jxH4LMJYh66".to_owned()], 0);
    for spelling in [
        "rnXyVQzgxZe7TR1EPzTkGj2jxH4LMJYh66",
        "X75KjMo4pw9UQ9H7hbpCZUDfskq2PL3MEZS3AoCF1J86KTL",
        "X75KjMo4pw9UQ9H7hbpCZUDfskq2PLNGsUMNLb58LzRZpGB",
        "T7di9TxLtf6DqChnLPmSTByrXU44tu8PUjPbE7uMTvc3fM8",
        "T7di9TxLtf6DqChnLPmSTByrXU44tuxsbJJo1iDsZ39RgWa",
    ] {
        assert!(
            listed_classic.is_listed(spelling),
            "{spelling} must be caught"
        );
        assert!(listed_classic.check_destination(spelling).is_err());
        assert!(listed_classic.check_deposit(spelling, 1, 1).is_err());
    }
    assert!(!listed_classic.is_listed("rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpf"));
    let listed_x = Screening::new(
        ["T7di9TxLtf6DqChnLPmSTByrXU44tuxsbJJo1iDsZ39RgWa".to_owned()],
        0,
    );
    assert!(listed_x.is_listed("rnXyVQzgxZe7TR1EPzTkGj2jxH4LMJYh66"));
    assert!(listed_x.is_listed("X75KjMo4pw9UQ9H7hbpCZUDfskq2PL3MEZS3AoCF1J86KTL"));
    // A list file mixing spellings canonicalizes on load; an undecodable entry only matches itself.
    let dir = std::env::temp_dir().join(format!("confidant-canon-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("mixed.txt"),
        "X75KjMo4pw9UQ9H7hbpCZUDfskq2PL3MEZS3AoCF1J86KTL # eve as X\nnot-an-address\n",
    )
    .unwrap();
    let mixed = Screening::from_file(&dir.join("mixed.txt"), 0).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(mixed.listed_count(), 2);
    assert_eq!(
        mixed.undecodable_count(),
        1,
        "the inert entry is counted so startup can warn"
    );
    assert!(mixed.is_listed("rnXyVQzgxZe7TR1EPzTkGj2jxH4LMJYh66"));
    assert!(mixed.is_listed("not-an-address") && !mixed.is_listed("not-an-addres"));
    assert_eq!(listed_classic.undecodable_count(), 0);
}

/// Protocol fees (Railgun's model): a share of every shield and unshield accrues in the pool
/// account on every node's ledger, netted off the note and the payout; a treasury sweep is
/// recorded only after the node read it from the ledger and only up to what accrued; everything
/// replays from the ledger + mirror.
#[tokio::test]
async fn protocol_fees_accrue_net_and_sweep_verified_on_ledger() {
    const TREASURY: &str = "rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpf";
    // Policy sanity.
    assert!(
        FeePolicy::new(25, 25, None).is_err(),
        "a fee needs a treasury"
    );
    assert!(
        FeePolicy::new(2_000, 0, Some(TREASURY)).is_err(),
        "above the 10% cap"
    );
    assert!(FeePolicy::new(10, 10, Some("not-an-address")).is_err());
    assert!(!FeePolicy::new(0, 0, None).unwrap().is_active());
    let policy = FeePolicy::new(
        25,
        25,
        Some("XVLhHMPHU98es4dbozjVtdWzVrDjtV5fdx1mHp98tDMoQXb"),
    )
    .unwrap();
    assert_eq!(
        policy.treasury.as_deref(),
        Some(TREASURY),
        "treasury canonicalized"
    );
    assert_eq!(policy.shield_fee(100_000_000), 250_000);
    assert_eq!(policy.unshield_fee(40_000_000), 100_000);
    assert_eq!(policy.shield_fee(39), 0, "rounds down");

    let ledger = FakeLedger::default();
    // The pool account's balance as the ledger shows it: the independent bound on sweeps (below).
    let balance =
        |drops: u64| json!({ "account_data": { "Account": POOL, "Balance": drops.to_string() } });
    ledger.set_info(balance(1_000_000_000_000));
    let dir = std::env::temp_dir().join(format!("confidant-fees-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let state = dir.join("n.verifier-state.json");
    let coord = Node::new(Verifier::new(), ledger.clone(), POOL, Some(state.clone()))
        .with_fees(policy.clone(), FeeLedger::default());
    let signer = Node::new(Verifier::new(), ledger.clone(), POOL, None)
        .with_fees(policy.clone(), FeeLedger::default());
    let mut alice = Wallet::new();
    // The wallet builds the NET note: a 100 XRP deposit at 25 bps credits 99.75 XRP.
    let gross = 100_000_000u64;
    let wrong = alice.build_shield(gross).unwrap();
    ledger.put_tx("DEP", deposit_tx(POOL, "100000000", &wrong.hash_hex()));
    assert!(
        matches!(
            coord.submit_shield(&wrong.record, "DEP").await,
            Err(NodeError::Core(CoreError::ValueBalance { .. }))
        ),
        "a note for the gross amount is refused: the fee is not optional"
    );
    let p = alice.build_shield(gross - 250_000).unwrap();
    ledger.put_tx("DEP", deposit_tx(POOL, "100000000", &p.hash_hex()));
    let acc = coord.submit_shield(&p.record, "DEP").await.unwrap();
    assert_eq!(acc.fee, 250_000);
    assert_eq!(coord.fees().await.accrued, 250_000);
    // The signer nets the same fee from the ledger facts it reads itself; a retry does not re-accrue.
    let sub = Submission::Shield {
        record: p.record.clone(),
        deposit: DepositRef {
            tx_hash: "DEP".into(),
            delivered: 1,
            memo_hash: String::new(),
        },
    };
    let e = signer.endorse(&sub, &acc.memo_hex).await.unwrap();
    assert_eq!((e.fee, signer.fees().await.accrued), (250_000, 250_000));
    let again = signer.endorse(&sub, &acc.memo_hex).await.unwrap();
    assert_eq!((again.fee, signer.fees().await.accrued), (250_000, 250_000));
    assert_eq!(signer.state().root, coord.state().root);

    // Unshield 40 XRP: payout is net of 0.1 XRP on both nodes; the fee accrues once each.
    alice.sync(&coord.log(0)).unwrap();
    let u = alice.build_unshield(40_000_000, "rOut").unwrap();
    let acc_u = coord.submit_unshield(&u.record, "rOut").unwrap();
    assert_eq!(acc_u.fee, 100_000);
    assert_eq!(acc_u.accepted.payout.as_ref().unwrap().amount, 39_900_000);
    assert_eq!(coord.fees().await.accrued, 350_000);
    let usub = Submission::Unshield {
        record: u.record.clone(),
        destination: "rOut".into(),
    };
    let e = signer.endorse(&usub, &acc_u.memo_hex).await.unwrap();
    assert_eq!(
        (e.fee, e.accepted.payout.unwrap().amount),
        (100_000, 39_900_000)
    );
    let again = signer.endorse(&usub, &acc_u.memo_hex).await.unwrap();
    assert_eq!(
        again.accepted.payout.unwrap().amount,
        39_900_000,
        "the idempotent answer is net too"
    );
    assert_eq!(signer.fees().await.accrued, 350_000);
    // Transfers carry no fee.
    let mut bob = Wallet::new();
    alice.sync(&coord.log(0)).unwrap();
    let t = alice
        .build_transfer(&[confidant_core::TransferOutput {
            recipient: bob.info().address,
            amount: 1_000_000,
        }])
        .unwrap();
    let acc_t = coord.submit_transfer(&t.record).unwrap();
    assert_eq!(acc_t.fee, 0);
    assert_eq!(coord.fees().await.accrued, 350_000);
    bob.sync(&coord.log(0)).unwrap();

    // The ledger file lives next to the state file.
    let fee_file = Node::<FakeLedger>::fee_ledger_path(&state);
    let on_disk: FeeLedger =
        serde_json::from_str(&std::fs::read_to_string(&fee_file).unwrap()).unwrap();
    assert_eq!((on_disk.accrued, on_disk.swept), (350_000, 0));

    // A sweep: recorded only after the node reads a validated pool → treasury Payment, once per
    // hash, never above what is available; wrong destination / sender / unknown tx refused.
    let sweep = |hash: &str, to: &str, drops: &str| {
        ledger.put_tx(
            hash,
            json!({ "validated": true, "hash": hash, "ledger_index": 200,
            "tx_json": { "TransactionType": "Payment", "Account": POOL, "Destination": to },
            "meta": { "delivered_amount": drops } }),
        );
    };
    sweep("SW1", TREASURY, "300000");
    let r = coord.fee_swept("SW1").await.unwrap();
    assert_eq!((r.swept, r.available), (300_000, 50_000));
    let r = coord.fee_swept("SW1").await.unwrap();
    assert_eq!(r.swept, 300_000, "counted once");
    sweep("SW2", TREASURY, "60000");
    assert!(
        matches!(coord.fee_swept("SW2").await, Err(NodeError::Fee(_))),
        "exceeds available"
    );
    sweep("SW3", "rEve", "1");
    assert!(matches!(
        coord.fee_swept("SW3").await,
        Err(NodeError::Ledger(LedgerError::Sweep(
            "destination is not the treasury"
        )))
    ));
    ledger.put_tx("SW4", json!({ "validated": true, "tx_json": { "TransactionType": "Payment", "Account": "rEve", "Destination": TREASURY }, "meta": { "delivered_amount": "1" } }));
    assert!(matches!(
        coord.fee_swept("SW4").await,
        Err(NodeError::Ledger(LedgerError::Sweep(
            "not sent by the pool account"
        )))
    ));
    ledger.put_tx("SW5", json!({ "validated": false, "tx_json": { "TransactionType": "Payment", "Account": POOL, "Destination": TREASURY }, "meta": { "delivered_amount": "1" } }));
    assert!(matches!(
        coord.fee_swept("SW5").await,
        Err(NodeError::Ledger(LedgerError::Sweep("not validated")))
    ));
    ledger.put_tx("SW6", json!({ "validated": true, "tx_json": { "TransactionType": "OfferCreate", "Account": POOL, "Destination": TREASURY } }));
    assert!(matches!(
        coord.fee_swept("SW6").await,
        Err(NodeError::Ledger(LedgerError::Sweep("not a Payment")))
    ));
    ledger.put_tx("SW7", json!({ "validated": true, "tx_json": { "TransactionType": "Payment", "Account": POOL, "Destination": TREASURY }, "meta": { "delivered_amount": { "currency": "USD", "value": "1" } } }));
    assert!(matches!(
        coord.fee_swept("SW7").await,
        Err(NodeError::Ledger(LedgerError::Sweep(_)))
    ));
    assert!(matches!(
        coord.fee_swept("NOPE").await,
        Err(NodeError::Ledger(LedgerError::Rpc(_)))
    ));
    let no_treasury = Node::new(Verifier::new(), ledger.clone(), POOL, None);
    assert!(matches!(
        no_treasury.fee_swept("SW1").await,
        Err(NodeError::Fee(_))
    ));
    assert_eq!(coord.fees().await.available, 50_000);

    // The belt independent of the fee ledger (auditor, 2026-10-13): what may be swept is also
    // bounded by the pool account's excess over the notes it backs, read from the ledger; after a
    // sweep the pool must still cover every note, or the sweep is not recorded.
    assert_eq!(coord.outstanding_backing(), 59_750_000, "99.75 in − 40 out");
    ledger.set_info(balance(59_750_000 + 20_000));
    let r = coord.fees().await;
    assert_eq!(
        (r.pool_excess, r.available),
        (Some(20_000), 20_000),
        "excess binds below the fee ledger's 50_000"
    );
    // The pool total the UI shows: the same backing figure, reported in the clear (it is public by
    // construction) and under its camelCase wire name.
    assert_eq!(
        r.outstanding_backing, 59_750_000,
        "GET /fees reports the backing"
    );
    assert_eq!(
        serde_json::to_value(&r).unwrap()["outstandingBacking"],
        json!(59_750_000),
        "wire name"
    );
    sweep("SW9", TREASURY, "30000");
    ledger.set_info(balance(59_750_000 - 1));
    assert!(
        matches!(coord.fee_swept("SW9").await, Err(NodeError::Fee(_))),
        "pool no longer covers outstanding notes"
    );
    assert_eq!(coord.fees().await.swept, 300_000, "not recorded");
    ledger.set_info(Value::Null);
    let r = coord.fees().await;
    assert_eq!(
        (r.pool_excess, r.available),
        (None, 0),
        "ledger unavailable: nothing sweepable"
    );
    assert!(matches!(
        coord.fee_swept("SW9").await,
        Err(NodeError::Ledger(LedgerError::Rpc(_)))
    ));
    ledger.set_info(custody(&["rS1"], 1, true));
    assert_eq!(coord.fees().await.pool_excess, None, "no Balance field");
    ledger.set_info(balance(1_000_000_000_000));
    assert_eq!(coord.fees().await.available, 50_000);

    // Replay: a new node rebuilds the same accrual from the memos + deposit facts and the same
    // sweeps from the pool account's Payments to the treasury (the sweep itself is on the ledger).
    ledger.created();
    for memo in [&acc.memo_hex, &acc_u.memo_hex, &acc_t.memo_hex] {
        ledger.push_anchor(memo);
    }
    ledger.account.lock().unwrap().push(json!({ "validated": true, "hash": "SW1",
        "tx_json": { "TransactionType": "Payment", "Account": POOL, "Destination": TREASURY }, "meta": { "delivered_amount": "300000" } }));
    ledger.account.lock().unwrap().push(json!({ "validated": true, "hash": "SW1",
        "tx_json": { "TransactionType": "Payment", "Account": POOL, "Destination": TREASURY }, "meta": { "delivered_amount": "300000" } }));
    let (rebuilt, fees) =
        Node::<FakeLedger>::rebuild_from_ledger(&ledger, POOL, &coord.log(0), &policy)
            .await
            .unwrap();
    assert_eq!(rebuilt.state().root, coord.state().root);
    assert_eq!(
        (fees.accrued, fees.swept, fees.available()),
        (350_000, 300_000, 50_000),
        "a duplicate listing counts once"
    );
    // Without the fee policy the same ledger does not rebuild: the notes were credited net.
    assert!(matches!(
        Node::<FakeLedger>::rebuild_from_ledger(&ledger, POOL, &coord.log(0), &FeePolicy::off())
            .await,
        Err(NodeError::Core(CoreError::ValueBalance { .. }))
    ));

    // HTTP: GET /fees, POST /fees/swept (409 on excess, 422 on a non-sweep tx).
    let app = http::router(Arc::new(
        Node::new(Verifier::new(), ledger.clone(), POOL, None).with_fees(
            policy.clone(),
            FeeLedger {
                accrued: 1_000,
                swept: 0,
                sweeps: Default::default(),
            },
        ),
    ));
    let (s, v) = call(&app, "GET", "/fees", None).await;
    assert_eq!(
        (s, v["available"].as_u64(), v["shieldBps"].as_u64()),
        (StatusCode::OK, Some(1_000), Some(25))
    );
    let (s, _) = call(
        &app,
        "POST",
        "/fees/swept",
        Some(json!({ "txHash": "SW1" })),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = call(
        &app,
        "POST",
        "/fees/swept",
        Some(json!({ "txHash": "SW3" })),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    sweep("SW8", TREASURY, "1000");
    let (s, v) = call(
        &app,
        "POST",
        "/fees/swept",
        Some(json!({ "txHash": "SW8" })),
    )
    .await;
    assert_eq!((s, v["available"].as_u64()), (StatusCode::OK, Some(0)));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn credential_gate_refuses_deposits_without_an_accepted_credential_at_submit_and_endorse() {
    const ISSUER: &str = "rwbgyYBhFiBExQ7K5yJKyYdvHYwGHdBamt";
    const TYPE: &str = "436F6E666964616E744B5943"; // "ConfidantKYC"

    // Policy parsing: the issuer must be an address, the type 1–64 bytes of hex; text is derived.
    assert!(!CredentialGate::off().is_active());
    assert!(matches!(
        CredentialGate::new("not-an-address", TYPE, None),
        Err(CredentialError::Policy(_))
    ));
    for bad in ["", "ABC", "zz", &"AB".repeat(65)] {
        assert!(
            matches!(
                CredentialGate::new(ISSUER, bad, None),
                Err(CredentialError::Policy(_))
            ),
            "{bad}"
        );
    }
    let gate = CredentialGate::new(ISSUER, "436f6e666964616e744b5943", None).unwrap();
    let p = gate.policy().unwrap();
    assert_eq!(
        (
            p.issuer.as_str(),
            p.credential_type.as_str(),
            p.type_text.as_deref()
        ),
        (ISSUER, TYPE, Some("ConfidantKYC"))
    );
    assert_eq!(
        CredentialGate::new(ISSUER, "00FF", None)
            .unwrap()
            .policy()
            .unwrap()
            .type_text,
        None
    );
    assert_eq!(
        CredentialGate::new(ISSUER, TYPE, Some("KYC"))
            .unwrap()
            .policy()
            .unwrap()
            .type_text
            .as_deref(),
        Some("KYC")
    );

    // check(): every refusal names issuer, type and account; the gate off passes anything.
    let entry = |accepted: bool, exp: Option<u64>| {
        let mut e = json!({ "Subject": "rAlice", "Issuer": ISSUER, "CredentialType": TYPE, "Flags": if accepted { LSF_ACCEPTED } else { 0 } });
        if let Some(x) = exp {
            e["Expiration"] = json!(x);
        }
        e
    };
    let why = |r: Result<(), CredentialError>| match r {
        Err(CredentialError::Missing {
            issuer,
            type_text,
            account,
            why,
        }) => {
            assert_eq!(
                (issuer.as_str(), type_text.as_str(), account.as_str()),
                (ISSUER, "ConfidantKYC", "rAlice")
            );
            why
        }
        other => panic!("expected a refusal, got {other:?}"),
    };
    assert_eq!(why(gate.check("rAlice", None, 1_000)), "none on the ledger");
    assert_eq!(
        why(gate.check("rAlice", Some(&entry(false, None)), 1_000)),
        "issued but not accepted by the subject"
    );
    assert_eq!(
        why(gate.check("rAlice", Some(&entry(true, Some(1_000))), 1_000)),
        "expired"
    );
    assert_eq!(
        why(gate.check("rAlice", Some(&entry(true, Some(999))), 1_000)),
        "expired"
    );
    for (k, v) in [
        ("Issuer", "rSomeoneElse"),
        ("Subject", "rBob"),
        ("CredentialType", "00FF"),
    ] {
        let mut other = entry(true, None);
        other[k] = json!(v);
        assert_eq!(
            why(gate.check("rAlice", Some(&other), 1_000)),
            "the ledger answered for another credential",
            "{k}"
        );
    }
    assert!(gate
        .check("rAlice", Some(&entry(true, None)), 1_000)
        .is_ok());
    assert!(gate
        .check("rAlice", Some(&entry(true, Some(1_001))), 1_000)
        .is_ok());
    let mut lower = entry(true, None);
    lower["CredentialType"] = json!(TYPE.to_lowercase());
    assert!(
        gate.check("rAlice", Some(&lower), 1_000).is_ok(),
        "type hex case does not matter"
    );
    assert!(CredentialGate::off().check("rAlice", None, 1_000).is_ok());
    assert!(ripple_now() > 800_000_000);
    // The live ledger's "no such entry" answer, in both shapes rippled's error can reach the node in,
    // is an ordinary "none" — never a lookup failure (that would be a 502 instead of the 403).
    for m in [
        "entryNotFound",
        "ledger rpc: Entry not found.",
        "Entry not found",
        "ENTRY NOT FOUND.",
    ] {
        assert!(is_entry_not_found(m), "{m}");
    }
    for m in [
        "txnNotFound",
        "ledger rpc: connection refused",
        "actNotFound rX",
        "",
    ] {
        assert!(!is_entry_not_found(m), "{m}");
    }
    // A policy error reads as one; a refusal carries the contract's wording.
    assert!(CredentialError::Policy("x".into())
        .to_string()
        .starts_with("credential policy:"));
    assert!(gate
        .check("rAlice", None, 1)
        .unwrap_err()
        .to_string()
        .starts_with(&format!(
            "no accepted credential from {ISSUER} of type ConfidantKYC for rAlice"
        )));

    // The node: gated vs open, at submit and at endorse, reading the ledger each time.
    let ledger = FakeLedger::default();
    let open = node(&ledger);
    let gated =
        Node::new(Verifier::new(), ledger.clone(), POOL, None).with_credential(gate.clone());
    assert_eq!(
        gated.credential_policy().map(|p| p.issuer.as_str()),
        Some(ISSUER)
    );
    assert!(open.credential_policy().is_none());
    let alice = Wallet::new();
    let p1 = alice.build_shield(100).unwrap();
    ledger.put_tx("DEP1", deposit_tx(POOL, "100", &p1.hash_hex()));
    match gated.submit_shield(&p1.record, "DEP1").await {
        Err(NodeError::Credential(m)) => assert!(
            m.contains("no accepted credential")
                && m.contains(ISSUER)
                && m.contains("rAlice")
                && m.contains("none on the ledger"),
            "{m}"
        ),
        other => panic!("expected a credential refusal, got {other:?}"),
    }
    ledger.put_credential("rAlice", ISSUER, TYPE, false, None); // issued, not accepted
    assert!(
        matches!(gated.submit_shield(&p1.record, "DEP1").await, Err(NodeError::Credential(m)) if m.contains("not accepted"))
    );
    ledger.put_credential("rAlice", ISSUER, TYPE, true, Some(1)); // accepted long ago, expired
    assert!(
        matches!(gated.submit_shield(&p1.record, "DEP1").await, Err(NodeError::Credential(m)) if m.contains("expired"))
    );
    // The open node never asks the ledger for credentials.
    let before = ledger.credential_calls();
    let acc = open.submit_shield(&p1.record, "DEP1").await.unwrap();
    assert_eq!(ledger.credential_calls(), before);
    // Endorse runs the same gate: refused while expired, accepted once the credential is good.
    let sub = Submission::Shield {
        record: p1.record.clone(),
        deposit: DepositRef {
            tx_hash: "DEP1".into(),
            delivered: 100,
            memo_hash: p1.hash_hex(),
        },
    };
    assert!(matches!(
        gated.endorse(&sub, &acc.memo_hex).await,
        Err(NodeError::Credential(_))
    ));
    ledger.put_credential("rAlice", ISSUER, TYPE, true, Some(ripple_now() + 3_600));
    gated.endorse(&sub, &acc.memo_hex).await.unwrap();
    let bob = Wallet::new();
    let p2 = bob.build_shield(50).unwrap();
    ledger.put_tx("DEP2", deposit_tx(POOL, "50", &p2.hash_hex()));
    gated.submit_shield(&p2.record, "DEP2").await.unwrap();
    // The ledger unreachable for the lookup: fail closed with a ledger error, never an acceptance.
    ledger.fail_credentials(true);
    let carol = Wallet::new();
    let p3 = carol.build_shield(70).unwrap();
    ledger.put_tx("DEP3", deposit_tx(POOL, "70", &p3.hash_hex()));
    assert!(matches!(
        gated.submit_shield(&p3.record, "DEP3").await,
        Err(NodeError::Ledger(LedgerError::Rpc(_)))
    ));
    ledger.fail_credentials(false);

    // HTTP: /policy reports the gate; a refusal is 403 and names the credential.
    let app = http::router(Arc::new(
        Node::new(Verifier::new(), ledger.clone(), POOL, None).with_credential(gate.clone()),
    ));
    let (s, v) = call(&app, "GET", "/policy", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        (
            v["credential"]["issuer"].as_str(),
            v["credential"]["type"].as_str(),
            v["credential"]["typeText"].as_str()
        ),
        (Some(ISSUER), Some(TYPE), Some("ConfidantKYC"))
    );
    assert_eq!(
        (
            v["screening"]["active"].as_bool(),
            v["screening"]["standbyLedgers"].as_u64()
        ),
        (Some(false), Some(0))
    );
    ledger.clear_credentials();
    let dave = Wallet::new();
    let p4 = dave.build_shield(30).unwrap();
    ledger.put_tx("DEP4", deposit_tx(POOL, "30", &p4.hash_hex()));
    let (s, v) = call(
        &app,
        "POST",
        "/submit/shield",
        Some(json!({ "record": p4.record, "depositTxHash": "DEP4" })),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(v["error"].as_str().unwrap().contains("credential"), "{v}");
    let open_app = http::router(Arc::new(node(&ledger)));
    let (s, v) = call(&open_app, "GET", "/policy", None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["credential"].is_null());
}

#[tokio::test]
async fn proof_of_funds_endpoint_is_read_only_bounded_and_judges_the_challenge() {
    use confidant_node::{
        PofChallenge, CHALLENGE_PREFIX, MAX_EXPIRY_WINDOW, MAX_LENDER_LEN, MEANING, POF_PERMITS,
        RECENT_WINDOW,
    };
    let ledger = FakeLedger::default();
    let n = Arc::new(node(&ledger));
    let app = http::router(n.clone());
    let mut alice = Wallet::new();
    for (i, amount) in [100u64, 50].iter().enumerate() {
        let p = alice.build_shield(*amount).unwrap();
        let tx = format!("D{i}");
        ledger.put_tx(&tx, deposit_tx(POOL, &amount.to_string(), &p.hash_hex()));
        n.submit_shield(&p.record, &tx).await.unwrap();
    }
    alice.sync(&n.log(0)).unwrap();
    let snap = |n: &Node<FakeLedger>| {
        let s = n.state();
        (
            s.root,
            s.bundle_count,
            s.nullifier_count,
            s.leaf_count,
            s.consumed_deposits,
        )
    };
    let before = snap(&n);

    const BORROWER: &str = "rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpf";
    let ch = PofChallenge {
        borrower: BORROWER.into(),
        lender: "lender-1".into(),
        ledger_index: 990,
        expiry: 1500,
    };
    // The canonical bytes are pinned: the wallet builds exactly this string.
    assert_eq!(
        ch.bytes(120),
        format!("{CHALLENGE_PREFIX}|{BORROWER}|120|990|1500|lender-1").into_bytes()
    );
    let pof = alice.build_proof_of_funds(120, &ch.bytes(120)).unwrap();
    ledger.set_validated(1_000);

    // Happy path, node level: the verdict names what it was judged against; nothing changed.
    let v = n
        .verify_proof_of_funds(&pof.record, 120, &ch)
        .await
        .unwrap();
    assert!(v.ok);
    assert_eq!(v.amount, 120);
    assert_eq!(v.challenge, ch);
    assert_eq!(v.validated_ledger, 1_000);
    assert_eq!(v.root, n.state().root);
    assert_eq!(
        v.bundle_hash,
        hex::encode(bundle_hash(&pof.record).unwrap())
    );
    assert_eq!(v.meaning, MEANING);
    assert!(v.meaning.contains("not a lock"));
    assert_eq!(snap(&n), before, "verification mutates nothing");
    assert_eq!(n.log(0).len(), 2, "nothing is logged");

    // Through HTTP.
    let body = |amount: u64, ch: &PofChallenge| json!({ "record": pof.record, "amount": amount, "challenge": ch });
    let (s, v) = call(&app, "POST", "/pof/verify", Some(body(120, &ch))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["ok"], true);
    assert_eq!(v["validatedLedger"], 1_000);
    assert_eq!(v["challenge"]["ledgerIndex"], 990);
    assert_eq!(v["root"], n.state().root);
    assert!(v["meaning"].as_str().unwrap().contains("snapshot"));

    // The proof is bound: another amount or a challenge with one field changed is a core refusal.
    let (s, v) = call(&app, "POST", "/pof/verify", Some(body(100, &ch))).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let mut other = ch.clone();
    other.lender = "lender-2".into();
    let (s, v) = call(&app, "POST", "/pof/verify", Some(body(120, &other))).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(matches!(
        n.verify_proof_of_funds(&pof.record, 120, &other).await,
        Err(NodeError::Core(CoreError::InvalidSpendAuth))
    ));
    // A pool record is not a proof of funds, and a proof of funds is not a pool operation.
    let shield = n.log(0)[0].clone();
    assert!(matches!(
        n.verify_proof_of_funds(&shield, 100, &ch).await,
        Err(NodeError::Core(CoreError::NotAProofOfFunds))
    ));
    assert!(matches!(
        n.submit_transfer(&pof.record),
        Err(NodeError::Core(CoreError::NotAPoolOperation))
    ));

    // Challenge fields this node will not judge (400, before any ledger read or verification).
    let refused = |why: &'static str, f: &dyn Fn(&mut PofChallenge)| {
        let mut c = ch.clone();
        f(&mut c);
        let app = app.clone();
        let body = body(120, &c);
        async move {
            let (s, v) = call(&app, "POST", "/pof/verify", Some(body)).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{why}: {v}");
            let e = v["error"].as_str().unwrap();
            assert!(e.contains(why), "{why}: {e}");
        }
    };
    refused("not an XRPL address", &|c| c.borrower = "nobody".into()).await;
    refused("classic address", &|c| {
        c.borrower = "XVLhHMPHU98es4dbozjVtdWzVrDjtV5fdx1mHp98tDMoQXb".into()
    })
    .await;
    refused("lender id", &|c| c.lender = String::new()).await;
    refused("lender id", &|c| c.lender = "x".repeat(MAX_LENDER_LEN + 1)).await;
    refused("lender id", &|c| c.lender = "a|b".into()).await;
    refused("lender id", &|c| c.lender = "a\nb".into()).await;
    refused("after the ledger index", &|c| c.expiry = c.ledger_index).await;
    refused("window too long", &|c| {
        c.expiry = c.ledger_index + MAX_EXPIRY_WINDOW + 1
    })
    .await;
    // Freshness against the validated ledger this node read itself.
    refused("ahead of the validated ledger", &|c| c.ledger_index = 1_001).await;
    refused("not recent", &|c| {
        c.ledger_index = 1_000 - RECENT_WINDOW - 1
    })
    .await;
    refused("expired", &|c| {
        c.ledger_index = 999;
        c.expiry = 1_000
    })
    .await;
    // Exactly at the edges the challenge is still judged.
    let mut edge = ch.clone();
    edge.ledger_index = 1_000 - RECENT_WINDOW;
    edge.expiry = 1_001;
    let edge_pof = alice.build_proof_of_funds(120, &edge.bytes(120)).unwrap();
    n.verify_proof_of_funds(&edge_pof.record, 120, &edge)
        .await
        .unwrap();
    let mut edge = ch.clone();
    edge.expiry = edge.ledger_index + MAX_EXPIRY_WINDOW;
    assert!(edge.validate().is_ok());

    // Lender id length is counted in characters, not bytes.
    let mut wide = ch.clone();
    wide.lender = "ё".repeat(MAX_LENDER_LEN);
    assert!(wide.validate().is_ok());

    // No validated ledger ⇒ 502, nothing judged.
    ledger.set_validated(0);
    let (s, v) = call(&app, "POST", "/pof/verify", Some(body(120, &ch))).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{v}");
    ledger.set_validated(1_000);

    // Bounded: with every permit held the node answers 429 and verifies nothing.
    let held = n
        .pof_permits()
        .try_acquire_many(POF_PERMITS as u32)
        .unwrap();
    let (s, v) = call(&app, "POST", "/pof/verify", Some(body(120, &ch))).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{v}");
    assert!(v["error"].as_str().unwrap().contains("busy"));
    drop(held);
    let (s, v) = call(&app, "POST", "/pof/verify", Some(body(120, &ch))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(n.pof_permits().available_permits(), POF_PERMITS);

    // The body is capped: an oversized request is refused before it is parsed.
    let huge = json!({ "record": "x".repeat(http::POF_BODY_LIMIT), "amount": 1, "challenge": ch });
    let (s, _) = call(&app, "POST", "/pof/verify", Some(huge)).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    // ...while the other routes keep axum's default limit (a verdict request is ≈ 19 KB).
    assert!(serde_json::to_vec(&pof.record).unwrap().len() < http::POF_BODY_LIMIT);

    assert_eq!(snap(&n), before, "the whole exercise mutated nothing");
    assert_eq!(n.log(0).len(), 2);
}
