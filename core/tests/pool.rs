//! The negative suite from `docs/audit/REQ-pool-design-2026-10-04.md` §8 plus the audit
//! session's IMPL conditions (RESP, Q-POOL-6). Each test names the guard it exercises.

use confidant_core::orchard::builder::{Builder, BundleType};
use confidant_core::orchard::bundle::Flags;
use confidant_core::orchard::bundle::TxVersion;
use confidant_core::orchard::circuit::{OrchardCircuitVersion, VerifyingKey};
use confidant_core::orchard::value::NoteValue;
use confidant_core::orchard::{Address, Anchor};
use confidant_core::record::bundle_from_record;
use confidant_core::sighash::{shield_ctx, sighash};
use confidant_core::{Direction, Error, OpTag, Pool, TransferOutput};
use rand::rngs::OsRng;

fn pool_with(owners: &[&str]) -> Pool {
    let mut pool = Pool::new();
    for o in owners {
        pool.register_owner(o).unwrap();
    }
    pool
}

fn out(recipient: &str, amount: u64) -> TransferOutput {
    TransferOutput {
        recipient: recipient.to_owned(),
        amount,
    }
}

#[test]
fn end_to_end_shield_transfer_unshield_reveal() {
    let mut pool = pool_with(&["alice", "bob"]);
    let empty_root = pool.state().root;

    let shield = pool.shield("alice", 100, "DEPOSIT-TX-1").unwrap();
    let s = pool.state();
    assert_eq!(s.leaf_count, 2, "bundles are padded to two actions");
    assert_eq!(
        s.nullifier_count, 2,
        "dummy-spend nullifiers are recorded too"
    );
    assert_eq!(s.bundle_count, 1);
    assert_ne!(s.root, empty_root);
    assert_eq!(shield.new_root, s.root);
    assert!(shield.leaf_index < 2);
    assert_eq!(pool.balance("alice").unwrap(), 100);
    assert_eq!(pool.balance("bob").unwrap(), 0);

    let transfer = pool.transfer("alice", &[out("bob", 60)]).unwrap();
    assert_eq!(transfer.nullifiers.len(), 2);
    assert_eq!(transfer.new_commitments.len(), 2);
    assert_eq!(pool.balance("alice").unwrap(), 40);
    assert_eq!(pool.balance("bob").unwrap(), 60);
    assert_eq!(pool.records()[1].value_balance, 0);

    let unshield = pool.unshield("bob", 25, "rBobFreshAddress").unwrap();
    assert_eq!(unshield.payout.amount, 25);
    assert_eq!(unshield.payout.destination, "rBobFreshAddress");
    assert_eq!(pool.balance("bob").unwrap(), 35);
    assert_eq!(pool.records()[2].value_balance, 25);
    assert_eq!(pool.state().leaf_count, 6);

    // Auditor reveal with Bob's key: his 60 (now spent) and his 35 change; balance 35.
    let bob = pool.owner_info("bob").unwrap();
    let r = pool.reveal(&bob.viewing_key).unwrap();
    assert_eq!(r.balance, 35);
    let mut incoming: Vec<(u64, bool)> = r
        .notes
        .iter()
        .filter(|n| n.direction == Direction::Incoming)
        .map(|n| (n.amount, n.spent))
        .collect();
    incoming.sort();
    assert_eq!(incoming, vec![(35, false), (60, true)]);
    assert!(r.notes.iter().all(|n| n.direction == Direction::Incoming));

    // Alice's key: her 100 (spent), her 40 change, and the 60 she paid Bob (outgoing).
    let alice = pool.owner_info("alice").unwrap();
    let r = pool.reveal(&alice.viewing_key).unwrap();
    assert_eq!(r.balance, 40);
    let outgoing: Vec<_> = r
        .notes
        .iter()
        .filter(|n| n.direction == Direction::Outgoing)
        .collect();
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].amount, 60);
    assert_eq!(outgoing[0].recipient.as_deref(), Some(bob.address.as_str()));
    let mut incoming: Vec<(u64, bool)> = r
        .notes
        .iter()
        .filter(|n| n.direction == Direction::Incoming)
        .map(|n| (n.amount, n.spent))
        .collect();
    incoming.sort();
    assert_eq!(incoming, vec![(40, false), (100, true)]);

    // An unrelated key sees nothing: no dummy output is ever attributed to anyone.
    let mut other = Pool::new();
    let carol = other.register_owner("carol").unwrap();
    let r = pool.reveal(&carol.viewing_key).unwrap();
    assert_eq!(r.balance, 0);
    assert!(r.notes.is_empty());

    // Anchoring hook.
    assert!(matches!(
        pool.set_anchored_root(&hex::encode([1u8; 32])),
        Err(Error::UnknownAnchor)
    ));
    pool.set_anchored_root(&unshield.new_root).unwrap();
    assert_eq!(
        pool.state().anchored_root.as_deref(),
        Some(unshield.new_root.as_str())
    );
    assert_eq!(pool.anchor_history().len(), 4);
}

#[test]
fn double_spend_is_refused_and_state_is_unchanged() {
    let mut pool = pool_with(&["alice", "bob"]);
    pool.shield("alice", 100, "D1").unwrap();
    // Two bundles spending the same note, both valid at the current anchor.
    let b1 = pool.build_transfer("alice", &[out("bob", 60)]).unwrap();
    let b2 = pool.build_transfer("alice", &[out("bob", 70)]).unwrap();
    pool.submit(b1, OpTag::Transfer, "", None).unwrap();
    let before = serde_json::to_string(&pool.state()).unwrap();
    let before_records = pool.records().len();
    assert!(matches!(
        pool.submit(b2, OpTag::Transfer, "", None),
        Err(Error::DoubleSpend)
    ));
    assert_eq!(serde_json::to_string(&pool.state()).unwrap(), before);
    assert_eq!(pool.records().len(), before_records);
    assert_eq!(pool.balance("alice").unwrap(), 40);
    assert_eq!(pool.balance("bob").unwrap(), 60);
}

#[test]
fn replay_of_an_accepted_bundle_is_refused() {
    let mut pool = pool_with(&["alice", "bob"]);
    pool.shield("alice", 100, "D1").unwrap();
    pool.transfer("alice", &[out("bob", 60)]).unwrap();
    for record in pool.records().to_vec() {
        let bundle = bundle_from_record(&record, pool.params().version).unwrap();
        // The rebuilt bundle still verifies as a bundle...
        assert!(pool
            .verify(&bundle, record.op, &record.external, None)
            .is_err());
        // ...but its nullifiers are spent, so the pool refuses it.
        assert!(matches!(
            pool.submit(bundle, record.op, &record.external, None),
            Err(Error::DoubleSpend)
        ));
    }
}

#[test]
fn unknown_anchor_is_refused() {
    let mut pool = pool_with(&["alice"]);
    pool.shield("alice", 100, "D1").unwrap();
    let alice = pool.owner_info("alice").unwrap();
    let addr_bytes: [u8; 43] = hex::decode(alice.address).unwrap().try_into().unwrap();
    let recipient = Address::from_raw_address_bytes(&addr_bytes).unwrap();
    // A shield (dummy spends only) proves fine against any anchor; the pool must still refuse
    // one it has never had.
    let bogus = Anchor::from_bytes([3u8; 32]).unwrap();
    assert!(!pool
        .anchor_history()
        .contains(&hex::encode(bogus.to_bytes())));
    let version = pool.params().version;
    let mut b = Builder::new(BundleType::DEFAULT, version, Flags::SPENDS_DISABLED, bogus).unwrap();
    b.add_output(None, recipient, NoteValue::from_raw(5), [0u8; 512])
        .unwrap();
    let (unauth, _) = b.build::<i64>(OsRng).unwrap().unwrap();
    let c: [u8; 32] = unauth.commitment(TxVersion::V5).unwrap().into();
    let sh = sighash(&c, OpTag::Shield, &shield_ctx("D2"));
    let bundle = unauth
        .create_proof(&pool.params().pk, OsRng)
        .unwrap()
        .apply_signatures(OsRng, sh, &[])
        .unwrap();
    assert!(bundle.verify_proof(&pool.params().vk).is_ok());
    assert!(matches!(
        pool.submit(bundle, OpTag::Shield, "D2", Some(5)),
        Err(Error::UnknownAnchor)
    ));
}

#[test]
fn tampered_value_balance_is_refused() {
    let mut pool = pool_with(&["alice"]);
    let (bundle, _) = pool.build_shield("alice", 100, "D1").unwrap();
    let tampered = bundle
        .try_map_value_balance(|v| Ok::<i64, ()>(v - 1))
        .unwrap();
    // With the vouched amount the mismatch is caught first.
    assert!(matches!(
        pool.verify(&tampered, OpTag::Shield, "D1", Some(100)),
        Err(Error::ValueBalance {
            expected: -100,
            actual: -101
        })
    ));
    // Without it: the value balance is part of the bundle commitment, so the sighash moves and
    // every spend-authorization signature fails.
    assert!(matches!(
        pool.submit(tampered, OpTag::Shield, "D1", None),
        Err(Error::InvalidSpendAuth)
    ));
    assert_eq!(pool.state().bundle_count, 0);
}

#[test]
fn swapped_binding_signature_is_refused() {
    let mut pool = pool_with(&["alice"]);
    let (a, _) = pool.build_shield("alice", 100, "D1").unwrap();
    let (b, _) = pool.build_shield("alice", 100, "D1").unwrap();
    // Same op, same sighash inputs except the actions themselves; b's binding signature does not
    // bind a's value commitments. Spend-auth signatures of `a` still verify, so this isolates
    // the binding-signature guard.
    let mut rec = confidant_core::record::record_from_bundle(&a, 0, OpTag::Shield, "D1", 0, 0);
    rec.binding_signature =
        confidant_core::record::record_from_bundle(&b, 0, OpTag::Shield, "D1", 0, 0)
            .binding_signature;
    let forged = bundle_from_record(&rec, pool.params().version).unwrap();
    assert!(matches!(
        pool.submit(forged, OpTag::Shield, "D1", Some(100)),
        Err(Error::InvalidBindingSignature)
    ));
    assert_eq!(pool.state().bundle_count, 0);
}

#[test]
fn corrupted_proof_is_refused() {
    let mut pool = pool_with(&["alice"]);
    let (bundle, _) = pool.build_shield("alice", 100, "D1").unwrap();
    let mut rec = confidant_core::record::record_from_bundle(&bundle, 0, OpTag::Shield, "D1", 0, 0);
    // Flip one byte in the middle of the proof; the length stays canonical.
    let mut proof = hex::decode(&rec.proof).unwrap();
    let mid = proof.len() / 2;
    proof[mid] ^= 0x01;
    rec.proof = hex::encode(proof);
    let corrupted = bundle_from_record(&rec, pool.params().version).unwrap();
    assert!(matches!(
        pool.submit(corrupted, OpTag::Shield, "D1", Some(100)),
        Err(Error::InvalidProof)
    ));
    assert_eq!(pool.state().bundle_count, 0);
}

#[test]
fn repointed_unshield_is_refused_then_original_is_accepted() {
    let mut pool = pool_with(&["alice"]);
    pool.shield("alice", 100, "D1").unwrap();
    let bundle = pool.build_unshield("alice", 30, "rDestinationA").unwrap();
    // Same bundle, different destination ⇒ different sighash ⇒ signatures fail.
    assert!(matches!(
        pool.verify(&bundle, OpTag::Unshield, "rDestinationB", Some(30)),
        Err(Error::InvalidSpendAuth)
    ));
    // Same destination, claimed payout ≠ the bundle's own value balance ⇒ refused before anything.
    assert!(matches!(
        pool.verify(&bundle, OpTag::Unshield, "rDestinationA", Some(31)),
        Err(Error::ValueBalance {
            expected: 31,
            actual: 30
        })
    ));
    let acc = pool
        .submit(bundle, OpTag::Unshield, "rDestinationA", Some(30))
        .unwrap();
    assert_eq!(acc.bundle_index, 1);
    assert_eq!(pool.balance("alice").unwrap(), 70);
}

#[test]
fn bundle_presented_as_another_op_is_refused() {
    let mut pool = pool_with(&["alice", "bob"]);
    pool.shield("alice", 100, "D1").unwrap();
    let transfer = pool.build_transfer("alice", &[out("bob", 10)]).unwrap();
    assert!(matches!(
        pool.verify(&transfer, OpTag::Unshield, "rX", None),
        Err(Error::ValueBalanceSign {
            op: OpTag::Unshield,
            actual: 0
        })
    ));
    let unshield = pool.build_unshield("alice", 10, "rX").unwrap();
    assert!(matches!(
        pool.verify(&unshield, OpTag::Transfer, "", None),
        Err(Error::ValueBalanceSign {
            op: OpTag::Transfer,
            actual: 10
        })
    ));
    assert!(matches!(
        pool.verify(&unshield, OpTag::Shield, "rX", None),
        Err(Error::ValueBalanceSign {
            op: OpTag::Shield,
            actual: 10
        })
    ));
    let (shield, _) = pool.build_shield("alice", 7, "D2").unwrap();
    assert!(matches!(
        pool.verify(&shield, OpTag::Unshield, "D2", None),
        Err(Error::ValueBalanceSign {
            op: OpTag::Unshield,
            actual: -7
        })
    ));
}

#[test]
fn proof_does_not_verify_under_another_circuit_version() {
    let pool = pool_with(&["alice"]);
    let (bundle, _) = pool.build_shield("alice", 1, "D1").unwrap();
    assert!(bundle.verify_proof(&pool.params().vk).is_ok());
    let other = VerifyingKey::build(OrchardCircuitVersion::PostNu6_3);
    assert!(bundle.verify_proof(&other).is_err());
}

#[test]
fn bad_requests_are_refused_before_proving() {
    let mut pool = pool_with(&["alice", "bob"]);
    assert!(matches!(
        pool.shield("alice", 0, "D"),
        Err(Error::ZeroAmount)
    ));
    assert!(matches!(
        pool.shield("nobody", 1, "D"),
        Err(Error::UnknownOwner(_))
    ));
    pool.shield("alice", 100, "D1").unwrap();
    assert!(matches!(
        pool.transfer("alice", &[out("bob", 150)]),
        Err(Error::InsufficientFunds {
            available: 100,
            requested: 150
        })
    ));
    assert!(matches!(
        pool.transfer("alice", &[out("bob", 0)]),
        Err(Error::ZeroAmount)
    ));
    assert!(matches!(
        pool.transfer("alice", &[]),
        Err(Error::ZeroAmount)
    ));
    assert!(matches!(
        pool.transfer("alice", &[out("nobody", 1)]),
        Err(Error::UnknownOwner(_))
    ));
    let many: Vec<_> = (0..9).map(|_| out("bob", 1)).collect();
    assert!(matches!(
        pool.transfer("alice", &many),
        Err(Error::TooManyOutputs(9))
    ));
    assert!(matches!(
        pool.unshield("alice", 101, "rX"),
        Err(Error::InsufficientFunds { .. })
    ));
    assert!(matches!(
        pool.unshield("alice", 1, ""),
        Err(Error::Build(_))
    ));
    assert!(matches!(
        pool.register_owner("alice"),
        Err(Error::OwnerExists(_))
    ));
    assert_eq!(pool.state().bundle_count, 1);
}

#[test]
fn spend_witnessed_at_an_older_anchor_is_still_accepted() {
    let mut pool = pool_with(&["alice", "bob", "carol"]);
    pool.shield("alice", 100, "D1").unwrap();
    let old = pool.build_transfer("alice", &[out("bob", 10)]).unwrap();
    // The tree grows past the anchor `old` was witnessed at.
    pool.shield("carol", 5, "D2").unwrap();
    pool.shield("bob", 5, "D3").unwrap();
    assert_ne!(
        hex::encode(old.anchor().to_bytes()),
        pool.state().root,
        "the bundle's anchor is no longer the current root"
    );
    pool.submit(old, OpTag::Transfer, "", None).unwrap();
    assert_eq!(pool.balance("alice").unwrap(), 90);
    assert_eq!(pool.balance("bob").unwrap(), 15);
}

#[test]
fn snapshot_restore_keeps_nullifiers_and_witnesses() {
    let mut pool = pool_with(&["alice", "bob"]);
    pool.shield("alice", 100, "D1").unwrap();
    pool.transfer("alice", &[out("bob", 60)]).unwrap();
    pool.set_anchored_root(&pool.state().root).unwrap();

    let json = serde_json::to_string(&pool.snapshot()).unwrap();
    let restored = Pool::restore(&serde_json::from_str(&json).unwrap()).unwrap();
    let mut restored = restored;

    assert_eq!(
        serde_json::to_string(&restored.state()).unwrap(),
        serde_json::to_string(&pool.state()).unwrap()
    );
    assert_eq!(restored.balance("alice").unwrap(), 40);
    assert_eq!(restored.balance("bob").unwrap(), 60);
    assert_eq!(restored.anchor_history(), pool.anchor_history());
    let bob = pool.owner_info("bob").unwrap();
    assert_eq!(
        restored.owner_info("bob").unwrap().viewing_key,
        bob.viewing_key
    );
    assert_eq!(
        serde_json::to_string(&restored.reveal(&bob.viewing_key).unwrap()).unwrap(),
        serde_json::to_string(&pool.reveal(&bob.viewing_key).unwrap()).unwrap()
    );

    // A pre-restart nullifier is still spent after restore.
    let replay = bundle_from_record(&pool.records()[1], pool.params().version).unwrap();
    assert!(matches!(
        restored.submit(replay, OpTag::Transfer, "", None),
        Err(Error::DoubleSpend)
    ));
    // A note received before the restart still spends after it.
    restored.transfer("bob", &[out("alice", 10)]).unwrap();
    assert_eq!(restored.balance("bob").unwrap(), 50);
    assert_eq!(restored.balance("alice").unwrap(), 50);

    // A corrupted log does not restore: a tampered value balance fails the verify tail...
    let mut bad = pool.snapshot();
    bad.records[1].value_balance = 1;
    assert!(Pool::restore(&bad).is_err());
    // ...and a leaf index that disagrees with the replayed tree is caught too.
    let mut bad = pool.snapshot();
    bad.records[1].first_leaf += 1;
    assert!(matches!(Pool::restore(&bad), Err(Error::Snapshot(_))));
    let mut bad = pool.snapshot();
    bad.records.swap(0, 1);
    assert!(matches!(Pool::restore(&bad), Err(Error::Snapshot(_))));
}

#[test]
fn record_roundtrip_is_exact() {
    let mut pool = pool_with(&["alice", "bob"]);
    pool.shield("alice", 100, "D1").unwrap();
    pool.transfer("alice", &[out("bob", 60)]).unwrap();
    pool.unshield("bob", 1, "rX").unwrap();
    for record in pool.records() {
        let bundle = bundle_from_record(record, pool.params().version).unwrap();
        let again = confidant_core::record::record_from_bundle(
            &bundle,
            record.index,
            record.op,
            &record.external,
            record.first_leaf,
            record.timestamp,
        );
        assert_eq!(
            serde_json::to_string(record).unwrap(),
            serde_json::to_string(&again).unwrap()
        );
        assert!(bundle.verify_proof(&pool.params().vk).is_ok());
    }
}
