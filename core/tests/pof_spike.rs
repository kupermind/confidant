//! SPIKE (not for merge): proof of shielded funds with the Orchard circuit unchanged.

mod common;

use std::time::Instant;

use common::*;
use confidant_core::{Error, Verifier, Wallet};

#[test]
fn proof_of_funds_spike() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    shield(&mut v, &mut alice, 50, "D2");
    shield(&mut v, &mut bob, 30, "D3");
    alice.sync(v.records()).unwrap();
    let count = v.state().bundle_count;

    let ch = b"lender-nonce-42|rBorrower|ledger 1234";
    let t = Instant::now();
    let p = alice.build_proof_of_funds(120, ch).unwrap();
    let prove = t.elapsed();
    let t = Instant::now();
    v.verify_proof_of_funds(&p.record, 120, ch).unwrap();
    let verify = t.elapsed();
    let json = serde_json::to_vec(&p.record).unwrap();
    println!(
        "PoF 120 of 150: prove {prove:?}, verify {verify:?}, actions {}, record json {} B",
        p.record.actions.len(),
        json.len()
    );

    // Reveals only the threshold: value balance is 120, not Alice's 150.
    assert_eq!(p.record.value_balance, 120);
    // Bound to the challenge and the amount.
    assert!(matches!(
        v.verify_proof_of_funds(&p.record, 120, b"other"),
        Err(Error::InvalidSpendAuth)
    ));
    assert!(matches!(
        v.verify_proof_of_funds(&p.record, 119, ch),
        Err(Error::ValueBalance { .. })
    ));
    // The pool never accepts it, whatever the lender does with it.
    assert!(matches!(
        v.submit_unshield(&p.record, "rLender"),
        Err(Error::InvalidSpendAuth)
    ));
    assert!(v.submit_transfer(&p.record).is_err());
    assert_eq!(v.state().bundle_count, count, "nothing applied");
    // Cannot claim more than held.
    assert!(matches!(
        alice.build_proof_of_funds(151, ch),
        Err(Error::InsufficientFunds { .. })
    ));
    // Bob's proof for his 30 works; for 31 he cannot build one.
    bob.sync(v.records()).unwrap();
    let pb = bob.build_proof_of_funds(30, ch).unwrap();
    v.verify_proof_of_funds(&pb.record, 30, ch).unwrap();
    assert!(bob.build_proof_of_funds(31, ch).is_err());

    // A third party with only the public log reaches the same verdict.
    let mut auditor = Verifier::new();
    for r in v.records() {
        auditor.replay(r).unwrap();
    }
    auditor.verify_proof_of_funds(&p.record, 120, ch).unwrap();

    // Once the proven notes are spent, the stale proof fails (nullifier now in the set).
    unshield(&mut v, &mut alice, 140, "rAlice");
    assert!(matches!(
        v.verify_proof_of_funds(&p.record, 120, ch),
        Err(Error::DoubleSpend)
    ));
}
