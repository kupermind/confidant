//! End-to-end: the SPEC demo script, start to finish, at the core boundary.
//!
//! This is the "definition of done" for the core side, run as one scenario in the order the
//! stage demo runs it: a pre-populated pool (anonymity set) → Alice shields → the root is
//! anchored → Alice pays Bob inside the pool (nothing for the ledger) → the backend restarts →
//! Bob unshields to a fresh address → the auditor reveals exactly Bob → a third party who holds
//! only the public bundle log re-verifies everything and lands on the anchored root.
//!
//! What this cannot cover, by design: the XRPL Payments themselves (deposit, memo anchoring,
//! payout) and the HTTP layer. `docs/E2E.md` lists what the API-level e2e must add on top.

use std::collections::BTreeSet;
use std::time::Instant;

use confidant_core::record::bundle_from_record;
use confidant_core::{BundleRecord, Direction, Error, OpTag, Pool, Snapshot, TransferOutput};

const DENOMINATION: u64 = 100;
const ANONYMITY_SET: usize = 8;

fn pay(recipient: &str, amount: u64) -> TransferOutput {
    TransferOutput {
        recipient: recipient.to_owned(),
        amount,
    }
}

/// What the front-end does after an accepted op: write `root` into a memo Payment on XRPL,
/// then confirm it to the core. Here the ledger is simulated by the returned string.
fn anchor_on_ledger(pool: &mut Pool) -> String {
    let root = pool.state().root;
    pool.set_anchored_root(&root).unwrap();
    assert_eq!(pool.state().anchored_root.as_deref(), Some(root.as_str()));
    root
}

fn is_hex_of(s: &str, bytes: usize) -> bool {
    s.len() == bytes * 2 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[test]
fn demo_script_end_to_end() {
    let mut timings: Vec<(&str, std::time::Duration)> = Vec::new();
    let mut step = |label: &'static str, t: Instant| timings.push((label, t.elapsed()));

    // ---------------------------------------------------------------- pre-event: the pool
    let t = Instant::now();
    let mut pool = Pool::new();
    step("build params + empty pool", t);

    // Pre-populate: many testnet accounts shield a fixed denomination, a few pay each other,
    // so Alice and Bob are not the only leaves on stage.
    let t = Instant::now();
    let accounts: Vec<String> = (1..=ANONYMITY_SET)
        .map(|i| format!("acct-{i:02}"))
        .collect();
    for (i, a) in accounts.iter().enumerate() {
        pool.register_owner(a).unwrap();
        pool.shield(a, DENOMINATION, &format!("XRPL-DEPOSIT-{i:02}"))
            .unwrap();
    }
    pool.transfer("acct-01", &[pay("acct-02", DENOMINATION)])
        .unwrap();
    pool.transfer("acct-03", &[pay("acct-04", 40), pay("acct-05", 60)])
        .unwrap();
    let prepopulated_bundles = pool.state().bundle_count;
    assert_eq!(prepopulated_bundles, ANONYMITY_SET + 2);
    step("pre-populate (8 shields + 2 transfers)", t);

    let alice = pool.register_owner("alice").unwrap();
    let bob = pool.register_owner("bob").unwrap();
    // Onboarding hands the auditor exactly one key per holder.
    let auditor_key_for_bob = bob.viewing_key.clone();
    let auditor_key_for_alice = alice.viewing_key.clone();
    assert!(is_hex_of(&auditor_key_for_bob, 96));
    assert!(is_hex_of(&bob.address, 43));
    assert_ne!(alice.address, bob.address);

    // ---------------------------------------------------------------- 1. shield
    let t = Instant::now();
    let before = pool.state();
    let shield = pool
        .shield("alice", DENOMINATION, "XRPL-TX-ALICE-DEPOSIT")
        .unwrap();
    let after = pool.state();
    step("shield alice 100", t);
    assert_eq!(after.leaf_count, before.leaf_count + 2);
    assert_eq!(after.bundle_count, before.bundle_count + 1);
    assert_ne!(after.root, before.root);
    assert_eq!(shield.new_root, after.root);
    assert!(is_hex_of(&shield.commitment, 32));
    assert!(shield.leaf_index >= before.leaf_count && shield.leaf_index < after.leaf_count);
    assert_eq!(pool.balance("alice").unwrap(), DENOMINATION);
    // The shield's public record names the deposit it is bound to, and nothing about the note.
    let rec = &pool.records()[shield.bundle_index];
    assert_eq!(rec.op, OpTag::Shield);
    assert_eq!(rec.external, "XRPL-TX-ALICE-DEPOSIT");
    assert_eq!(rec.value_balance, -(DENOMINATION as i64));

    // Anchor the root on the ledger (memo Payment) and confirm it.
    let anchored_after_shield = anchor_on_ledger(&mut pool);

    // ---------------------------------------------------------------- 2. private transfer
    let t = Instant::now();
    let transfer = pool.transfer("alice", &[pay("bob", DENOMINATION)]).unwrap();
    step("private transfer alice → bob 100", t);
    assert_eq!(pool.balance("alice").unwrap(), 0);
    assert_eq!(pool.balance("bob").unwrap(), DENOMINATION);
    assert_eq!(transfer.nullifiers.len(), 2);
    assert_eq!(transfer.new_commitments.len(), 2);

    // "Here's the ledger… nothing." The public record of the hop carries no sender, recipient
    // or amount: a zero value balance, no external context, and only commitments, nullifiers,
    // randomized keys, hiding value commitments, ciphertexts and the proof.
    let hop: &BundleRecord = &pool.records()[transfer.bundle_index];
    assert_eq!(hop.op, OpTag::Transfer);
    assert_eq!(hop.external, "");
    assert_eq!(hop.value_balance, 0);
    assert_eq!(
        hop.actions.len(),
        2,
        "padded to the minimum, same shape as any other hop"
    );
    for a in &hop.actions {
        assert!(is_hex_of(&a.nullifier, 32));
        assert!(is_hex_of(&a.cmx, 32));
        assert!(is_hex_of(&a.cv_net, 32));
        assert!(is_hex_of(&a.rk, 32));
        assert!(is_hex_of(&a.epk, 32));
        assert!(is_hex_of(&a.enc_ciphertext, 580));
        assert!(is_hex_of(&a.out_ciphertext, 80));
        assert!(is_hex_of(&a.spend_auth_sig, 64));
    }
    let hop_json = serde_json::to_string(hop).unwrap();
    for word in ["alice", "bob", &alice.address, &bob.address] {
        assert!(!hop_json.contains(word), "public record leaks `{word}`");
    }
    // The hop is indistinguishable in shape from the pre-populated hops.
    let shape = |r: &BundleRecord| (r.op, r.actions.len(), r.value_balance, r.external.clone());
    let prepop_hop = pool
        .records()
        .iter()
        .find(|r| r.op == OpTag::Transfer && r.index < prepopulated_bundles)
        .unwrap();
    assert_eq!(shape(hop), shape(prepop_hop));

    // Nothing hit the ledger for the hop: the anchored root is still the post-shield one until
    // the front-end decides to anchor again.
    assert_eq!(
        pool.state().anchored_root.as_deref(),
        Some(anchored_after_shield.as_str())
    );
    anchor_on_ledger(&mut pool);

    // ---------------------------------------------------------------- backend restart
    let t = Instant::now();
    let snapshot_json = serde_json::to_string(&pool.snapshot()).unwrap();
    let snapshot: Snapshot = serde_json::from_str(&snapshot_json).unwrap();
    let state_before = serde_json::to_string(&pool.state()).unwrap();
    let mut pool = Pool::restore(&snapshot).unwrap();
    step("backend restart (snapshot → restore)", t);
    assert_eq!(serde_json::to_string(&pool.state()).unwrap(), state_before);
    assert_eq!(pool.balance("bob").unwrap(), DENOMINATION);
    assert_eq!(
        pool.owner_info("bob").unwrap().viewing_key,
        auditor_key_for_bob
    );

    // ---------------------------------------------------------------- 3. unshield
    let t = Instant::now();
    let unshield = pool
        .unshield("bob", DENOMINATION, "rBobFreshXrplAddress")
        .unwrap();
    step("unshield bob 100 → fresh address", t);
    assert_eq!(unshield.payout.amount, DENOMINATION);
    assert_eq!(unshield.payout.destination, "rBobFreshXrplAddress");
    assert_eq!(pool.balance("bob").unwrap(), 0);
    let exit = &pool.records()[unshield.bundle_index];
    assert_eq!(exit.op, OpTag::Unshield);
    assert_eq!(exit.external, "rBobFreshXrplAddress");
    assert_eq!(exit.value_balance, DENOMINATION as i64);
    // Fixed denomination: Bob's exit is one of many identical-looking 100s, not 1:1 linkable to
    // Alice's deposit by amount alone.
    let hundreds_in = pool
        .records()
        .iter()
        .filter(|r| r.op == OpTag::Shield && r.value_balance == -(DENOMINATION as i64))
        .count();
    assert_eq!(hundreds_in, ANONYMITY_SET + 1);
    let final_root = anchor_on_ledger(&mut pool);

    // ---------------------------------------------------------------- 4. selective disclosure
    let t = Instant::now();
    let bob_report = pool.reveal(&auditor_key_for_bob).unwrap();
    step("auditor reveal (bob)", t);
    // Exactly Bob: one incoming 100 (now spent), balance 0, nothing else from the whole pool.
    assert_eq!(bob_report.balance, 0);
    assert_eq!(bob_report.notes.len(), 1);
    assert_eq!(bob_report.notes[0].direction, Direction::Incoming);
    assert_eq!(bob_report.notes[0].amount, DENOMINATION);
    assert!(bob_report.notes[0].spent);
    assert_eq!(bob_report.notes[0].bundle_index, transfer.bundle_index);

    // Alice's key tells her side of the same hop: her deposit (spent) and the 100 she paid Bob.
    let alice_report = pool.reveal(&auditor_key_for_alice).unwrap();
    assert_eq!(alice_report.balance, 0);
    let mut kinds: Vec<(Direction, u64, bool)> = alice_report
        .notes
        .iter()
        .map(|n| (n.direction, n.amount, n.spent))
        .collect();
    kinds.sort_by_key(|k| (k.0 == Direction::Outgoing, k.1));
    assert_eq!(
        kinds,
        vec![
            (Direction::Incoming, DENOMINATION, true),
            (Direction::Outgoing, DENOMINATION, false)
        ]
    );
    let outgoing = alice_report
        .notes
        .iter()
        .find(|n| n.direction == Direction::Outgoing)
        .unwrap();
    assert_eq!(outgoing.recipient.as_deref(), Some(bob.address.as_str()));

    // A pre-populated account's key sees only its own activity, none of Alice's or Bob's.
    let bystander = pool.owner_info("acct-06").unwrap();
    let bystander_report = pool.reveal(&bystander.viewing_key).unwrap();
    assert_eq!(bystander_report.balance, DENOMINATION);
    assert_eq!(bystander_report.notes.len(), 1);
    let touched: BTreeSet<usize> = bystander_report
        .notes
        .iter()
        .map(|n| n.bundle_index)
        .collect();
    assert!(!touched.contains(&shield.bundle_index));
    assert!(!touched.contains(&transfer.bundle_index));
    assert!(!touched.contains(&unshield.bundle_index));

    // ---------------------------------------------------------------- 5. anyone re-verifies
    // A third party holds only the public log (no keys, no plaintext) and the anchored root.
    let t = Instant::now();
    let log_json = serde_json::to_string(pool.records()).unwrap();
    let log: Vec<BundleRecord> = serde_json::from_str(&log_json).unwrap();
    assert!(!log_json.contains("spendingKey"));
    let mut verifier = Pool::new();
    for record in &log {
        let bundle = bundle_from_record(record, verifier.params().version).unwrap();
        if record.index == unshield.bundle_index {
            // Before accepting Bob's exit: the published proof does not verify for any other
            // destination or amount (the sighash binds both).
            assert!(matches!(
                verifier.verify(
                    &bundle,
                    OpTag::Unshield,
                    "rAttackerAddress",
                    Some(DENOMINATION)
                ),
                Err(Error::InvalidSpendAuth)
            ));
            assert!(matches!(
                verifier.verify(
                    &bundle,
                    OpTag::Unshield,
                    &record.external,
                    Some(DENOMINATION + 1)
                ),
                Err(Error::ValueBalance { .. })
            ));
        }
        let acc = verifier
            .submit(bundle, record.op, &record.external, None)
            .unwrap();
        assert_eq!(acc.first_leaf, record.first_leaf);
    }
    step("third-party replay of the whole log", t);
    assert_eq!(verifier.state().root, final_root);
    assert_eq!(verifier.state().leaf_count, pool.state().leaf_count);
    assert_eq!(
        verifier.state().nullifier_count,
        pool.state().nullifier_count
    );
    assert_eq!(verifier.anchor_history(), pool.anchor_history());

    // The verifier refuses a tampered log entry and a replayed one.
    let mut tampered = log[transfer.bundle_index].clone();
    tampered.value_balance = 1;
    let bundle = bundle_from_record(&tampered, verifier.params().version).unwrap();
    assert!(matches!(
        verifier.submit(bundle, tampered.op, &tampered.external, None),
        Err(Error::ValueBalanceSign { .. })
    ));
    let replay =
        bundle_from_record(&log[unshield.bundle_index], verifier.params().version).unwrap();
    assert!(matches!(
        verifier.submit(
            replay,
            OpTag::Unshield,
            "rBobFreshXrplAddress",
            Some(DENOMINATION)
        ),
        Err(Error::DoubleSpend)
    ));
    // The verifier has no keys: the auditor key still reveals exactly Bob from the public log
    // (timestamps differ — each party records its own acceptance time).
    let from_log = verifier.reveal(&auditor_key_for_bob).unwrap();
    let shape = |r: &confidant_core::RevealReport| {
        (
            r.balance,
            r.notes
                .iter()
                .map(|n| (n.direction, n.amount, n.leaf_index, n.bundle_index, n.spent))
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(shape(&from_log), shape(&bob_report));

    // ---------------------------------------------------------------- bookkeeping
    let s = pool.state();
    assert_eq!(s.bundle_count, prepopulated_bundles + 3);
    assert_eq!(
        s.leaf_count,
        2 * s.bundle_count as u64,
        "every bundle padded to two actions"
    );
    assert_eq!(s.nullifier_count, s.leaf_count as usize);
    assert_eq!(pool.anchor_history().len(), s.bundle_count + 1);

    eprintln!("\n{:<44} {:>10}", "e2e demo step", "elapsed");
    for (label, d) in &timings {
        eprintln!("{label:<44} {d:>10.2?}");
    }
}
