# Confidant

**A compliant shielded pool for the XRP Ledger.** Hide the transaction *graph* that XLS-96 leaves
public — sender, receiver, and linkage of internal transfers — while keeping privacy auditable on
demand through a holder's viewing key.

Built for the AI & Fintech XRP Ledger Hackathon (NYC, Oct 24–25, 2026). Testnet-first.

---

## ⚠️ Branding note — do not lose this

- **Formal name: `Confidant`.** Use this in the pitch, slides, and any institution-facing material.
- **Stylized wordmark: `C0nf!d@nt`.** Logo / README banner / demo / social flair **only**.
- **Why the split:** the leet styling skews "hacker-cool," which is great for hackathon
  memorability but slightly off-key against the *institution-grade, compliant* half of the pitch.
  Keep the clean name where it needs to be taken seriously.
- **Identifiers are ASCII:** repo / crate / npm = `confidant` (no `@`/`!` — those are display-only
  and are not valid in package names).

---

## What it does

A Zcash/Railgun-grade shielded pool, brought to XRPL, with selective disclosure and no single
trusted operator:

1. **Shield** — the holder's wallet builds and proves a note on their own machine; the deposit
   Payment into the pool carries the bundle's hash in a memo, so the deposit backs exactly that note.
2. **Private transfer** — pay inside the pool; the ledger shows no sender, receiver, or amount —
   only an anchor memo with the new root, nullifiers and commitments.
3. **Unshield** — exit to a fresh XRPL address; the payout is multisigned by a k-of-N quorum of
   independent verifiers, each of which re-verified the proof on its own state.
4. **Selective disclosure** — the holder's full viewing key reveals exactly that holder's activity
   and balance, and nothing about anyone else's transactions.

Anyone can rebuild the pool's state from the pool account's transactions plus a mirror of the
bundle bytes, re-verifying every proof, signature and deposit (`docs/NODE-OPERATOR.md`).

## Stack

- **Core:** Rust crate `confidant-core` — the Zcash Orchard Action circuit reused unchanged
  (`orchard` 0.15.5, `FixedPostNu6_2`); `Wallet` (keys, notes, build/prove/sign, runs on the
  holder's machine) and `Verifier` (public state, verify-and-apply, anchor memos, rebuild, reveal;
  one per signer and for anyone else).
- **Quorum services (`backend/`):** a Rust verifier node (`confidant-node`, axum over `Verifier`;
  reads the ledger, holds no keys), Node signer sidecars (the only key holders; sign only what their
  own node endorsed) and a coordinator (k-of-N multisig, anchor then payout) — `backend/README.md`.
- **Front-end (prototype, `frontend/`):** one static page, no build step. The wallet is the core
  itself compiled to **WebAssembly** (`frontend/wasm`, wasm-bindgen) running in a Web Worker: the
  spending key, the notes, proving and signing never leave the browser (≈ 25 s to build the proving
  keys once per session, ≈ 20 s per proof, single-threaded). `xrpl.js` in the browser does the
  faucet and the deposit Payment with the bundle-hash memo; the coordinator is called through
  `/api/`. Only if WebAssembly is unavailable does the page fall back to the **demo wallet service**
  (`backend/wallet`, holds demo keys on the host). The contract: `docs/API-BOUNDARY.md`.
  A one-page overview for visitors, judges and lenders — `frontend/how-it-works.html`, served at
  `/how-it-works.html` and linked from the app's header — explains the pool, the quorum, the hackathon
  lending features and the honest limits. `frontend/balances.html` (`/balances.html`) reconciles the
  shielded total with the pool account's balance live from the ledger and answers the usual questions.
- No Solidity, no WASM, no `xrpl-rust`.

## Privacy and trust — what we claim, what we don't

- **Claim:** parity with Tornado/Railgun on hiding the internal graph, on XRPL, plus selective
  disclosure. Not a novel privacy guarantee.
- **Notes are non-custodial:** keys and plaintext never leave the holder's wallet; nobody else can
  spend or read a holder's notes.
- **The pool account is federated, not consensus-verified:** a k-of-N SignerList of independent verifiers
  holds it and verifies every proof and every deposit before signing. **Collusion of k signers can
  drain the pool account; it cannot spend anyone's notes.** We verify by quorum because today's
  XRPL has no on-ledger Halo 2 verifier; an amendment adding one would make verification native.
  "Custodial" describes only this federated custody of the pool account.
- **Endpoint correlation:** the pool hides the internal graph, not the fact that someone shielded X
  and later someone unshielded X; with a small anonymity set those endpoints correlate. An empty pool
  gives the first operations no privacy — pre-populate it.
- **Auditor key = the holder's full viewing key:** it reveals that holder's complete activity
  (receipts, change, spends, and the recipients/amounts of their outgoing transfers) and balance,
  nothing about transactions they were not a party to.
- **Demo wallets are hosted:** the prototype's wallet service keeps the demo spending keys on the
  demo host — say so; the non-custodial claim holds for a wallet running on the holder's machine.
  **Testnet demo only, and it must not carry over to real value:** a custodial wallet service is never the path for a mainnet or real-XRP pool; the browser (WASM) wallet is the production path and the service does not graduate with it.
- **Protocol fee** (server-side policy, Railgun's model): a configurable share of every shield and
  unshield (transfers free) is credited to a treasury account under the same k-of-N; the rates are
  public (`GET /config`), the note is credited net, the payout is net, and every accrual and sweep is
  verifiable by replaying the ledger. It is an operating fee, not a privacy feature.
- **Boundary screening** (optional, per node): shields from listed depositors or too-fresh deposits
  and unshields to listed addresses are refused on public data (e.g. OFAC's XRP entries). It is
  gatekeeping at the pool's public boundary, not a per-note proof of innocence; lists can be wrong
  or late; anyone can re-audit a pool from the public log.
- **Do not** claim "untraceable / trustless today."

## Status

What is done and what is left, per role: [`ROADMAP.md`](./ROADMAP.md). The v2 core and the quorum
services (verifier node, signer sidecars, coordinator) are merged, audited and exercised end to end on
XRPL testnet with a 2-of-3 quorum; the user-facing layer (wallet UX, deposit from the user's account,
UI over the coordinator, demo) is the API/XRPL lead's work for the hackathon —
[`docs/API-BOUNDARY.md`](./docs/API-BOUNDARY.md) is the hand-off. How this design compares with the
closest production system, Starknet's STRK20 (same note-and-nullifier family; on-chain STARK
verification there, a k-of-N quorum here; escrowed vs holder-initiated disclosure):
[`docs/COMPARISON-STRK20.md`](./docs/COMPARISON-STRK20.md). Why the proof system is Halo 2 with the Zcash
Orchard circuit reused unchanged — requirements, alternatives, costs accepted:
[`docs/WHY-HALO2.md`](./docs/WHY-HALO2.md).

## Full technical task

See [`SPEC.md`](./SPEC.md) (подробная русская версия: [`SPEC.ru.md`](./SPEC.ru.md)) — objective,
architecture, trust model, the core, API contract, XRPL side, milestones, definition of done, demo
script, testing, audit process, and the native-amendment sketch.

## Run

```bash
cargo test --workspace                       # 29 tests; cargo llvm-cov reports 100% line coverage
cargo run --release --example demo           # offline walk-through of the v2 flow
cargo build --release --example cli && cd e2e/testnet && npm install && \
  CLI=../../target/release/examples/cli npm run e2e    # the demo on XRPL testnet, 2-of-3 quorum
```
