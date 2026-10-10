# End-to-end testing

Two levels. Both play the SPEC demo script in stage order: pre-populated pool → Alice shields →
root anchored → Alice pays Bob inside the pool → (backend restart) → Bob unshields to a fresh
address → the auditor reveals exactly Bob → a third party re-verifies the public log against the
anchored roots.

## 1. Core level — `core/tests/e2e_demo.rs` (runs in CI)

Against `confidant-core` alone, no network, with the v2 trust split in place: six wallets
pre-populate the pool; Alice's wallet builds a shield and the verifier accepts it against a
deposit reference (memo = bundle hash); a second signer replays the public log and agrees on every
anchor memo; Alice pays Bob (the public record of the hop carries no address or amount and has the
same shape as every other hop); Bob unshields and a signer asked to endorse a re-pointed payout
refuses; the auditor key reveals exactly Bob (a bystander's key only the bystander); a new node
rebuilds from the anchor memos plus a mirror and lands on the same root; tampered and replayed
entries are refused.

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
| new node | reads the pool account's `account_tx`, decodes the three `confidant/anchor` memos in ledger order, and **looks up every shield memo's deposit itself** | `cli verifier rebuild memos.json log.json deposits.json` (`rebuild_checked`) lands on the last anchored root with the same bundle and consumed-deposit counts; a wrong deposit amount or missing deposit facts refuse the join |
| lagging signer | — | a signer with no state cannot endorse the unshield; after `rebuild` from the ledger it agrees |

Each run writes `data/e2e-<timestamp>/` (gitignored): `*.wallet.json` (spending keys, mode 0600,
loud warning), `*.verifier-state.json` (public), bundles, memos, `log.json`, `report.json` with
explorer links. It is a test tool; the XRPL parts it drives by hand (deposit validation, memo
anchoring, multisign, payout, scanner) live in `backend/` as services — §2b runs the same demo
through them.

```bash
cargo build --release --example cli
cd e2e/testnet && npm install            # xrpl 5.x + a local Node 22 (needs require(esm))
CLI=../../target/release/examples/cli npm run e2e
```

Env: `XRPL_WS` (default `wss://s.altnet.rippletest.net:51233`), `DEPOSIT_XRP` (default 25), `CLI`.
Needs network access to the testnet and its faucet; about 3½ minutes per run (three faucet calls,
two single-signed setup transactions, one deposit, three multisigned anchors, one multisigned
payout).

Reference run (2026-10-05, v2): pool `rQHXk7c3XzGycM2qx651xGnbUnfocoUMxB`; `SignerListSet`
`058FF483…`, `asfDisableMaster` `23FBA67C…`, deposit `69B70691…`, anchors `EB2593F4…` (260 B),
`D3D99CD2…` (196 B), `645CE5B7…` (230 B), payout `FA8BE0AC…` — all on testnet.xrpl.org; 3/3
signers endorsed every honest memo, 3/3 refused the lying one, the new node's root equals the last
anchor.

## 2b. Through the quorum services — `e2e/testnet/run-services.mjs` (manual)

The same demo as a client of the **coordinator**, with real services: 4 Rust verifier nodes (the
coordinator's and three signers'), 3 signer sidecars, 1 coordinator, launched by the harness from
`backend/quorum/lib.mjs`. Asserted on top of §2: a bogus deposit hash is refused by the coordinator's
node (ledger lookup); the same deposit again (identical bundle, fresh bundle) is refused; exactly one
pool transaction (the anchor) during the private hop; an unshield through the coordinator is anchored
then paid 2-of-3 and the answer carries the multisigned payout blob (decodes to the payout, is
resubmittable); a **signer sidecar refuses** a tampered memo, a re-pointed payout, an inflated payout
amount, a transaction from another account, an inflated fee, an anchor to another destination, a
future Sequence, a missing or too-distant LastLedgerSequence and a payout placed before its anchor,
and signs the honest set; a replay is refused by the coordinator; a node refuses `/anchored` for a
non-anchor transaction or a root the anchor does not carry; **nodes configured for another quorum or
signer set refuse to start** (custody gate); a **new Rust node started with `--rebuild --mirror`**
lands on the quorum's root and anchored root with every deposit re-validated. **Boundary screening**
(every node launched with a demo list holding one funded account and a 2-ledger standby): the first
shield is refused with 403 "standby" right after its deposit validates and accepted once the ledger
caught up; a deposit from the listed account is refused with 403 at the coordinator's node; an
unshield to the listed address is refused at the coordinator and by a signer sidecar's own node;
the replayable screening audit (`backend/quorum/screening/audit.mjs`) over the public log reports
clean.

```bash
cargo build --release --example cli --bin confidant-node
cd e2e/testnet && CLI=../../target/release/examples/cli NODE_BIN=../../target/release/confidant-node npm run e2e:services
```

## 3. What the user-facing layer adds on top (API/XRPL lead) — see `docs/API-BOUNDARY.md`

The coordinator already exposes `POST /submit/*`, `GET /log`, `GET /state` and `POST /audit/reveal`.
What remains for the user-facing layer is the wallet side: a local wallet process (or the CLI) that
builds bundles, the deposit Payment with memo = bundle hash sent from the user's XRPL account, and a
UI that drives shield → transfer → unshield → reveal against the coordinator and shows the explorer.
