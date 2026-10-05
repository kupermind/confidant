# End-to-end testing

Two levels. Both play the SPEC demo script in stage order: pre-populated pool → Alice shields →
root anchored → Alice pays Bob inside the pool → (backend restart) → Bob unshields to a fresh
address → the auditor reveals exactly Bob → a third party re-verifies the public log against the
anchored roots.

## 1. Core level — `core/tests/e2e_demo.rs` (runs in CI)

Against `confidant-core` alone, no network. Adds what only the core can check: the public record
of the hop carries no sender/recipient/amount and has the same shape as every other hop; snapshot →
restore is state-identical; a bystander's key sees only the bystander; tampered / replayed /
re-pointed log entries are refused by the verifier.

```bash
cargo test --test e2e_demo -- --nocapture      # prints a timing table
```

## 2. XRPL testnet level — `e2e/testnet/run.mjs` (manual, or CI `workflow_dispatch`)

Real Payments on XRPL **testnet**, `xrpl.js` on the XRPL side (where SPEC puts it), the core driven
through `core/examples/cli.rs` — a test tool standing in for the backend (one `Pool`, snapshot on
disk, the five operations as subcommands, JSON out).

What happens on ledger and what is asserted:

| Step | On XRPL testnet | Asserted |
|---|---|---|
| accounts | faucet funds the pool account, Alice and an anchor sink; Bob's exit address is a fresh, unfunded wallet | — |
| shield | Alice → pool Payment (25 XRP) | the harness **validates the deposit on-ledger** (validated, Payment, destination = pool, `delivered_amount`) before calling `cli shield` — the backend's custodial backing step |
| anchor | pool → sink Payment, 1 drop, memo `confidant/root` = tree root | `cli anchored <root>` records it; later the memo is read back from the ledger and compared |
| private transfer | nothing | the pool account's `Sequence` is unchanged across the hop |
| anchor | memo Payment with the new root | as above |
| unshield | pool → Bob's fresh address Payment for `payout.amount` | the fresh account exists with exactly that balance |
| anchor | memo Payment with the final root | as above |
| reveal | — | Bob's key: one incoming note, spent, balance 0; Alice's key: outgoing payment to Bob's address |
| third party | reads the three anchor memos back from the ledger | `cli export-log` + `cli verify-log` (fresh pool, no keys) lands on the last anchored root; every memo root matches |

Each run writes `data/e2e-<timestamp>.{pool-state,log,report}.json` (gitignored; the state file
holds spending keys) and prints explorer links for every transaction.

```bash
cargo build --release --example cli
cd e2e/testnet && npm install            # xrpl 5.x + a local Node 22 (needs require(esm))
CLI=../../target/release/examples/cli npm run e2e
```

Env: `XRPL_WS` (default `wss://s.altnet.rippletest.net:51233`), `DEPOSIT_XRP` (default 25), `CLI`.
Needs network access to the testnet and its faucet; ~90 s per run.

Reference run (2026-10-05): deposit `CE49EB29…`, anchors `21B3DD66…`, `8C0A4C9B…`, `6CCB0E9A…`,
payout `5B5FF7B8…` on testnet; 3 bundles, 6 leaves, 3 anchors, replayed root = last memo root.

## 3. What the API-level e2e adds once `backend/` exists (API/XRPL lead)

Same steps through the SPEC endpoints instead of the CLI:

- `POST /shield` accepted only after the deposit is found on-ledger (as the harness does); a bogus
  `depositTxHash` or amount is refused.
- `POST /transfer` with a wrong `senderAuth` is refused.
- `GET /pool/state.root` equals the memo root after each anchor; `anchoredRoot` follows.
- `POST /unshield` → pool → destination Payment for exactly `payout.amount`.
- `POST /audit/reveal` with Bob's key returns the report above.
- Restarting the backend between steps changes nothing (snapshot on disk).

The harness is written so swapping `core(...)` (CLI calls) for HTTP calls is the only change.
