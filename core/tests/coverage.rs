//! Tests for the branches the behavioural suite in `pool.rs` does not reach: accessors,
//! encoding errors, snapshot corruption, note selection corners and record parsing. Together
//! with `tests/pool.rs` they keep line coverage of the crate at 100% (CI enforces it).

use confidant_core::orchard::circuit::ProvingKey;
use confidant_core::record::{bundle_from_record, record_from_bundle};
use confidant_core::{BundleRecord, Error, OpTag, Pool, Snapshot, TransferOutput};

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
fn accessors_and_defaults() {
    let mut pool = Pool::default();
    assert_eq!(pool.state().bundle_count, 0);
    assert_eq!(pool.anchor_history().len(), 1);
    assert!(pool.bundle(0).is_none());
    assert!(matches!(
        pool.balance("nobody"),
        Err(Error::UnknownOwner(_))
    ));
    assert!(matches!(
        pool.owner_info("nobody"),
        Err(Error::UnknownOwner(_))
    ));
    // Params are process-wide and built from the configured circuit version.
    let other = ProvingKey::build(pool.params().version.circuit_version());
    assert_eq!(other.circuit_version(), pool.params().pk.circuit_version());

    pool.register_owner("alice").unwrap();
    pool.shield("alice", 10, "D1").unwrap();
    assert!(pool.bundle(0).is_some());
    assert!(pool.bundle(1).is_none());
    for nf in &pool.records()[0].actions {
        let bytes: [u8; 32] = hex::decode(&nf.nullifier).unwrap().try_into().unwrap();
        assert!(pool.is_spent(&bytes));
    }
    assert!(!pool.is_spent(&[0u8; 32]));
    // Every error renders a message.
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
        Error::OwnerExists("a".into()),
        Error::UnknownOwner("a".into()),
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
    ] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn note_selection_stops_early_and_exact_spends_have_no_change() {
    let mut pool = pool_with(&["alice", "bob"]);
    pool.shield("alice", 100, "D1").unwrap();
    pool.shield("alice", 5, "D2").unwrap();
    // Largest-first: the 100 note alone covers 50, the 5 note stays untouched.
    pool.transfer("alice", &[out("bob", 50)]).unwrap();
    assert_eq!(pool.balance("alice").unwrap(), 55);
    let alice = pool.owner_info("alice").unwrap();
    let r = pool.reveal(&alice.viewing_key).unwrap();
    let unspent: Vec<u64> = r
        .notes
        .iter()
        .filter(|n| !n.spent && n.recipient.is_none())
        .map(|n| n.amount)
        .collect();
    assert!(unspent.contains(&5), "the small note was not selected");
    assert!(unspent.contains(&50), "change came back");

    // Exact spend: 50 + 5 = 55 leaves no change output.
    let before = pool.state().leaf_count;
    pool.unshield("alice", 55, "rExact").unwrap();
    assert_eq!(pool.balance("alice").unwrap(), 0);
    assert_eq!(
        pool.state().leaf_count,
        before + 2,
        "two spends, dummy outputs only"
    );
    let r = pool.reveal(&alice.viewing_key).unwrap();
    assert_eq!(r.balance, 0);
    assert!(r.notes.iter().all(|n| n.spent || n.recipient.is_some()));
}

#[test]
fn request_validation_corners() {
    let mut pool = pool_with(&["alice", "bob"]);
    pool.shield("alice", 100, "D1").unwrap();
    assert!(matches!(
        pool.unshield("alice", 0, "rX"),
        Err(Error::ZeroAmount)
    ));
    assert!(matches!(
        pool.transfer("alice", &[out("bob", u64::MAX), out("bob", 1)]),
        Err(Error::AmountOverflow)
    ));
    // Vouching an amount for a transfer: zero is fine, anything else is a caller error.
    let t = pool.build_transfer("alice", &[out("bob", 1)]).unwrap();
    assert!(pool.verify(&t, OpTag::Transfer, "", Some(0)).is_ok());
    assert!(matches!(
        pool.verify(&t, OpTag::Transfer, "", Some(5)),
        Err(Error::VouchedAmountNotApplicable {
            op: OpTag::Transfer,
            amount: 5
        })
    ));
    assert_eq!(pool.state().bundle_count, 1);
}

#[test]
fn malformed_keys_and_roots_are_refused() {
    let mut pool = pool_with(&["alice"]);
    pool.shield("alice", 1, "D1").unwrap();
    assert!(matches!(pool.reveal("zz"), Err(Error::InvalidEncoding)));
    assert!(matches!(pool.reveal("00"), Err(Error::InvalidEncoding)));
    assert!(matches!(
        pool.reveal(&"00".repeat(96)),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        pool.set_anchored_root("zz"),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        pool.set_anchored_root("00"),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        pool.set_anchored_root(&"ff".repeat(32)),
        Err(Error::InvalidEncoding)
    ));
    assert_eq!(pool.state().anchored_root, None);
}

#[test]
fn snapshot_corruption_is_refused() {
    let mut pool = pool_with(&["alice", "bob"]);
    pool.shield("alice", 100, "D1").unwrap();
    pool.transfer("alice", &[out("bob", 60)]).unwrap();
    let snap = pool.snapshot();
    assert_eq!(snap.anchored_root, None);
    let restored = Pool::restore(&snap).unwrap();
    assert_eq!(restored.state().anchored_root, None);
    assert_eq!(restored.state().root, pool.state().root);

    let mut bad = pool.snapshot();
    bad.format = 2;
    assert!(matches!(Pool::restore(&bad), Err(Error::Snapshot(_))));

    let mut bad = pool.snapshot();
    let dup = bad.owners[0].clone();
    bad.owners.push(dup);
    assert!(matches!(Pool::restore(&bad), Err(Error::OwnerExists(_))));

    let mut bad = pool.snapshot();
    bad.owners[0].spending_key = "zz".into();
    assert!(matches!(Pool::restore(&bad), Err(Error::InvalidEncoding)));

    let mut bad = pool.snapshot();
    bad.owners[0].spending_key = "00".into();
    assert!(matches!(Pool::restore(&bad), Err(Error::InvalidEncoding)));

    let mut bad = pool.snapshot();
    bad.anchored_root = Some("00".repeat(32));
    assert!(matches!(Pool::restore(&bad), Err(Error::UnknownAnchor)));

    // A snapshot round-trips through JSON unchanged.
    let json = serde_json::to_string(&snap).unwrap();
    let back: Snapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), json);
}

fn with_record(pool: &Pool, f: impl FnOnce(&mut BundleRecord)) -> Result<(), Error> {
    let mut rec = pool.records()[0].clone();
    f(&mut rec);
    bundle_from_record(&rec, pool.params().version).map(|_| ())
}

#[test]
fn malformed_records_are_refused() {
    let mut pool = pool_with(&["alice"]);
    pool.shield("alice", 1, "D1").unwrap();
    let ff = "ff".repeat(32);

    // Well-formed record rebuilds; `record_from_bundle` is the inverse.
    let bundle = bundle_from_record(&pool.records()[0], pool.params().version).unwrap();
    let again = record_from_bundle(&bundle, 0, OpTag::Shield, "D1", 0, 0);
    assert_eq!(again.actions.len(), pool.records()[0].actions.len());

    // Hex and length errors on every field.
    for field in [
        "nullifier",
        "rk",
        "cmx",
        "cv_net",
        "epk",
        "enc_ciphertext",
        "out_ciphertext",
        "spend_auth_sig",
    ] {
        for bad in ["zz", "00"] {
            let r = with_record(&pool, |rec| {
                let a = &mut rec.actions[0];
                match field {
                    "nullifier" => a.nullifier = bad.into(),
                    "rk" => a.rk = bad.into(),
                    "cmx" => a.cmx = bad.into(),
                    "cv_net" => a.cv_net = bad.into(),
                    "epk" => a.epk = bad.into(),
                    "enc_ciphertext" => a.enc_ciphertext = bad.into(),
                    "out_ciphertext" => a.out_ciphertext = bad.into(),
                    _ => a.spend_auth_sig = bad.into(),
                }
            });
            assert!(matches!(r, Err(Error::InvalidEncoding)), "{field} = {bad}");
        }
    }
    // Non-canonical field elements and points.
    assert!(matches!(
        with_record(&pool, |r| r.actions[0].nullifier = ff.clone()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with_record(&pool, |r| r.actions[0].cmx = ff.clone()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with_record(&pool, |r| r.actions[0].cv_net = ff.clone()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with_record(&pool, |r| r.actions[0].rk = ff.clone()),
        Err(Error::InvalidEncoding)
    ));
    // Identity points are rejected by Orchard's own `Action::from_parts` (rk) / epk check.
    let zero = "00".repeat(32);
    assert!(matches!(
        with_record(&pool, |r| r.actions[0].epk = zero.clone()),
        Err(Error::Build(_) | Error::InvalidEncoding)
    ));
    assert!(matches!(
        with_record(&pool, |r| r.actions[0].rk = zero.clone()),
        Err(Error::Build(_) | Error::InvalidEncoding)
    ));
    // Bundle-level fields.
    assert!(matches!(
        with_record(&pool, |r| r.actions.clear()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with_record(&pool, |r| r.flags = 0x80),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with_record(&pool, |r| r.anchor = "zz".into()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with_record(&pool, |r| r.anchor = ff.clone()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with_record(&pool, |r| r.proof = "zz".into()),
        Err(Error::InvalidEncoding)
    ));
    assert!(matches!(
        with_record(&pool, |r| r.proof.truncate(r.proof.len() - 2)),
        Err(Error::Build(_))
    ));
    assert!(matches!(
        with_record(&pool, |r| r.binding_signature = "00".into()),
        Err(Error::InvalidEncoding)
    ));
}
