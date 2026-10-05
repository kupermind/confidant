//! End-to-end (v2): the SPEC demo script at the core boundary with the trust split in place —
//! wallets build and prove, a verifier accepts, anchor memos are produced per op, a second
//! verifier fed the same log agrees, a new node rebuilds from memos + mirror, and a keyless
//! third party replays the public log. `docs/E2E.md` lists what the testnet harness adds.

mod common;

use std::time::Instant;

use common::*;
use confidant_core::memo::AnchorMemo;
use confidant_core::record::{bundle_from_record, bundle_hash};
use confidant_core::{BundleRecord, Direction, Error, OpTag, Verifier, Wallet};

const DENOMINATION: u64 = 100;
const ANONYMITY_SET: usize = 6;

fn is_hex_of(s: &str, bytes: usize) -> bool {
    s.len() == bytes * 2 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[test]
fn demo_script_end_to_end() {
    let mut timings: Vec<(&str, std::time::Duration)> = Vec::new();
    let mut step = |label: &'static str, t: Instant| timings.push((label, t.elapsed()));

    let t = Instant::now();
    let mut v = Verifier::new();
    // Two independent signers' verifiers: they see the same submissions and must agree.
    let mut signer2 = Verifier::new();
    step("build params + verifiers", t);

    // Pre-populate: many wallets shield a fixed denomination, a few pay each other.
    let t = Instant::now();
    let mut accounts: Vec<Wallet> = (0..ANONYMITY_SET).map(|_| Wallet::new()).collect();
    for (i, w) in accounts.iter_mut().enumerate() {
        w.sync(v.records()).unwrap();
        let p = w.build_shield(DENOMINATION).unwrap();
        v.submit_shield(
            &p.record,
            &deposit_for(&p, &format!("XRPL-DEPOSIT-{i:02}"), DENOMINATION),
        )
        .unwrap();
    }
    for w in accounts.iter_mut() {
        w.sync(v.records()).unwrap();
    }
    let a1 = accounts[1].info().address;
    let p = accounts[0]
        .build_transfer(&[out(&a1, DENOMINATION)])
        .unwrap();
    v.submit_transfer(&p.record).unwrap();
    let a3 = accounts[3].info().address;
    let a4 = accounts[4].info().address;
    let p = accounts[2]
        .build_transfer(&[out(&a3, 40), out(&a4, 60)])
        .unwrap();
    v.submit_transfer(&p.record).unwrap();
    let prepopulated = v.state().bundle_count;
    assert_eq!(prepopulated, ANONYMITY_SET + 2);
    step("pre-populate (6 shields + 2 transfers)", t);

    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    let auditor_key_for_bob = bob.info().viewing_key;
    assert!(is_hex_of(&auditor_key_for_bob, 96));
    assert!(is_hex_of(&bob.info().address, 43));

    // 1. Shield — wallet first, deposit memo = bundle hash, verifier accepts against the deposit.
    let t = Instant::now();
    alice.sync(v.records()).unwrap();
    let prepared = alice.build_shield(DENOMINATION).unwrap();
    let deposit = deposit_for(&prepared, "XRPL-TX-ALICE-DEPOSIT", DENOMINATION);
    let before = v.state();
    let shield = v.submit_shield(&prepared.record, &deposit).unwrap();
    alice.sync(v.records()).unwrap();
    step("shield alice 100 (wallet build + verifier accept)", t);
    assert_eq!(v.state().leaf_count, before.leaf_count + 2);
    assert_eq!(shield.bundle_hash, prepared.hash_hex());
    assert_eq!(alice.balance(), DENOMINATION);
    assert!(v.is_deposit_consumed("XRPL-TX-ALICE-DEPOSIT"));
    // The anchor memo for the shield names the deposit and the root; ≤ 1 KB.
    let memo = v.anchor_memo(shield.bundle_index).unwrap();
    assert_eq!(memo.op, OpTag::Shield);
    assert_eq!(memo.external, "XRPL-TX-ALICE-DEPOSIT");
    assert_eq!(hex::encode(memo.root), shield.new_root);
    assert!(memo.encode().unwrap().len() <= 1024);

    // 2. Private transfer — the public record leaks nothing and matches every other hop's shape.
    let t = Instant::now();
    let prepared = alice
        .build_transfer(&[out(&bob.info().address, DENOMINATION)])
        .unwrap();
    let transfer = v.submit_transfer(&prepared.record).unwrap();
    alice.sync(v.records()).unwrap();
    bob.sync(v.records()).unwrap();
    step("private transfer alice → bob 100", t);
    assert_eq!(alice.balance(), 0);
    assert_eq!(bob.balance(), DENOMINATION);
    let hop: &BundleRecord = &v.records()[transfer.bundle_index];
    assert_eq!(hop.external, "");
    assert_eq!(hop.value_balance, 0);
    assert_eq!(hop.actions.len(), 2);
    let hop_json = serde_json::to_string(hop).unwrap();
    for word in [alice.info().address, bob.info().address] {
        assert!(!hop_json.contains(&word), "public record leaks an address");
    }
    let shape = |r: &BundleRecord| (r.op, r.actions.len(), r.value_balance, r.external.clone());
    let prepop_hop = v
        .records()
        .iter()
        .find(|r| r.op == OpTag::Transfer && r.index < prepopulated)
        .unwrap();
    assert_eq!(shape(hop), shape(prepop_hop));

    // Signer 2 catches up from the public log and agrees on every memo so far.
    let t = Instant::now();
    for record in v.records() {
        signer2.replay(record).unwrap();
    }
    for i in 0..v.state().bundle_count {
        signer2.check_memo(i, &v.anchor_memo(i).unwrap()).unwrap();
    }
    assert_eq!(signer2.state().root, v.state().root);
    step("second signer replays the log and agrees", t);

    // 3. Unshield — payout bound to destination and amount.
    let t = Instant::now();
    let prepared = bob
        .build_unshield(DENOMINATION, "rBobFreshXrplAddress")
        .unwrap();
    let unshield = v
        .submit_unshield(&prepared.record, "rBobFreshXrplAddress")
        .unwrap();
    bob.sync(v.records()).unwrap();
    step("unshield bob 100 → fresh address", t);
    let payout = unshield.payout.clone().unwrap();
    assert_eq!(payout.amount, DENOMINATION);
    assert_eq!(payout.destination, "rBobFreshXrplAddress");
    assert_eq!(bob.balance(), 0);
    // A signer that is asked to sign the payout for a different destination refuses.
    let mut rogue_request = prepared.record.clone();
    rogue_request.external = "rAttacker".into();
    assert!(matches!(
        signer2.submit_unshield(&rogue_request, "rAttacker"),
        Err(Error::InvalidSpendAuth)
    ));
    signer2
        .submit_unshield(&prepared.record, "rBobFreshXrplAddress")
        .unwrap();
    assert_eq!(signer2.state().root, v.state().root);

    // 4. Selective disclosure — exactly Bob.
    let t = Instant::now();
    let bob_report = v.reveal(&auditor_key_for_bob).unwrap();
    step("auditor reveal (bob)", t);
    assert_eq!(bob_report.balance, 0);
    assert_eq!(bob_report.notes.len(), 1);
    assert_eq!(bob_report.notes[0].direction, Direction::Incoming);
    assert_eq!(bob_report.notes[0].amount, DENOMINATION);
    assert!(bob_report.notes[0].spent);
    let bystander = v.reveal(&accounts[5].info().viewing_key).unwrap();
    assert_eq!(bystander.notes.len(), 1);
    assert_eq!(bystander.balance, DENOMINATION);

    // 5. A new node rebuilds from the ledger's memos + a mirror; a keyless third party replays.
    let t = Instant::now();
    let memos: Vec<AnchorMemo> = (0..v.state().bundle_count)
        .map(|i| v.anchor_memo(i).unwrap())
        .collect();
    let mirror_log: Vec<BundleRecord> =
        serde_json::from_str(&serde_json::to_string(v.records()).unwrap()).unwrap();
    assert!(!serde_json::to_string(&mirror_log)
        .unwrap()
        .contains("spendingKey"));
    let node = Verifier::rebuild(&memos, |h| {
        mirror_log
            .iter()
            .find(|r| bundle_hash(r).unwrap() == *h)
            .cloned()
    })
    .unwrap();
    step("new node: rebuild from memos + mirror", t);
    assert_eq!(node.state().root, v.state().root);
    assert_eq!(node.anchor_history(), v.anchor_history());
    let from_log = node.reveal(&auditor_key_for_bob).unwrap();
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
    // Tampered / replayed entries are refused by any verifier.
    let mut tampered = mirror_log[transfer.bundle_index].clone();
    tampered.value_balance = 1;
    assert!(matches!(
        Verifier::new().verify(
            &bundle_from_record(&tampered, v.params().version).unwrap(),
            OpTag::Transfer,
            "",
            None
        ),
        Err(Error::ValueBalanceSign { .. })
    ));
    let mut fresh = Verifier::new();
    for r in &mirror_log {
        fresh.replay(r).unwrap();
    }
    assert!(matches!(
        fresh.submit_unshield(&mirror_log[unshield.bundle_index], "rBobFreshXrplAddress"),
        Err(Error::DoubleSpend)
    ));

    let s = v.state();
    assert_eq!(s.bundle_count, prepopulated + 3);
    assert_eq!(s.leaf_count, 2 * s.bundle_count as u64);
    assert_eq!(s.consumed_deposits, ANONYMITY_SET + 1);

    eprintln!("\n{:<48} {:>10}", "e2e demo step (v2)", "elapsed");
    for (label, d) in &timings {
        eprintln!("{label:<48} {d:>10.2?}");
    }
}
