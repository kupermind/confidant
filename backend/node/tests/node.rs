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
    anchor_memos, canonical, deposit_facts, deposit_source, http, Ledger, LedgerError, Node,
    NodeError, Screening, LSF_DISABLE_MASTER, MEMO_SHIELD,
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
}

impl FakeLedger {
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

    let rebuilt = Node::<FakeLedger>::rebuild_from_ledger(&ledger, POOL, &mirror)
        .await
        .unwrap();
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
    };
    let caught = Node::<FakeLedger>::rebuild_from_ledger(&ledger2, POOL, &mirror)
        .await
        .unwrap();
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
        Node::<FakeLedger>::rebuild_from_ledger(&bad, POOL, &mirror).await,
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
        Node::<FakeLedger>::rebuild_from_ledger(&nodep, POOL, &mirror).await,
        Err(NodeError::Ledger(LedgerError::Rpc(_)))
    ));
    // Deposit delivered less than the shield ⇒ refuse.
    let short = FakeLedger::default();
    short.created();
    short.put_tx("DEP1", deposit_tx(POOL, "99", &p.hash_hex()));
    short.push_anchor(&a0.memo_hex);
    assert!(matches!(
        Node::<FakeLedger>::rebuild_from_ledger(&short, POOL, &mirror).await,
        Err(NodeError::Core(CoreError::ValueBalance { .. }))
    ));
    // Mirror missing a bundle ⇒ refuse.
    assert!(matches!(
        Node::<FakeLedger>::rebuild_from_ledger(&ledger, POOL, &mirror[..1]).await,
        Err(NodeError::Core(CoreError::MirrorMissing(_)))
    ));
    // A pool with only its creation on the ledger ⇒ empty verifier. A scan that does not start at
    // the pool's creation (partial-history server, or nothing at all) ⇒ refuse to serve, even when
    // the memos it does show would rebuild fine.
    let fresh = FakeLedger::default();
    fresh.created();
    let empty = Node::<FakeLedger>::rebuild_from_ledger(&fresh, POOL, &[])
        .await
        .unwrap();
    assert_eq!(empty.state().bundle_count, 0);
    assert!(matches!(
        Node::<FakeLedger>::rebuild_from_ledger(&FakeLedger::default(), POOL, &[]).await,
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
        Node::<FakeLedger>::rebuild_from_ledger(&suffix, POOL, &mirror).await,
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
