//! The negative suite: v1's guards (design REQ §8, IMPL RESP Q-POOL-6) carried over to the
//! Wallet/Verifier split, plus the v2 guards (deposit binding, consumed deposits, anchor memos,
//! rebuild from memos + mirror). Each test names the guard it exercises.

mod common;

use common::*;
use confidant_core::memo::AnchorMemo;
use confidant_core::orchard::builder::{Builder, BundleType};
use confidant_core::orchard::bundle::{Flags, TxVersion};
use confidant_core::orchard::circuit::{OrchardCircuitVersion, VerifyingKey};
use confidant_core::orchard::value::NoteValue;
use confidant_core::orchard::Anchor;
use confidant_core::record::{bundle_from_record, bundle_hash, record_from_bundle};
use confidant_core::sighash::sighash;
use confidant_core::{
    BundleRecord, DepositRef, Direction, Error, OpTag, Verifier, VerifierSnapshot, Wallet,
    WalletSecret,
};
use rand::rngs::OsRng;

fn state_json(v: &Verifier) -> String {
    serde_json::to_string(&v.state()).unwrap()
}

#[test]
fn end_to_end_shield_transfer_unshield_reveal() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    let empty_root = v.state().root;

    let acc = shield(&mut v, &mut alice, 100, "DEPOSIT-TX-1");
    let s = v.state();
    assert_eq!(s.leaf_count, 2, "bundles are padded to two actions");
    assert_eq!(
        s.nullifier_count, 2,
        "dummy-spend nullifiers are recorded too"
    );
    assert_eq!(s.bundle_count, 1);
    assert_eq!(s.consumed_deposits, 1);
    assert_ne!(s.root, empty_root);
    assert_eq!(acc.new_root, s.root);
    assert_eq!(
        acc.bundle_hash,
        hex::encode(bundle_hash(&v.records()[0]).unwrap())
    );
    assert!(v.is_deposit_consumed("DEPOSIT-TX-1"));
    assert_eq!(alice.balance(), 100);
    assert_eq!(bob.balance(), 0);
    assert_eq!(v.records()[0].external, "DEPOSIT-TX-1");

    let acc = transfer(&mut v, &mut alice, &mut bob, 60);
    assert_eq!(acc.nullifiers.len(), 2);
    assert_eq!(acc.commitments.len(), 2);
    assert!(acc.payout.is_none());
    assert_eq!(alice.balance(), 40);
    assert_eq!(bob.balance(), 60);
    assert_eq!(v.records()[1].value_balance, 0);
    assert_eq!(v.records()[1].external, "");

    let acc = unshield(&mut v, &mut bob, 25, "rBobFreshAddress");
    let payout = acc.payout.unwrap();
    assert_eq!(payout.amount, 25);
    assert_eq!(payout.destination, "rBobFreshAddress");
    assert_eq!(bob.balance(), 35);
    assert_eq!(v.records()[2].value_balance, 25);
    assert_eq!(v.records()[2].external, "rBobFreshAddress");
    assert_eq!(v.state().leaf_count, 6);

    // Auditor reveal with Bob's key: his 60 (now spent) and his 35 change; balance 35.
    let r = v.reveal(&bob.info().viewing_key).unwrap();
    assert_eq!(r.balance, 35);
    let mut incoming: Vec<(u64, bool)> = r
        .notes
        .iter()
        .filter(|n| n.direction == Direction::Incoming)
        .map(|n| (n.amount, n.spent))
        .collect();
    incoming.sort();
    assert_eq!(incoming, vec![(35, false), (60, true)]);

    // Alice's key: her 100 (spent), her 40 change, and the 60 she paid Bob (outgoing).
    let r = v.reveal(&alice.info().viewing_key).unwrap();
    assert_eq!(r.balance, 40);
    let outgoing: Vec<_> = r
        .notes
        .iter()
        .filter(|n| n.direction == Direction::Outgoing)
        .collect();
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].amount, 60);
    assert_eq!(
        outgoing[0].recipient.as_deref(),
        Some(bob.info().address.as_str())
    );

    // An unrelated key sees nothing: no dummy output is ever attributed to anyone.
    let carol = Wallet::new();
    let r = v.reveal(&carol.info().viewing_key).unwrap();
    assert_eq!(r.balance, 0);
    assert!(r.notes.is_empty());

    // The wallet's own view equals the verifier's.
    assert_eq!(bob.replica().state().root, v.state().root);

    // Anchoring hook.
    assert!(matches!(
        v.set_anchored_root(&hex::encode([1u8; 32])),
        Err(Error::UnknownAnchor)
    ));
    let root = v.state().root;
    v.set_anchored_root(&root).unwrap();
    assert_eq!(v.state().anchored_root, Some(root));
    assert_eq!(v.anchor_history().len(), 4);
}

// ----- deposit binding (v2) -------------------------------------------------------------

#[test]
fn deposit_binding_refuses_wrong_memo_amount_and_reuse() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let mut mallory = Wallet::new();
    alice.sync(v.records()).unwrap();
    let p = alice.build_shield(100).unwrap();

    // A stranger's bundle against Alice's deposit: the memo commits to Alice's bundle.
    mallory.sync(v.records()).unwrap();
    let theirs = mallory.build_shield(100).unwrap();
    assert!(matches!(
        v.submit_shield(&theirs.record, &deposit_for(&p, "DEP-A", 100)),
        Err(Error::DepositBindingMismatch)
    ));
    // Wrong delivered amount.
    assert!(matches!(
        v.submit_shield(&p.record, &deposit_for(&p, "DEP-A", 99)),
        Err(Error::ValueBalance {
            expected: -99,
            actual: -100
        })
    ));
    // Malformed memo hash.
    let mut bad = deposit_for(&p, "DEP-A", 100);
    bad.memo_hash = "zz".into();
    assert!(matches!(
        v.submit_shield(&p.record, &bad),
        Err(Error::InvalidEncoding)
    ));
    // Missing deposit reference.
    let none = deposit_for(&p, "", 100);
    assert!(matches!(
        v.submit_shield(&p.record, &none),
        Err(Error::DepositRequired)
    ));
    assert_eq!(v.state().bundle_count, 0);

    // The rightful bundle is accepted...
    v.submit_shield(&p.record, &deposit_for(&p, "DEP-A", 100))
        .unwrap();
    // ...and the same deposit cannot back a second shield, even a fresh bundle.
    alice.sync(v.records()).unwrap();
    let again = alice.build_shield(100).unwrap();
    let before = state_json(&v);
    assert!(matches!(
        v.submit_shield(&again.record, &deposit_for(&again, "DEP-A", 100)),
        Err(Error::DepositConsumed(_))
    ));
    assert_eq!(state_json(&v), before);
    // A different deposit works.
    v.submit_shield(&again.record, &deposit_for(&again, "DEP-B", 100))
        .unwrap();
    assert_eq!(v.state().consumed_deposits, 2);
}

#[test]
fn bundle_hash_covers_the_bundle_and_not_the_metadata() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    alice.sync(v.records()).unwrap();
    let p = alice.build_shield(5).unwrap();
    let h = bundle_hash(&p.record).unwrap();
    let mut meta = p.record.clone();
    meta.index = 7;
    meta.timestamp = 123;
    meta.first_leaf = 99;
    meta.external = "anything".into();
    assert_eq!(
        bundle_hash(&meta).unwrap(),
        h,
        "metadata is not part of the hash"
    );
    let mut body = p.record.clone();
    body.value_balance += 1;
    assert_ne!(bundle_hash(&body).unwrap(), h);
    let mut body = p.record.clone();
    body.actions[0].cmx = body.actions[1].cmx.clone();
    assert_ne!(bundle_hash(&body).unwrap(), h);
    let mut body = p.record.clone();
    body.proof = "zz".into();
    assert!(matches!(bundle_hash(&body), Err(Error::InvalidEncoding)));
    // The accepted record hashes the same as the submitted one.
    let acc = v
        .submit_shield(&p.record, &deposit_for(&p, "D", 5))
        .unwrap();
    assert_eq!(acc.bundle_hash, hex::encode(h));
    assert_eq!(bundle_hash(&v.records()[0]).unwrap(), h);
}

// ----- v1 guards carried over --------------------------------------------------------------

#[test]
fn double_spend_is_refused_and_state_is_unchanged() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let bob = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    // Two bundles spending the same note, both valid at the current anchor.
    let b1 = alice
        .build_transfer(&[out(&bob.info().address, 60)])
        .unwrap();
    let b2 = alice
        .build_transfer(&[out(&bob.info().address, 70)])
        .unwrap();
    v.submit_transfer(&b1.record).unwrap();
    let before = state_json(&v);
    assert!(matches!(
        v.submit_transfer(&b2.record),
        Err(Error::DoubleSpend)
    ));
    assert_eq!(state_json(&v), before);
}

#[test]
fn replay_of_an_accepted_bundle_is_refused() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    transfer(&mut v, &mut alice, &mut bob, 60);
    unshield(&mut v, &mut bob, 10, "rX");
    for record in v.records().to_vec() {
        let err = match record.op {
            OpTag::Shield => v
                .submit_shield(
                    &record,
                    &DepositRef {
                        tx_hash: "D1".into(),
                        delivered: 100,
                        memo_hash: hex::encode(bundle_hash(&record).unwrap()),
                    },
                )
                .unwrap_err(),
            OpTag::Transfer => v.submit_transfer(&record).unwrap_err(),
            OpTag::Unshield => v.submit_unshield(&record, &record.external).unwrap_err(),
        };
        assert!(
            matches!(err, Error::DoubleSpend | Error::DepositConsumed(_)),
            "{err:?}"
        );
        // Replay through the log path is refused on ordering first.
        assert!(matches!(
            v.replay(&record),
            Err(Error::RecordOutOfOrder { .. })
        ));
    }
    assert_eq!(v.state().bundle_count, 3);
}

#[test]
fn replay_is_refused_before_the_proof_is_checked() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    let mut rec = v.records()[0].clone();
    let mut proof = hex::decode(&rec.proof).unwrap();
    let mid = proof.len() / 2;
    proof[mid] ^= 0x01;
    rec.proof = hex::encode(proof);
    // Same nullifiers, corrupted proof: refused as a replay, not as a bad proof.
    let bundle = bundle_from_record(&rec, v.params().version).unwrap();
    assert!(matches!(
        v.verify(&bundle, OpTag::Shield, "D1", Some(100)),
        Err(Error::DoubleSpend)
    ));
}

#[test]
fn unknown_anchor_is_refused() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    // A shield (dummy spends only) proves fine against any anchor; the verifier must still
    // refuse one the pool has never had.
    let bogus = Anchor::from_bytes([3u8; 32]).unwrap();
    assert!(!v.anchor_history().contains(&hex::encode(bogus.to_bytes())));
    let version = v.params().version;
    let mut b = Builder::new(BundleType::DEFAULT, version, Flags::SPENDS_DISABLED, bogus).unwrap();
    b.add_output(None, alice.address(), NoteValue::from_raw(5), [0u8; 512])
        .unwrap();
    let (unauth, _) = b.build::<i64>(OsRng).unwrap().unwrap();
    let c: [u8; 32] = unauth.commitment(TxVersion::V5).unwrap().into();
    let sh = sighash(&c, OpTag::Shield, &[]);
    let bundle = unauth
        .create_proof(&v.params().pk, OsRng)
        .unwrap()
        .apply_signatures(OsRng, sh, &[])
        .unwrap();
    assert!(bundle.verify_proof(&v.params().vk).is_ok());
    let record = record_from_bundle(&bundle, 0, OpTag::Shield, "", 0, 0);
    let dep = DepositRef {
        tx_hash: "D2".into(),
        delivered: 5,
        memo_hash: hex::encode(bundle_hash(&record).unwrap()),
    };
    assert!(matches!(
        v.submit_shield(&record, &dep),
        Err(Error::UnknownAnchor)
    ));
}

#[test]
fn tampered_value_balance_is_refused() {
    let v = Verifier::new();
    let mut alice = Wallet::new();
    alice.sync(v.records()).unwrap();
    let p = alice.build_shield(100).unwrap();
    let bundle = bundle_from_record(&p.record, v.params().version).unwrap();
    let tampered = bundle
        .try_map_value_balance(|x| Ok::<i64, ()>(x - 1))
        .unwrap();
    assert!(matches!(
        v.verify(&tampered, OpTag::Shield, "D1", Some(100)),
        Err(Error::ValueBalance {
            expected: -100,
            actual: -101
        })
    ));
    // The value balance is part of the bundle commitment, so the sighash moves and every
    // spend-authorization signature fails.
    assert!(matches!(
        v.verify(&tampered, OpTag::Shield, "D1", None),
        Err(Error::InvalidSpendAuth)
    ));
}

#[test]
fn swapped_binding_signature_is_refused() {
    let v = Verifier::new();
    let mut alice = Wallet::new();
    alice.sync(v.records()).unwrap();
    let a = alice.build_shield(100).unwrap();
    let b = alice.build_shield(100).unwrap();
    let mut rec = a.record.clone();
    rec.binding_signature = b.record.binding_signature.clone();
    let forged = bundle_from_record(&rec, v.params().version).unwrap();
    assert!(matches!(
        v.verify(&forged, OpTag::Shield, "D1", Some(100)),
        Err(Error::InvalidBindingSignature)
    ));
}

#[test]
fn corrupted_proof_is_refused() {
    let v = Verifier::new();
    let mut alice = Wallet::new();
    alice.sync(v.records()).unwrap();
    let p = alice.build_shield(100).unwrap();
    let mut rec = p.record.clone();
    let mut proof = hex::decode(&rec.proof).unwrap();
    let mid = proof.len() / 2;
    proof[mid] ^= 0x01;
    rec.proof = hex::encode(proof);
    let corrupted = bundle_from_record(&rec, v.params().version).unwrap();
    assert!(matches!(
        v.verify(&corrupted, OpTag::Shield, "D1", Some(100)),
        Err(Error::InvalidProof)
    ));
}

#[test]
fn repointed_unshield_is_refused_then_original_is_accepted() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    let p = alice.build_unshield(30, "rDestinationA").unwrap();
    let before = state_json(&v);
    // Same bundle, different destination ⇒ different sighash ⇒ signatures fail.
    assert!(matches!(
        v.submit_unshield(&p.record, "rDestinationB"),
        Err(Error::InvalidSpendAuth)
    ));
    assert!(matches!(
        v.submit_unshield(&p.record, ""),
        Err(Error::Build(_))
    ));
    assert_eq!(state_json(&v), before);
    let bundle = bundle_from_record(&p.record, v.params().version).unwrap();
    assert!(matches!(
        v.verify(&bundle, OpTag::Unshield, "rDestinationA", Some(31)),
        Err(Error::ValueBalance {
            expected: 31,
            actual: 30
        })
    ));
    let acc = v.submit_unshield(&p.record, "rDestinationA").unwrap();
    assert_eq!(acc.payout.unwrap().amount, 30);
}

#[test]
fn bundle_presented_as_another_op_is_refused() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let bob = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    let t = alice
        .build_transfer(&[out(&bob.info().address, 10)])
        .unwrap();
    let u = alice.build_unshield(10, "rX").unwrap();
    let s = alice.build_shield(7).unwrap();
    let tb = bundle_from_record(&t.record, v.params().version).unwrap();
    let ub = bundle_from_record(&u.record, v.params().version).unwrap();
    let sb = bundle_from_record(&s.record, v.params().version).unwrap();
    assert!(matches!(
        v.verify(&tb, OpTag::Unshield, "rX", None),
        Err(Error::ValueBalanceSign {
            op: OpTag::Unshield,
            actual: 0
        })
    ));
    assert!(matches!(
        v.verify(&ub, OpTag::Transfer, "", None),
        Err(Error::ValueBalanceSign {
            op: OpTag::Transfer,
            actual: 10
        })
    ));
    assert!(matches!(
        v.verify(&ub, OpTag::Shield, "D", None),
        Err(Error::ValueBalanceSign {
            op: OpTag::Shield,
            actual: 10
        })
    ));
    assert!(matches!(
        v.verify(&sb, OpTag::Unshield, "rX", None),
        Err(Error::ValueBalanceSign {
            op: OpTag::Unshield,
            actual: -7
        })
    ));
    // Through the public API too: a transfer bundle handed to submit_unshield.
    assert!(matches!(
        v.submit_unshield(&t.record, "rX"),
        Err(Error::ValueBalanceSign { .. })
    ));
    assert!(matches!(
        v.submit_transfer(&u.record),
        Err(Error::ValueBalanceSign { .. })
    ));
    assert!(matches!(
        v.verify(&tb, OpTag::Transfer, "", Some(5)),
        Err(Error::VouchedAmountNotApplicable {
            op: OpTag::Transfer,
            amount: 5
        })
    ));
    assert!(v.verify(&tb, OpTag::Transfer, "", Some(0)).is_ok());
}

#[test]
fn proof_does_not_verify_under_another_circuit_version() {
    let v = Verifier::new();
    let mut alice = Wallet::new();
    alice.sync(v.records()).unwrap();
    let p = alice.build_shield(1).unwrap();
    let bundle = bundle_from_record(&p.record, v.params().version).unwrap();
    assert!(bundle.verify_proof(&v.params().vk).is_ok());
    let other = VerifyingKey::build(OrchardCircuitVersion::PostNu6_3);
    assert!(bundle.verify_proof(&other).is_err());
}

#[test]
fn wallet_refuses_bad_requests_before_proving() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let bob = Wallet::new();
    assert!(matches!(alice.build_shield(0), Err(Error::ZeroAmount)));
    shield(&mut v, &mut alice, 100, "D1");
    assert!(matches!(
        alice.build_transfer(&[out(&bob.info().address, 150)]),
        Err(Error::InsufficientFunds {
            available: 100,
            requested: 150
        })
    ));
    assert!(matches!(
        alice.build_transfer(&[out(&bob.info().address, 0)]),
        Err(Error::ZeroAmount)
    ));
    assert!(matches!(alice.build_transfer(&[]), Err(Error::ZeroAmount)));
    assert!(matches!(
        alice.build_transfer(&[out("zz", 1)]),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        alice.build_transfer(&[out(&"00".repeat(43), 1)]),
        Err(Error::InvalidEncoding)
    ));
    let many: Vec<_> = (0..9).map(|_| out(&bob.info().address, 1)).collect();
    assert!(matches!(
        alice.build_transfer(&many),
        Err(Error::TooManyOutputs(9))
    ));
    assert!(matches!(
        alice.build_transfer(&[
            out(&bob.info().address, u64::MAX),
            out(&bob.info().address, 1)
        ]),
        Err(Error::AmountOverflow)
    ));
    assert!(matches!(
        alice.build_unshield(101, "rX"),
        Err(Error::InsufficientFunds { .. })
    ));
    assert!(matches!(
        alice.build_unshield(0, "rX"),
        Err(Error::ZeroAmount)
    ));
    assert!(matches!(alice.build_unshield(1, ""), Err(Error::Build(_))));
    // A wallet that never synced has no notes and cannot build a spend.
    let fresh = Wallet::new();
    assert!(matches!(
        fresh.build_unshield(1, "rX"),
        Err(Error::InsufficientFunds {
            available: 0,
            requested: 1
        })
    ));
    assert_eq!(v.state().bundle_count, 1);
}

#[test]
fn spend_witnessed_at_an_older_anchor_is_still_accepted() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    let mut carol = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    let old = alice
        .build_transfer(&[out(&bob.info().address, 10)])
        .unwrap();
    // The tree grows past the anchor `old` was witnessed at.
    shield(&mut v, &mut carol, 5, "D2");
    shield(&mut v, &mut bob, 5, "D3");
    assert_ne!(old.record.anchor, v.state().root);
    v.submit_transfer(&old.record).unwrap();
    alice.sync(v.records()).unwrap();
    bob.sync(v.records()).unwrap();
    assert_eq!(alice.balance(), 90);
    assert_eq!(bob.balance(), 15);
}

// ----- wallet sync, snapshots, rebuild (v2) ----------------------------------------------

#[test]
fn wallet_sync_is_append_only_and_tracks_spent_notes() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    shield(&mut v, &mut alice, 5, "D2");
    transfer(&mut v, &mut alice, &mut bob, 50);
    assert_eq!(alice.balance(), 55);
    assert_eq!(alice.synced(), 3);
    assert_eq!(alice.sync(v.records()).unwrap(), 0, "nothing new");
    assert!(matches!(
        alice.sync(&v.records()[..1]),
        Err(Error::LogRewound)
    ));
    // A second wallet with the same key, synced from scratch, sees the same notes.
    let mut twin = Wallet::from_secret(&alice.secret()).unwrap();
    assert_eq!(twin.sync(v.records()).unwrap(), 3);
    assert_eq!(twin.balance(), 55);
    assert_eq!(twin.info().address, alice.info().address);
    // Exact spend without change: 50 + 5 = 55 leaves no change output.
    let before = v.state().leaf_count;
    unshield(&mut v, &mut alice, 55, "rExact");
    assert_eq!(alice.balance(), 0);
    assert_eq!(
        v.state().leaf_count,
        before + 2,
        "two spends, dummy outputs only"
    );
    // A corrupted log does not sync.
    let mut bad = v.records().to_vec();
    bad[3].value_balance = 1;
    let mut fresh = Wallet::from_secret(&alice.secret()).unwrap();
    assert!(fresh.sync(&bad).is_err());
    assert!(matches!(
        Wallet::from_secret(&WalletSecret {
            format: 2,
            spending_key: alice.secret().spending_key
        }),
        Err(Error::Snapshot(_))
    ));
    assert!(matches!(
        Wallet::from_secret(&WalletSecret {
            format: 1,
            spending_key: "zz".into()
        }),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        Wallet::from_secret(&WalletSecret {
            format: 1,
            spending_key: "00".into()
        }),
        Err(Error::InvalidEncoding)
    ));
}

#[test]
fn verifier_snapshot_restore_keeps_nullifiers_and_consumed_deposits() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    transfer(&mut v, &mut alice, &mut bob, 60);
    let root = v.state().root;
    v.set_anchored_root(&root).unwrap();

    let json = serde_json::to_string(&v.snapshot()).unwrap();
    assert!(
        !json.contains("spendingKey"),
        "verifier snapshots carry no keys"
    );
    let snap: VerifierSnapshot = serde_json::from_str(&json).unwrap();
    let mut restored = Verifier::restore(&snap).unwrap();
    assert_eq!(state_json(&restored), state_json(&v));
    assert_eq!(restored.anchor_history(), v.anchor_history());
    assert!(restored.is_deposit_consumed("D1"));
    assert_eq!(
        serde_json::to_string(&restored.reveal(&bob.info().viewing_key).unwrap()).unwrap(),
        serde_json::to_string(&v.reveal(&bob.info().viewing_key).unwrap()).unwrap()
    );
    // Pre-restart nullifiers are still spent; a note received before the restart still spends.
    assert!(matches!(
        restored.submit_transfer(&v.records()[1]),
        Err(Error::DoubleSpend)
    ));
    bob.sync(restored.records()).unwrap();
    let p = bob
        .build_transfer(&[out(&alice.info().address, 10)])
        .unwrap();
    restored.submit_transfer(&p.record).unwrap();

    // Corruption is refused.
    let mut bad = v.snapshot();
    bad.format = 2;
    assert!(matches!(Verifier::restore(&bad), Err(Error::Snapshot(_))));
    let mut bad = v.snapshot();
    bad.records[1].value_balance = 1;
    assert!(Verifier::restore(&bad).is_err());
    let mut bad = v.snapshot();
    bad.records[1].first_leaf += 1;
    assert!(matches!(Verifier::restore(&bad), Err(Error::Snapshot(_))));
    let mut bad = v.snapshot();
    bad.records.swap(0, 1);
    assert!(matches!(
        Verifier::restore(&bad),
        Err(Error::RecordOutOfOrder { .. })
    ));
    let mut bad = v.snapshot();
    bad.records[0].external.clear();
    assert!(matches!(
        Verifier::restore(&bad),
        Err(Error::DepositRequired)
    ));
    // Two shields in a log claiming the same deposit: the second is refused on replay.
    let mut v2 = Verifier::new();
    let mut carol = Wallet::new();
    shield(&mut v2, &mut carol, 1, "DUP");
    shield(&mut v2, &mut carol, 2, "OTHER");
    let mut bad = v2.snapshot();
    bad.records[1].external = "DUP".into();
    assert!(matches!(
        Verifier::restore(&bad),
        Err(Error::DepositConsumed(_))
    ));
    let mut bad = v.snapshot();
    bad.anchored_root = Some("00".repeat(32));
    assert!(matches!(Verifier::restore(&bad), Err(Error::UnknownAnchor)));
    let no_anchor = VerifierSnapshot {
        anchored_root: None,
        ..v.snapshot()
    };
    assert_eq!(
        Verifier::restore(&no_anchor).unwrap().state().anchored_root,
        None
    );
}

#[test]
fn anchor_memos_round_trip_and_rebuild_the_verifier() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    transfer(&mut v, &mut alice, &mut bob, 60);
    unshield(&mut v, &mut bob, 25, "rBobFreshAddress");

    // One memo per op; each round-trips through bytes and matches the verifier's own view.
    let memos: Vec<AnchorMemo> = (0..3).map(|i| v.anchor_memo(i).unwrap()).collect();
    for (i, m) in memos.iter().enumerate() {
        let bytes = m.encode().unwrap();
        assert!(bytes.len() <= confidant_core::memo::MAX_BYTES);
        let back = AnchorMemo::decode(&bytes).unwrap();
        assert_eq!(&back, m);
        assert_eq!(
            AnchorMemo::decode_hex(&m.encode_hex().unwrap()).unwrap(),
            *m
        );
        v.check_memo(i, &back).unwrap();
        assert_eq!(hex::encode(m.root), v.anchor_history()[i + 1]);
        assert_eq!(m.op, v.records()[i].op);
        assert_eq!(m.external, v.records()[i].external);
        assert_eq!(m.actions.len(), 2);
    }
    assert_eq!(memos[0].external, "D1");
    assert_eq!(memos[1].external, "");
    assert_eq!(memos[2].external, "rBobFreshAddress");
    assert!(matches!(
        v.anchor_memo(3),
        Err(Error::MemoMismatch { index: 3, .. })
    ));

    // A tampered memo is detected field by field.
    let mut m = memos[1].clone();
    m.root[0] ^= 1;
    assert!(matches!(
        v.check_memo(1, &m),
        Err(Error::MemoMismatch { what: "root", .. })
    ));
    let mut m = memos[1].clone();
    m.op = OpTag::Shield;
    assert!(matches!(
        v.check_memo(1, &m),
        Err(Error::MemoMismatch { what: "op", .. })
    ));
    let mut m = memos[1].clone();
    m.bundle_hash[0] ^= 1;
    assert!(matches!(
        v.check_memo(1, &m),
        Err(Error::MemoMismatch {
            what: "bundle hash",
            ..
        })
    ));
    let mut m = memos[1].clone();
    m.external = "x".into();
    assert!(matches!(
        v.check_memo(1, &m),
        Err(Error::MemoMismatch {
            what: "external context",
            ..
        })
    ));
    let mut m = memos[1].clone();
    m.actions.pop();
    assert!(matches!(
        v.check_memo(1, &m),
        Err(Error::MemoMismatch {
            what: "actions",
            ..
        })
    ));

    // A new node rebuilds from the memos plus a mirror and lands on the same root.
    let mirror_log: Vec<BundleRecord> = v.records().to_vec();
    let mirror = |h: &[u8; 32]| {
        mirror_log
            .iter()
            .find(|r| bundle_hash(r).unwrap() == *h)
            .cloned()
    };
    let node = Verifier::rebuild(&memos, mirror).unwrap();
    assert_eq!(node.state().root, v.state().root);
    assert_eq!(node.anchor_history(), v.anchor_history());
    assert!(node.is_deposit_consumed("D1"));
    assert_eq!(
        serde_json::to_string(&node.reveal(&bob.info().viewing_key).unwrap()).unwrap(),
        serde_json::to_string(&v.reveal(&bob.info().viewing_key).unwrap()).unwrap()
    );

    // Mirror serves nothing / a wrong bundle / a memo claims a wrong root or actions.
    assert!(matches!(
        Verifier::rebuild(&memos, |_| None),
        Err(Error::MirrorMissing(_))
    ));
    let wrong = |_: &[u8; 32]| Some(mirror_log[1].clone());
    assert!(matches!(
        Verifier::rebuild(&memos, wrong),
        Err(Error::BundleHashMismatch)
    ));
    let mut bad_root = memos.clone();
    bad_root[2].root[0] ^= 1;
    assert!(matches!(
        Verifier::rebuild(&bad_root, mirror),
        Err(Error::MemoMismatch {
            index: 2,
            what: "root"
        })
    ));
    let mut bad_actions = memos.clone();
    bad_actions[1].actions.swap(0, 1);
    assert!(matches!(
        Verifier::rebuild(&bad_actions, mirror),
        Err(Error::MemoMismatch {
            index: 1,
            what: "actions"
        })
    ));
    // A memo whose external disagrees with how the bundle was signed is refused by the tail.
    let mut bad_ext = memos.clone();
    bad_ext[2].external = "rAttacker".into();
    assert!(matches!(
        Verifier::rebuild(&bad_ext, mirror),
        Err(Error::InvalidSpendAuth)
    ));
}

#[test]
fn anchor_memo_codec_rejects_malformed_bytes_and_oversize() {
    let m = AnchorMemo {
        op: OpTag::Unshield,
        root: [1u8; 32],
        bundle_hash: [2u8; 32],
        external: "rDest".into(),
        actions: vec![([3u8; 32], [4u8; 32]); 8],
    };
    let bytes = m.encode().unwrap();
    assert_eq!(bytes.len(), 1 + 1 + 32 + 32 + 1 + 5 + 1 + 8 * 64);
    assert_eq!(AnchorMemo::decode(&bytes).unwrap(), m);
    // Version, op, truncation, trailing bytes, bad UTF-8.
    let mut b = bytes.clone();
    b[0] = 9;
    assert!(matches!(
        AnchorMemo::decode(&b),
        Err(Error::MemoMalformed("unknown version"))
    ));
    let mut b = bytes.clone();
    b[1] = 0;
    assert!(matches!(
        AnchorMemo::decode(&b),
        Err(Error::MemoMalformed("unknown op"))
    ));
    assert!(matches!(
        AnchorMemo::decode(&bytes[..bytes.len() - 1]),
        Err(Error::MemoMalformed("truncated"))
    ));
    let mut b = bytes.clone();
    b.push(0);
    assert!(matches!(
        AnchorMemo::decode(&b),
        Err(Error::MemoMalformed("trailing bytes"))
    ));
    let mut b = bytes.clone();
    b[67] = 0xff; // first byte of "rDest"
    assert!(matches!(
        AnchorMemo::decode(&b),
        Err(Error::MemoMalformed("external context is not UTF-8"))
    ));
    assert!(matches!(
        AnchorMemo::decode(&[]),
        Err(Error::MemoMalformed("truncated"))
    ));
    assert!(matches!(
        AnchorMemo::decode_hex("zz"),
        Err(Error::InvalidEncoding)
    ));
    // Encoding limits.
    let mut big = m.clone();
    big.actions = vec![([3u8; 32], [4u8; 32]); 16];
    assert!(matches!(big.encode(), Err(Error::MemoTooLarge(_))));
    let mut huge = m.clone();
    huge.actions = vec![([3u8; 32], [4u8; 32]); 300];
    assert!(matches!(
        huge.encode(),
        Err(Error::MemoMalformed("more than 255 actions"))
    ));
    let mut long = m.clone();
    long.external = "x".repeat(256);
    assert!(matches!(
        long.encode(),
        Err(Error::MemoMalformed(
            "external context longer than 255 bytes"
        ))
    ));
    // A record whose fields are not hex cannot produce a memo.
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    shield(&mut v, &mut alice, 1, "D1");
    let mut rec = v.records()[0].clone();
    rec.actions[0].nullifier = "zz".into();
    assert!(matches!(
        AnchorMemo::from_record(&rec, [0u8; 32]),
        Err(Error::InvalidEncoding)
    ));
}

#[test]
fn record_roundtrip_is_exact_and_malformed_records_are_refused() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    transfer(&mut v, &mut alice, &mut bob, 60);
    unshield(&mut v, &mut bob, 1, "rX");
    for record in v.records() {
        let bundle = bundle_from_record(record, v.params().version).unwrap();
        let again = record_from_bundle(
            &bundle,
            record.index,
            record.op,
            &record.external,
            record.first_leaf,
            record.timestamp,
        );
        assert_eq!(record, &again);
        assert!(bundle.verify_proof(&v.params().vk).is_ok());
    }
    let with = |f: &dyn Fn(&mut BundleRecord)| {
        let mut rec = v.records()[0].clone();
        f(&mut rec);
        bundle_from_record(&rec, v.params().version).map(|_| ())
    };
    let ff = "ff".repeat(32);
    let zero = "00".repeat(32);
    for bad in ["zz", "00"] {
        let b = bad.to_string();
        for field in 0..8 {
            let r = with(&|r| {
                let a = &mut r.actions[0];
                match field {
                    0 => a.nullifier = b.clone(),
                    1 => a.rk = b.clone(),
                    2 => a.cmx = b.clone(),
                    3 => a.cv_net = b.clone(),
                    4 => a.epk = b.clone(),
                    5 => a.enc_ciphertext = b.clone(),
                    6 => a.out_ciphertext = b.clone(),
                    _ => a.spend_auth_sig = b.clone(),
                }
            });
            assert!(
                matches!(r, Err(Error::InvalidEncoding)),
                "field {field} = {bad}"
            );
        }
    }
    assert!(matches!(
        with(&|r| r.actions[0].nullifier = ff.clone()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.actions[0].cmx = ff.clone()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.actions[0].cv_net = ff.clone()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.actions[0].rk = ff.clone()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.actions[0].epk = zero.clone()),
        Err(Error::Build(_) | Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.actions[0].rk = zero.clone()),
        Err(Error::Build(_) | Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.actions.clear()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.flags = 0x80),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.anchor = "zz".into()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.anchor = ff.clone()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.proof = "zz".into()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with(&|r| r.proof.truncate(r.proof.len() - 2)),
        Err(Error::Build(_))
    ));
    assert!(matches!(
        with(&|r| r.binding_signature = "00".into()),
        Err(Error::InvalidEncoding)
    ));
    // Canonical bytes refuse the same malformed fields.
    let mut rec = v.records()[0].clone();
    rec.anchor = "zz".into();
    assert!(matches!(bundle_hash(&rec), Err(Error::InvalidEncoding)));
    let mut rec = v.records()[0].clone();
    rec.binding_signature = "00".into();
    assert!(matches!(bundle_hash(&rec), Err(Error::InvalidEncoding)));
    let mut rec = v.records()[0].clone();
    rec.actions[0].enc_ciphertext = "00".into();
    assert!(matches!(bundle_hash(&rec), Err(Error::InvalidEncoding)));
}

#[test]
fn accessors_defaults_and_error_messages() {
    let v = Verifier::default();
    assert_eq!(v.state().bundle_count, 0);
    assert_eq!(v.anchor_history().len(), 1);
    assert!(v.bundle(0).is_none());
    assert!(!v.is_spent(&[0u8; 32]));
    assert!(!v.is_deposit_consumed("x"));
    assert!(matches!(v.witness(0), Err(Error::NoWitness(0))));
    let w = Wallet::default();
    assert_eq!(w.balance(), 0);
    assert_eq!(w.synced(), 0);
    assert_eq!(w.replica().state().root, v.state().root);
    assert_eq!(w.info().viewing_key.len(), 192);
    assert_eq!(w.info().address.len(), 86);
    assert_eq!(
        hex::encode(w.address().to_raw_address_bytes()),
        w.info().address
    );
    assert_eq!(w.secret().format, 1);
    assert!(matches!(v.reveal("zz"), Err(Error::InvalidEncoding)));
    assert!(matches!(v.reveal("00"), Err(Error::InvalidEncoding)));
    assert!(matches!(
        v.reveal(&"00".repeat(96)),
        Err(Error::InvalidEncoding)
    ));
    let mut v = v;
    assert!(matches!(
        v.set_anchored_root("zz"),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        v.set_anchored_root("00"),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        v.set_anchored_root(&"ff".repeat(32)),
        Err(Error::InvalidEncoding)
    ));
    for e in [
        Error::ZeroAmount,
        Error::AmountOverflow,
        Error::InvalidEncoding,
        Error::DoubleSpend,
        Error::UnknownAnchor,
        Error::InvalidProof,
        Error::InvalidSpendAuth,
        Error::InvalidBindingSignature,
        Error::TooManyOutputs(9),
        Error::Snapshot("x".into()),
        Error::Tree("x".into()),
        Error::NoWitness(3),
        Error::Internal("x"),
        Error::Build("x".into()),
        Error::InsufficientFunds {
            available: 1,
            requested: 2,
        },
        Error::ValueBalance {
            expected: 1,
            actual: 2,
        },
        Error::ValueBalanceSign {
            op: OpTag::Shield,
            actual: 1,
        },
        Error::VouchedAmountNotApplicable {
            op: OpTag::Transfer,
            amount: 5,
        },
        Error::DepositConsumed("d".into()),
        Error::DepositBindingMismatch,
        Error::DepositRequired,
        Error::MemoMalformed("m"),
        Error::MemoTooLarge(2000),
        Error::MemoMismatch {
            index: 1,
            what: "root",
        },
        Error::MirrorMissing("h".into()),
        Error::BundleHashMismatch,
        Error::LogRewound,
        Error::RecordOutOfOrder {
            expected: 1,
            found: 2,
        },
    ] {
        assert!(!e.to_string().is_empty());
    }
}
