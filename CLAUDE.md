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

Team: two builders — a **crypto/ZK lead** (Rust core, the quorum services in `backend/`: verifier
node, signer sidecars, coordinator, setup/launch scripts; CLI test tool, e2e harnesses, audit cycle)
and an **API/XRPL lead** (user-facing layer: local wallet UX, deposit Payment from the user's account,
UI over the coordinator, pre-population, pitch). The seam is documented in `docs/API-BOUNDARY.md`
(RU mirror `docs/API-BOUNDARY.ru.md`) — keep it in sync when an endpoint or response shape changes.
Refer to people by role, never by name, in anything that reaches git (see hard rules).

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
  Boundary screening (optional, per node) is gatekeeping on public lists at the public boundary, not a
  per-note proof of innocence — say so.
  CI checks the label markers in `SPEC.md`, `README.md`, `SPEC.ru.md`.
- **TEE is a production footnote only** — never in the demo mechanics, never the lead.
- **"Only what you build during the hackathon counts."** The core, the reused Orchard circuit, the
  harness and the XRPL integration already deployed on testnet are pre-existing (baseline tagged);
  the judged work is the new features built on-site (ROADMAP §0). Any pre-event prototype of them
  is named as pre-event. Be honest.
- **Scope is fixed in SPEC.md.** Non-goals (multi-asset, AMM, ZKML, on-ledger verify, WASM) stay cut.
- **UI rule (project lead, 2026-10-25) — Occam's razor on every element.** Every link, button, term,
  field and input requirement must survive one question: can we say in one sentence why the user has to
  do this here? If not, prefill it, derive it, move it to the other party, or drop it. The pages are for
  a non-expert; the ideal is one button; a field nobody knows how to fill is not a field; one action per
  state; the explanation before the input, not in the error after it; walk every page as a first-time
  user before opening the PR and say so in it. Full text and the worked example: `docs/UI-RULE.md`.

## Stack

- Rust `core/` (`confidant-core`): `Wallet`, `Verifier`, Orchard circuit reused unchanged, tree,
  nullifiers, deposit binding, anchor memos, `endorse`, `rebuild_checked`, reveal. XRPL-agnostic.
- Rust `backend/node/` (`confidant-node`, axum) over `Verifier`: reads the ledger by JSON-RPC only,
  holds no keys. Node `backend/quorum/` (`xrpl.js`): signer sidecars (the only key holders) and the
  coordinator. React/TS front-end + `xrpl.js` for the user side. No Solidity, no WASM, no `xrpl-rust`.

## Working rules

- **Branch + PR for everything; `main` only via PR with green CI** (fmt, clippy `-D warnings`, tests
  in release, **100% line coverage gate on `core/`, 95% on `backend/node`**, hygiene).
- **Audit cycle:** every change to `core/` or `backend/` gets a slice ruling from the audit session before merge
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
cargo llvm-cov -p confidant-core --all-targets --fail-under-lines 100
cargo llvm-cov -p confidant-node --all-targets --ignore-filename-regex '(main|rpc)\.rs' --fail-under-lines 95
cargo build --release --example cli --bin confidant-node
(cd e2e/testnet && npm install && CLI=../../target/release/examples/cli npm run e2e)             # CLI-driven
(cd backend/quorum && npm install) && (cd e2e/testnet && CLI=../../target/release/examples/cli NODE_BIN=../../target/release/confidant-node npm run e2e:services)  # through backend/
```

## Where the strategy lives

Funding/BD framing, the hackathon review, and name deliberations live in a **private planning
doc**, not in this repo. This repo holds the technical task only.
