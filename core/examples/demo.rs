//! Offline walk-through of the demo script: shield → private transfer → unshield → auditor
//! reveal, with timings. No XRPL involved; this is the core alone.
//!
//! `cargo run --release --example demo`

use std::time::Instant;

use confidant_core::{Pool, TransferOutput};

fn step<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    println!("{label:<34} {:>8.0?}", t.elapsed());
    out
}

fn main() {
    let mut pool = step("build circuit params + empty pool", Pool::new);
    let alice = pool.register_owner("alice").unwrap();
    let bob = pool.register_owner("bob").unwrap();
    println!("alice address  {}", alice.address);
    println!("bob   address  {}", bob.address);

    let shield = step("shield 100 → alice", || {
        pool.shield("alice", 100, "XRPL-DEPOSIT-TX-HASH").unwrap()
    });
    println!("  new root {}", shield.new_root);

    let transfer = step("private transfer alice → bob 60", || {
        pool.transfer(
            "alice",
            &[TransferOutput {
                recipient: "bob".into(),
                amount: 60,
            }],
        )
        .unwrap()
    });
    println!("  nullifiers {:?}", transfer.nullifiers);
    println!("  new root   {}", transfer.new_root);

    let unshield = step("unshield bob 25 → fresh XRPL address", || {
        pool.unshield("bob", 25, "rFreshRecipientAddress").unwrap()
    });
    println!("  payout {:?}", unshield.payout);
    println!("  new root {}", unshield.new_root);

    let state = pool.state();
    println!(
        "\npool state: {}",
        serde_json::to_string_pretty(&state).unwrap()
    );

    let report = step("auditor reveal with bob's viewing key", || {
        pool.reveal(&bob.viewing_key).unwrap()
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());

    let verify_all = step("third-party re-verification of the log", || {
        let fresh = Pool::new();
        let mut fresh_pool = fresh;
        for record in pool.records() {
            let bundle =
                confidant_core::record::bundle_from_record(record, pool.params().version).unwrap();
            fresh_pool
                .submit(bundle, record.op, &record.external, None)
                .unwrap();
        }
        fresh_pool.state().root == state.root
    });
    println!("  roots match: {verify_all}");
}
