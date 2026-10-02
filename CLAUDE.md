# CLAUDE.md — Confidant

Guidance for any Claude session working in this repo.

## What this is

**Confidant** — a compliant shielded pool for the XRP Ledger: hide the transaction *graph*
XLS-96 leaves public (sender/receiver/linkage of internal transfers) with selective disclosure
via an auditor/viewing key. Built for the AI & Fintech XRPL Hackathon (NYC, Oct 24–25, 2026),
**testnet-first**. The full buildable task is in [`SPEC.md`](./SPEC.md).

Team: two builders — a **crypto/ZK lead** (Rust core, circuits, API, auditor decrypt) and an
**XRPL/front-end lead** (`xrpl.js`, custody, root anchoring, UI, pitch). Refer to people by role,
never by name, in anything that reaches git (see hard rules).

## ⚠️ Branding — do not lose this remark

- **Formal name `Confidant`** in the pitch, slides, and any institution-facing material.
- **Stylized wordmark `C0nf!d@nt`** for the logo / README banner / demo / social flair **only**.
- The leet styling skews "hacker-cool" — great for hackathon memorability, slightly off-key
  against the *institution-grade, compliant* half of the pitch. Keep the clean name where it
  must be taken seriously. Identifiers (repo/crate/npm) are ASCII `confidant` — `@`/`!` are
  display-only and invalid in package names.

## Hard constraints (carry into every change)

- **Testnet first.** Mainnet only if the testnet demo is solid.
- **Privacy claims:** parity with Tornado/Railgun + selective disclosure. **Never** claim
  "fully anonymous / untraceable / trustless today." Shield/unshield amounts + addresses are
  public (inherent); real privacy = anonymity set + fixed denominations + delay.
- **TEE is a production footnote only** — "distributable + TEE removes the operator from the
  trust base." Never put TEE in the core demo mechanics or lead with it. (Last hackathon, judges
  didn't believe the team understood TEE — lead with ZK, which judges already recognize.)
- **"Only what you build during the hackathon counts."** The reused Halo 2 circuits + any FROST
  are pre-existing libraries; the judged work is the XRPL integration built on-site. Be honest
  about the split.
- **Scope is fixed in SPEC.md.** Build one thing well: shield → private transfer → unshield →
  auditor reveal. Non-goals (multi-asset, AMM, ZKML, on-chain verify, WASM) stay cut.

## Stack

- Rust backend + API (crypto core, tree, nullifiers, prover, auditor decrypt) — XRPL-agnostic.
- React/TS front-end + `xrpl.js` — owns all XRPL. No Solidity, no WASM, no `xrpl-rust`.

## Hard rules for git (public repo hygiene)

- **Never put a real person's name** in committed files, commit messages, or PR text. Use roles
  ("the crypto lead", "the XRPL lead"). This repo may become public.
- **No secrets** in the repo — keys, seeds, `.env` stay gitignored.
- End commit messages with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`
- No "Generated with Claude Code" footers in PR descriptions.

## Where the strategy lives

Funding/BD framing, the hackathon review, and name deliberations live in a **private planning
doc**, not in this repo. This repo holds the technical task only.
