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

## 2. XRPL testnet level — `e2e/testnet/run.mjs` (manual, or CI `workflow_dispatch`) — v2

Real Payments on XRPL **testnet** with the v2 trust split: wallets build and prove locally
(`cli wallet`), a coordinator and three signers each run their own `cli verifier` state, the pool
account is a **2-of-3 SignerList with the master key disabled**, and every accepted operation is
anchored in its own memo. Everything XRPL happens in `xrpl.js`, where SPEC puts it.

| Step | On XRPL testnet | Asserted |
|---|---|---|
| accounts | faucet funds the pool, Alice and an anchor sink; three signer keys and Bob's exit address are fresh | — |
| quorum | `SignerListSet` (quorum 2 of 3) then `AccountSet asfDisableMaster` on the pool | `account_info`: master disabled, signer list present |
| shield | Alice's wallet **builds first**; her deposit Payment carries memo `confidant/shield` = bundle hash | the coordinator **and every signer** validate the deposit on-ledger (validated Payment, Destination = pool, `delivered_amount`, memo = hash); the coordinator accepts; each signer `endorse`s on its own state and computes the same root; the same deposit against a fresh bundle is refused |
| anchor | pool → sink Payment, 1 drop, memo `confidant/anchor` = the per-op memo, **multisigned by 2 of the 3 signers that endorsed** | memo bytes on ledger equal what the quorum signed |
| private transfer | nothing | the pool account's `Sequence` is unchanged across the hop; 3/3 signers endorse |
| lying coordinator | — | a proposed memo with a flipped root bit is refused by **3/3 signers** (`endorse` exits non-zero, no signature) |
| unshield | anchor memo + payout Payment pool → Bob's fresh address, both 2-of-3 multisigned | signers endorse the honest memo; the fresh account holds exactly the payout |
| reveal | — | Bob's key on a signer's state: one incoming note, spent, balance 0 |
| new node | reads the pool account's `account_tx`, decodes the three `confidant/anchor` memos in ledger order | `cli verifier rebuild memos.json log.json` lands on the last anchored root with the same bundle and consumed-deposit counts; it also re-validates the deposit from the ledger |
| lagging signer | — | a signer with no state cannot endorse the unshield; after `rebuild` from the ledger it agrees |

Each run writes `data/e2e-<timestamp>/` (gitignored): `*.wallet.json` (spending keys, mode 0600,
loud warning), `*.verifier-state.json` (public), bundles, memos, `log.json`, `report.json` with
explorer links. It is a test tool; the API/XRPL lead lifts the XRPL parts (deposit validation,
memo anchoring, multisign, payout, scanner) into the backend.

```bash
cargo build --release --example cli
cd e2e/testnet && npm install            # xrpl 5.x + a local Node 22 (needs require(esm))
CLI=../../target/release/examples/cli npm run e2e
```

Env: `XRPL_WS` (default `wss://s.altnet.rippletest.net:51233`), `DEPOSIT_XRP` (default 25), `CLI`.
Needs network access to the testnet and its faucet; a few minutes per run (three faucet calls,
two single-signed setup transactions, one deposit, three multisigned anchors, one multisigned
payout).

## 3. What the API-level e2e adds once `backend/` exists (API/XRPL lead)

Same steps through the SPEC v2 endpoints instead of the CLI: `POST /submit/shield` with
`{bundle, depositTxHash}` accepted only after the service validates the deposit (as the harness
does) and refuses a bogus hash, amount or memo; `POST /submit/transfer`; `POST /submit/unshield`
followed by the quorum's payout; `GET /log` serving the mirror a wallet syncs from and a new node
rebuilds from; `POST /audit/reveal`. The signer side is `Verifier::endorse` behind a small
"sign this if it is what you computed" endpoint per signer; the coordinator collects ≥ k
signatures. The harness is written so swapping the `cli(...)` calls for HTTP calls is the only
change.
