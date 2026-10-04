# `confidant-core` — how the API layer uses it

The Rust core is XRPL-agnostic and HTTP-agnostic. The backend (axum, owned by the API/XRPL
lead) holds one `confidant_core::Pool` behind a mutex and maps the five SPEC endpoints onto it.
Every public type is `serde` camelCase, so responses can be returned as-is.

```toml
# backend/Cargo.toml
confidant-core = { path = "../core" }
```

```rust
use confidant_core::{Pool, TransferOutput};

let mut pool = Pool::new();                   // builds the Orchard proving/verifying keys once (~2.5 s)
let alice = pool.register_owner("alice")?;    // OwnerInfo { owner, address, viewingKey }
```

| SPEC endpoint | Core call | Notes |
| --- | --- | --- |
| `POST /shield` `{amount, owner, depositTxHash}` | `pool.shield(owner, amount, deposit_tx_hash)` → `ShieldReceipt {commitment, leafIndex, newRoot, bundleIndex, proof}` | Call **after** the deposit Payment is validated on XRPL. The core takes the (amount, deposit) pairing on your word — backing is operator-enforced. |
| `POST /transfer` `{owner, outputs:[{recipient, amount}]}` | `pool.transfer(owner, &[TransferOutput{recipient, amount}])` → `TransferReceipt {nullifiers[], newCommitments[], newRoot, bundleIndex, proof}` | Nothing hits the ledger. `recipient` is a registered owner id. ≤ 8 outputs. `senderAuth` (who may act as `owner`) is the API layer's job. |
| `POST /unshield` `{owner, amount, destination}` | `pool.unshield(owner, amount, destination)` → `UnshieldReceipt {…, payout:{amount, destination}}` | Then build + submit the pool→`destination` Payment for `payout.amount`. The proof's signatures are bound to `destination` and to the bundle's own value balance. |
| `POST /audit/reveal` `{auditorKey}` | `pool.reveal(viewing_key_hex)` → `RevealReport {balance, notes[]}` | `auditorKey` = the holder's full viewing key (`OwnerInfo.viewingKey`, 96 bytes hex). Reveals that holder's complete activity, nothing about others' transactions. |
| `GET /pool/state` | `pool.state()` → `PoolState {root, anchoredRoot, leafCount, nullifierCount, bundleCount}` | `root` is what the FE writes into the memo Payment each batch. |
| (anchoring hook) | `pool.set_anchored_root(root_hex)` | Call after the memo Payment is confirmed; must be a root the pool has had. |
| (publish / re-verify) | `pool.records()` → `&[BundleRecord]`; `record::bundle_from_record` + `pool.verify(...)` | The public log: no plaintext, no keys. A third party replays it into a fresh `Pool` and checks the final root against the anchored one (see `core/examples/demo.rs`). |
| (persistence) | `pool.snapshot()` / `Pool::restore(&snapshot)` | **Contains spending keys.** Write to a gitignored file (`*.pool-state.json`). Restore replays the log through the same verify tail. |

Errors are `confidant_core::Error` (`thiserror`); map `UnknownOwner` / `OwnerExists` /
`ZeroAmount` / `AmountOverflow` / `InsufficientFunds` / `TooManyOutputs` /
`VouchedAmountNotApplicable` / `InvalidEncoding` to 400, `DoubleSpend` / `UnknownAnchor` /
`ValueBalance*` / `InvalidProof` / `InvalidSpendAuth` / `InvalidBindingSignature` to 409/422, and
`Build` / `Tree` / `NoWitness` / `Snapshot` / `Internal` to 500.

Tests: `cargo test --workspace` (behavioural + negative suite in `core/tests/pool.rs`, coverage
corners in `core/tests/coverage.rs`); `cargo llvm-cov --workspace --all-targets` reports 100% line
coverage of the core, which CI enforces.

Timings on a laptop (release): keys 2.5 s once; shield/transfer/unshield ≈ 0.9 s each (2-action
bundle, +0.45 s per extra action); verify ≈ 10 ms; reveal is linear in the number of actions.

Run `cargo run --release --example demo` for an offline walk-through of the demo script.
