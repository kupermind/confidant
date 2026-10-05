//! Offline walk-through of the v2 demo script: two wallets, one verifier, no XRPL.
//!
//! `cargo run --release --example demo`

use std::time::Instant;

use confidant_core::memo::AnchorMemo;
use confidant_core::record::bundle_hash;
use confidant_core::{DepositRef, TransferOutput, Verifier, Wallet};

fn step<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    println!("{label:<44} {:>8.0?}", t.elapsed());
    r
}

fn main() {
    let mut verifier = step("build circuit params + empty verifier", Verifier::new);
    let mut alice = Wallet::new();
    let mut bob = Wallet::new();
    println!("alice address  {}", alice.info().address);
    println!("bob   address  {}", bob.info().address);

    // 1. Shield: the wallet builds first, the deposit memo carries the bundle hash, the verifier
    //    accepts only against that deposit.
    let prepared = step("wallet: build + prove shield 100", || {
        alice.build_shield(100).unwrap()
    });
    let deposit = DepositRef {
        tx_hash: "XRPL-DEPOSIT-TX-HASH".into(),
        delivered: 100,
        memo_hash: prepared.hash_hex(),
    };
    let acc = step("verifier: accept shield (deposit-bound)", || {
        verifier.submit_shield(&prepared.record, &deposit).unwrap()
    });
    println!(
        "  root {}  memo {}",
        acc.new_root,
        verifier.anchor_memo(0).unwrap().encode_hex().unwrap().len() / 2
    );

    // 2. Private transfer.
    alice.sync(verifier.records()).unwrap();
    let prepared = step("wallet: build + prove transfer → bob 60", || {
        alice
            .build_transfer(&[TransferOutput {
                recipient: bob.info().address,
                amount: 60,
            }])
            .unwrap()
    });
    let acc = step("verifier: accept transfer", || {
        verifier.submit_transfer(&prepared.record).unwrap()
    });
    println!("  root {}", acc.new_root);

    // 3. Unshield.
    bob.sync(verifier.records()).unwrap();
    let prepared = step("wallet(bob): build + prove unshield 25", || {
        bob.build_unshield(25, "rFreshRecipientAddress").unwrap()
    });
    let acc = step("verifier: accept unshield", || {
        verifier
            .submit_unshield(&prepared.record, "rFreshRecipientAddress")
            .unwrap()
    });
    println!("  payout {:?}", acc.payout.unwrap());

    // 4. Reveal and 5. a new node rebuilding from the anchor memos + a mirror.
    let report = step("auditor reveal with bob's viewing key", || {
        verifier.reveal(&bob.info().viewing_key).unwrap()
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    let memos: Vec<AnchorMemo> = (0..3).map(|i| verifier.anchor_memo(i).unwrap()).collect();
    let log = verifier.records().to_vec();
    let node = step("new node: rebuild from 3 memos + mirror", || {
        Verifier::rebuild(&memos, &mut |h| {
            log.iter().find(|r| bundle_hash(r).unwrap() == *h).cloned()
        })
        .unwrap()
    });
    println!(
        "  roots match: {}",
        node.state().root == verifier.state().root
    );
}
