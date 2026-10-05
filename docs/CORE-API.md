# `confidant-core` — how the API layer uses it (v2)

> The reference implementation of the verifier side is `backend/node` (`confidant-node`), and of the
> quorum (signer sidecars + coordinator) `backend/quorum/` — see `backend/README.md`. A user-facing
> API can sit in front of the coordinator or call a node directly.

Two types, two sides of the trust boundary. Neither touches XRPL or HTTP.

| Type | Runs where | Holds | Does |
|---|---|---|---|
| `Wallet` | the holder's machine (a local process the UI talks to over localhost) | the spending key, the holder's notes, a replica of the public state | `sync` from the public log; `build_shield` / `build_transfer` / `build_unshield` → a `Prepared` bundle (proven and signed) |
| `Verifier` | every signer of the pool account's SignerList, and anyone else who wants one | public state only: tree, nullifiers, consumed deposits, bundle log | `submit_*` (verify-and-apply), `anchor_memo`, `check_memo`, `rebuild`, `reveal`, `snapshot` / `restore` |

```toml
# backend/Cargo.toml
confidant-core = { path = "../core" }
```

## Verifier side — the five v2 endpoints

| SPEC v2 endpoint | Core call | Notes |
| --- | --- | --- |
| `GET /log?from=N` | `verifier.records()[N..]`, `verifier.state()` | The public log is the mirror wallets sync from and new nodes rebuild from. Serve it whole; it contains no plaintext and no keys. |
| `POST /submit/shield` `{bundle, depositTxHash}` | 1. look the deposit up on XRPL (`tx`): validated, `TransactionType == Payment`, `Destination == pool`, read `meta.delivered_amount` and the `confidant/shield` memo; 2. `verifier.submit_shield(&bundle, &DepositRef { tx_hash, delivered, memo_hash })` → `Accepted` | The ledger facts are yours; the core enforces `delivered == −valueBalance`, memo hash == bundle hash, and one shield per deposit. Then write the anchor memo (below). |
| `POST /submit/transfer` `{bundle}` | `verifier.submit_transfer(&bundle)` → `Accepted` | Nothing hits the ledger except the anchor memo. |
| `POST /submit/unshield` `{bundle, destination}` | `verifier.submit_unshield(&bundle, destination)` → `Accepted { payout: Some(..) }` | The signatures inside the bundle bind `destination` and the amount. The quorum then pays `payout.amount` to `payout.destination` from the pool account. |
| `POST /audit/reveal` `{viewingKey}` | `verifier.reveal(viewing_key_hex)` → `RevealReport` | Convenience over the public log; the holder's auditor can run the same on any mirror. |

After every accepted bundle: `verifier.anchor_memo(acc.bundle_index)?.encode_hex()` is the
`MemoData` (type `confidant/anchor`) of the Payment the quorum multisigns from the pool account;
once validated, `verifier.set_anchored_root(&acc.new_root)`. For an unshield the same batch (or the
next transaction) carries the payout.

Each signer runs its own `Verifier`. A signer service receives `{ submission: Submission,
proposedMemo: hex }` from the coordinator, repeats the deposit lookup itself for shields, and calls
`verifier.endorse(&submission, &memo)` — it checks the memo against the submission and its own state,
predicts the root on its own tree and applies only on agreement. Only then does it sign the anchor
(and the payout). A coordinator just collects ≥ k signatures. A signer that refuses produces no
signature and its state is unchanged.

Joining as a new node (or a signer starting up): read the pool account's transactions
(`account_tx`), decode each `confidant/anchor` memo (`AnchorMemo::decode_hex`), fetch bundles by hash
from any `GET /log`, look up every shield memo's deposit on-ledger, and
`Verifier::rebuild_checked(&memos, &mut mirror, &mut deposits)`. The root must land on the last
memo's root; a shield whose deposit is missing, delivered a different amount or commits to another
bundle refuses the join. `Verifier::rebuild` (no deposit facts) re-verifies the cryptography only
and trusts the quorum on backing — fine for a read-only mirror, not for a signer.

## Wallet side — what the local wallet process exposes to the UI

| UI action | Core call | Notes |
| --- | --- | --- |
| show address / auditor key | `wallet.info()` → `{ address, viewingKey }` | the holder hands `viewingKey` to their auditor; nobody else needs it |
| refresh | `wallet.sync(&log)` after `GET /log` | re-verifies new records into the replica; finds the wallet's notes; marks spent ones |
| balance | `wallet.balance()` | |
| shield | `let p = wallet.build_shield(amount)?;` then the UI sends the deposit Payment with memo `confidant/shield` = `p.hash_hex()`, then `POST /submit/shield { bundle: p.record, depositTxHash }` | build first, deposit second: the memo must commit to the bundle |
| transfer | `wallet.build_transfer(&[TransferOutput { recipient: <address hex>, amount }])?` → `POST /submit/transfer` | recipients are raw Orchard addresses, not ids |
| unshield | `wallet.build_unshield(amount, destination)?` → `POST /submit/unshield { bundle, destination }` | |
| persist | `wallet.secret()` → `WalletSecret` (JSON, **holds the spending key**, write 0600, never commit) / `Wallet::from_secret` | a wallet restores by re-syncing the log |

Errors are `confidant_core::Error` (`thiserror`). Map `ZeroAmount` / `AmountOverflow` /
`InsufficientFunds` / `TooManyOutputs` / `VouchedAmountNotApplicable` / `InvalidEncoding` /
`DepositRequired` / `MemoMalformed` to 400; `DoubleSpend` / `DepositConsumed` /
`DepositBindingMismatch` / `UnknownAnchor` / `ValueBalance*` / `InvalidProof` / `InvalidSpendAuth`
/ `InvalidBindingSignature` / `BundleHashMismatch` / `MemoMismatch` / `RecordOutOfOrder` to
409/422; `Build` / `Tree` / `NoWitness` / `Snapshot` / `MirrorMissing` / `LogRewound` /
`Internal` to 500.

Timings (laptop, release): circuit params 2.5 s once per process; a 2-action bundle proves in
≈ 0.9 s on the wallet side (+0.45 s per extra action); the verifier accepts in ≈ 10 ms; `sync` and
`rebuild` cost ≈ 10 ms per record; anchor memos are ≈ 230 B for 2 actions, ≤ 1 KB up to 8.

Tests: `cargo test --workspace`; `cargo llvm-cov --workspace --all-targets` reports 100% line
coverage of the core, which CI enforces. `cargo run --release --example demo` walks the v2 flow
offline; `core/examples/cli.rs` is the test tool the testnet harness drives.
