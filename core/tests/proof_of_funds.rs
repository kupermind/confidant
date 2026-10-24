//! Proof of shielded funds (hackathon feature C, core slice): a holder proves control of unspent
//! notes worth ≥ X, bound to a challenge, with the Orchard circuit unchanged. Both directions are
//! pinned: a proof of funds is refused by every apply path *before any signature check*, and a pool
//! record is refused by the proof-of-funds check. Built on-site from the reviewed design
//! (`docs/PROOF-OF-FUNDS.md`, audit conditions 2–4; the pre-event spike, PR #29, is the reference).

mod common;

use common::*;
use confidant_core::memo::AnchorMemo;
use confidant_core::record::bundle_from_record;
use confidant_core::record::bundle_hash;
use confidant_core::{DepositRef, Error, OpTag, Submission, Verifier, Wallet, BUNDLE_VERSION};

fn arr(hex_str: &str) -> [u8; 32] {
    hex::decode(hex_str).unwrap().try_into().unwrap()
}

const CH: &[u8] = b"lender-nonce-42|rBorrower|ledger 1234|expiry 1300";

/// Flips one hex digit in `s` at `at` so the string stays valid hex but the bytes change.
fn flip_hex(s: &str, at: usize) -> String {
    let mut b = s.as_bytes().to_vec();
    b[at] = if b[at] == b'0' { b'1' } else { b'0' };
    String::from_utf8(b).unwrap()
}

#[test]
fn proof_of_funds_is_bound_and_read_only_and_never_a_pool_operation() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    shield(&mut v, &mut alice, 50, "D2");
    shield(&mut v, &mut bob, 30, "D3");
    alice.sync(v.records()).unwrap();
    bob.sync(v.records()).unwrap();
    let snap = |v: &Verifier| {
        let s = v.state();
        (
            s.root,
            s.bundle_count,
            s.nullifier_count,
            s.leaf_count,
            s.consumed_deposits,
        )
    };
    let before = snap(&v);

    // Build: the record is tagged as a proof of funds; it reveals the threshold, not the balance.
    let p = alice.build_proof_of_funds(120, CH).unwrap();
    assert_eq!(p.record.op, OpTag::ProofOfFunds);
    assert_eq!(p.record.value_balance, 120);
    assert!(matches!(
        alice.build_proof_of_funds(0, CH),
        Err(Error::ZeroAmount)
    ));
    assert!(matches!(
        alice.build_proof_of_funds(151, CH),
        Err(Error::InsufficientFunds { .. })
    ));

    // Verify: read-only, bound to the challenge and the amount.
    v.verify_proof_of_funds(&p.record, 120, CH).unwrap();
    assert_eq!(snap(&v), before, "verification mutates nothing");
    assert!(matches!(
        v.verify_proof_of_funds(&p.record, 120, b"another challenge"),
        Err(Error::InvalidSpendAuth)
    ));
    // ...including a challenge of the same length with one byte changed: the bytes are bound, not
    // just their length.
    let mut near = CH.to_vec();
    near[0] ^= 1;
    assert!(matches!(
        v.verify_proof_of_funds(&p.record, 120, &near),
        Err(Error::InvalidSpendAuth)
    ));
    assert!(matches!(
        v.verify_proof_of_funds(&p.record, 119, CH),
        Err(Error::ValueBalance {
            expected: 119,
            actual: 120
        })
    ));
    assert!(matches!(
        v.verify_proof_of_funds(&p.record, 0, CH),
        Err(Error::ValueBalance { expected: 0, .. })
    ));

    // Direction 1: a proof of funds handed to any apply path is refused by its op, before any
    // signature is looked at — the error is NotAPoolOperation, never a signature error.
    assert!(matches!(
        v.submit_unshield(&p.record, "rLender"),
        Err(Error::NotAPoolOperation)
    ));
    assert!(matches!(
        v.submit_transfer(&p.record),
        Err(Error::NotAPoolOperation)
    ));
    let mut as_next = p.record.clone();
    as_next.index = v.records().len();
    assert!(matches!(v.replay(&as_next), Err(Error::NotAPoolOperation)));
    // Endorse: a proposed memo that matches the submission on op, context and hash still fails,
    // because a proof of funds cannot become an anchor memo.
    let proposed = AnchorMemo {
        op: OpTag::Unshield,
        root: [0u8; 32],
        bundle_hash: bundle_hash(&p.record).unwrap(),
        external: "rLender".into(),
        actions: Vec::new(),
    };
    let sub = Submission::Unshield {
        record: p.record.clone(),
        destination: "rLender".into(),
    };
    assert!(matches!(
        v.endorse(&sub, &proposed),
        Err(Error::MemoMalformed("not a pool operation"))
    ));
    let dep = DepositRef {
        tx_hash: "DX".into(),
        delivered: 120,
        memo_hash: p.hash_hex(),
    };
    assert!(v.submit_shield(&p.record, &dep).is_err());
    assert!(matches!(
        AnchorMemo::from_record(&p.record, [0u8; 32]),
        Err(Error::MemoMalformed("not a pool operation"))
    ));
    // The public verify() refuses the op by itself, for callers that decode a bundle first.
    let decoded = bundle_from_record(&p.record, BUNDLE_VERSION).unwrap();
    assert!(matches!(
        v.verify(&decoded, OpTag::ProofOfFunds, "", None),
        Err(Error::NotAPoolOperation)
    ));
    // ...and a memo read back from the ledger with the proof-of-funds op byte is refused.
    let mut bytes = v.anchor_memo(0).unwrap().encode().unwrap();
    assert_eq!(bytes[1], OpTag::Shield as u8);
    bytes[1] = OpTag::ProofOfFunds as u8;
    assert!(matches!(
        AnchorMemo::decode(&bytes),
        Err(Error::MemoMalformed("not a pool operation"))
    ));
    assert_eq!(
        snap(&v),
        before,
        "nothing was applied by any of the refusals"
    );

    // Direction 2: pool records are not proofs of funds.
    let u = alice.build_unshield(10, "rAlice").unwrap();
    assert!(matches!(
        v.verify_proof_of_funds(&u.record, 10, CH),
        Err(Error::NotAProofOfFunds)
    ));
    assert!(matches!(
        v.verify_proof_of_funds(&v.records()[0].clone(), 100, CH),
        Err(Error::NotAProofOfFunds)
    ));

    // Tampering: proof, spend authorization, binding signature, a duplicated action.
    let mut bad_proof = p.record.clone();
    bad_proof.proof = flip_hex(&bad_proof.proof, 8);
    assert!(matches!(
        v.verify_proof_of_funds(&bad_proof, 120, CH),
        Err(Error::InvalidProof)
    ));
    let mut bad_auth = p.record.clone();
    bad_auth.actions[0].spend_auth_sig = flip_hex(&bad_auth.actions[0].spend_auth_sig, 8);
    assert!(matches!(
        v.verify_proof_of_funds(&bad_auth, 120, CH),
        Err(Error::InvalidSpendAuth)
    ));
    let mut bad_binding = p.record.clone();
    bad_binding.binding_signature = flip_hex(&bad_binding.binding_signature, 8);
    assert!(matches!(
        v.verify_proof_of_funds(&bad_binding, 120, CH),
        Err(Error::InvalidBindingSignature)
    ));
    let mut dup = p.record.clone();
    let first = dup.actions[0].clone();
    dup.actions[1] = first;
    assert!(matches!(
        v.verify_proof_of_funds(&dup, 120, CH),
        Err(Error::DoubleSpend)
    ));

    // A keyless third party rebuilt from the public log reaches the same verdict; a verifier that
    // does not know the anchor refuses.
    let mut auditor = Verifier::new();
    for r in v.records() {
        auditor.replay(r).unwrap();
    }
    auditor.verify_proof_of_funds(&p.record, 120, CH).unwrap();
    assert!(matches!(
        Verifier::new().verify_proof_of_funds(&p.record, 120, CH),
        Err(Error::UnknownAnchor)
    ));

    // Bob proves his 30 and cannot prove 31.
    let pb = bob.build_proof_of_funds(30, CH).unwrap();
    v.verify_proof_of_funds(&pb.record, 30, CH).unwrap();
    assert!(bob.build_proof_of_funds(31, CH).is_err());

    // A snapshot, not a lock: once the proven notes move, the old proof fails.
    unshield(&mut v, &mut alice, 140, "rAlice");
    assert!(matches!(
        v.verify_proof_of_funds(&p.record, 120, CH),
        Err(Error::DoubleSpend)
    ));
}

/// Audit condition 2 on the ledger-driven path, and the amount arm on its own. (1) A proof-of-funds
/// record served by a mirror under a genuine-looking pool memo (a memo naming a pool op but the
/// proof's bundle hash) is refused by TYPE at `rebuild` and `rebuild_checked` — never left to the
/// signature domain — and the state stays empty. (2) A value-balance-zero bundle (a transfer to self)
/// relabelled as a proof of funds is refused by amount, before any signature work.
#[test]
fn a_proof_of_funds_in_the_mirror_is_refused_by_type_at_rebuild_and_a_zero_amount_by_amount() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    alice.sync(v.records()).unwrap();
    let p = alice.build_proof_of_funds(60, CH).unwrap();
    let pof_hash = bundle_hash(&p.record).unwrap();

    // The genuine memo for the log, then a memo that claims an unshield but names the proof's hash.
    let genuine = v.anchor_memo(0).unwrap();
    let forged = AnchorMemo {
        op: OpTag::Unshield,
        root: [0u8; 32],
        bundle_hash: pof_hash,
        external: "rAlice".into(),
        actions: p
            .record
            .actions
            .iter()
            .map(|a| (arr(&a.nullifier), arr(&a.cmx)))
            .collect(),
    };
    let log = v.records().to_vec();
    let mut mirror = |h: &[u8; 32]| {
        if *h == pof_hash {
            return Some(p.record.clone());
        }
        log.iter().find(|r| bundle_hash(r).unwrap() == *h).cloned()
    };
    // Sanity: the genuine memo alone rebuilds the state.
    assert_eq!(
        Verifier::rebuild(std::slice::from_ref(&genuine), &mut mirror)
            .unwrap()
            .state()
            .root,
        v.state().root
    );
    // The forged entry after the genuine log: refused by type, not by signature.
    assert!(matches!(
        Verifier::rebuild(&[genuine.clone(), forged.clone()], &mut mirror),
        Err(Error::NotAPoolOperation)
    ));
    // The checked rebuild refuses the same way; the forged entry first means the deposit lookup
    // is never even consulted (it is only asked about shield memos).
    let mut lookup =
        |_: &str| -> Option<DepositRef> { unreachable!("no shield memo reaches the lookup") };
    assert!(matches!(
        Verifier::rebuild_checked(&[forged], &mut mirror, &mut lookup),
        Err(Error::NotAPoolOperation)
    ));

    // amount > 0 is pinned on its own: value balance 0 == amount 0 is refused by amount.
    let mut zero = alice
        .build_transfer(&[out(&alice.info().address, 40)])
        .unwrap()
        .record;
    assert_eq!(zero.value_balance, 0);
    zero.op = OpTag::ProofOfFunds;
    assert!(matches!(
        v.verify_proof_of_funds(&zero, 0, CH),
        Err(Error::ValueBalance {
            expected: 0,
            actual: 0
        })
    ));
    assert!(matches!(
        v.verify_proof_of_funds(&zero, 40, CH),
        Err(Error::ValueBalance {
            expected: 40,
            actual: 0
        })
    ));
    assert_eq!(v.state().bundle_count, 1, "nothing was applied");
}
