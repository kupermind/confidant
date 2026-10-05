# CLAUDE.md — Confidant

Guidance for any Claude session working in this repo.

## What this is

**Confidant** — a compliant shielded pool for the XRP Ledger: hide the transaction *graph*
XLS-96 leaves public (sender/receiver/linkage of internal transfers) with selective disclosure via a
holder's full viewing key. Built for the AI & Fintech XRPL Hackathon (NYC, Oct 24–25, 2026),
**testnet-first**. The design of record is [`SPEC.md`](./SPEC.md) (Russian: `SPEC.ru.md`); work
status is [`ROADMAP.md`](./ROADMAP.md).

Architecture (v2, implemented 2026-10-05): a **`Wallet`** on the holder's machine builds, proves and
signs every bundle (keys and plaintext never leave it); **`Verifier`s** run by a k-of-N quorum of
independent signers (XRPL SignerList on the pool account, master key disabled) re-verify every proof
and every deposit on their own state and sign only roots they computed themselves; every accepted
operation is anchored in its own memo so anyone can rebuild the state from the ledger.

Team: two builders — a **crypto/ZK lead** (Rust core, CLI test tool, e2e harness, audit cycle) and an
**API/XRPL lead** (HTTP layer over `Verifier`, signer services, scanner, `xrpl.js`, UI, pitch). Refer to
people by role, never by name, in anything that reaches git (see hard rules).

## ⚠️ Branding — do not lose this remark

- **Formal name `Confidant`** in the pitch, slides, and any institution-facing material.
- **Stylized wordmark `C0nf!d@nt`** for the logo / README banner / demo / social flair **only**.
- Identifiers (repo/crate/npm) are ASCII `confidant`.

## Hard constraints (carry into every change)

- **Testnet first.** Mainnet only if the testnet demo is solid.
- **Honest labels.** Say: notes are non-custodial; the pool account and proof verification are
  federated (k-of-N); state is reconstructible from the ledger; **collusion of k signers can drain the
  pool account but cannot spend anyone's notes**; quorum because today's XRPL has no on-ledger Halo 2
  verifier. **Never** claim "fully anonymous / untraceable / trustless today / private from the
  operator." Shield/unshield amounts + addresses are public (inherent); an empty pool has no privacy.
  CI checks the label markers in `SPEC.md`, `README.md`, `SPEC.ru.md`.
- **TEE is a production footnote only** — never in the demo mechanics, never the lead.
- **"Only what you build during the hackathon counts."** The core, the reused Orchard circuit and
  the harness are pre-existing; the judged work is the XRPL integration built on-site. Be honest.
- **Scope is fixed in SPEC.md.** Non-goals (multi-asset, AMM, ZKML, on-ledger verify, WASM) stay cut.

## Stack

- Rust `core/` (`confidant-core`): `Wallet`, `Verifier`, Orchard circuit reused unchanged, tree,
  nullifiers, deposit binding, anchor memos, `endorse`, `rebuild_checked`, reveal. XRPL-agnostic.
- Rust backend (axum) over `Verifier`; React/TS front-end + `xrpl.js` owns all XRPL. No Solidity, no
  WASM, no `xrpl-rust`.

## Working rules

- **Branch + PR for everything; `main` only via PR with green CI** (fmt, clippy `-D warnings`, tests
  in release, **100% line coverage gate**, hygiene).
- **Audit cycle:** every change to `core/` gets a slice ruling from the audit session before merge
  (`docs/audit/REQ-*` → `RESP-*`, committed verbatim, indexed in `docs/audit/INDEX.md`). Mutation-test
  every new guard (remove it → its test must go red) and report the table in the REQ.
- Keep `SPEC.md`, `SPEC.ru.md`, `ROADMAP.md` and `docs/` in sync with the code in the same PR.

## Hard rules for git (public repo hygiene)

- **Never put a real person's name** in committed files, commit messages, or PR text. Use roles.
- **No secrets** in the repo — keys, seeds, `.env`, `*.wallet.json`, `data/` stay gitignored.
- End commit messages with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`
- No "Generated with Claude Code" footers in PR descriptions.

## Commands

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release
cargo llvm-cov --workspace --all-targets --fail-under-lines 100
cargo build --release --example cli && (cd e2e/testnet && npm install && CLI=../../target/release/examples/cli npm run e2e)
```

## Where the strategy lives

Funding/BD framing, the hackathon review, and name deliberations live in a **private planning
doc**, not in this repo. This repo holds the technical task only.
