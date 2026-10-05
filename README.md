# Confidant

**A compliant shielded pool for the XRP Ledger.** Hide the transaction *graph* that
XLS-96 leaves public — sender, receiver, and linkage of internal transfers — while
keeping privacy auditable on demand through a viewing/auditor key.

Built for the AI & Fintech XRP Ledger Hackathon (NYC, Oct 24–25, 2026). Testnet-first.

---

## ⚠️ Branding note — do not lose this

- **Formal name: `Confidant`.** Use this in the pitch, slides, and any
  institution-facing material.
- **Stylized wordmark: `C0nf!d@nt`.** Logo / README banner / demo / social flair **only**.
- **Why the split:** the leet styling skews "hacker-cool," which is great for hackathon
  memorability but slightly off-key against the *institution-grade, compliant* half of the
  pitch. Keep the clean name where it needs to be taken seriously.
- **Identifiers are ASCII:** repo / crate / npm = `confidant` (no `@`/`!` — those are
  display-only and are not valid in package names).

---

## What it does

A Zcash/Railgun-grade shielded pool, brought to XRPL, with selective disclosure:

1. **Shield** — move a token from a public XRPL balance into the pool (an ordinary Payment).
2. **Private transfer** — pay inside the pool; the ledger shows no sender, receiver, or
   amount — only an anchored commitment root.
3. **Unshield** — exit to a fresh XRPL address, unlinkable to the deposit among the pool's set.
4. **Selective disclosure** — an auditor/viewing key reveals exactly one holder's balance,
   and nothing else. Private *and* auditable.

## Stack

- **Core:** Rust crate `confidant-core` — the Zcash Orchard Action circuit reused unchanged
  (`orchard` 0.15.5, `FixedPostNu6_2`), commitment tree + nullifier set, prover/verifier, auditor
  reveal. **Backend + API:** Rust (axum) over the core.
- **Frontend:** React/TypeScript + `xrpl.js` — owns all XRPL (shield/unshield Payments,
  custody signing, root-memo anchoring).
- No Solidity, no WASM, no `xrpl-rust`.

## Privacy — what we claim, what we don't

- **Claim:** parity with Tornado/Railgun (hide the internal graph), on XRPL, plus selective
  disclosure. Not a novel privacy guarantee.
- **Inherent & public (any pool):** shield/unshield amounts + external addresses. Real-world
  privacy = anonymity-set size + fixed denominations + delay.
- **Custodial today:** the operator holds every holder's spending key — it sees internal plaintext
  **and can spend any holder's funds** — and vouches that each shield is backed by a real XRPL
  deposit. Removed in production by distributing the operator and/or a TEE. **Do not** claim
  "untraceable / trustless today."
- **Endpoint correlation:** the pool hides the internal graph, not the fact that someone shielded X
  and later someone unshielded X; with a small anonymity set those endpoints correlate. An empty pool
  gives the first operations no privacy — pre-populate it.
- **Auditor key = the holder's full viewing key:** it reveals that holder's complete activity
  (receipts, change, spends, and the recipients/amounts of their outgoing transfers) and balance,
  nothing about transactions they were not a party to.

## Status

What is done and what is left, per role: [`ROADMAP.md`](./ROADMAP.md).

## Full technical task

See [`SPEC.md`](./SPEC.md) (подробная русская версия: [`SPEC.ru.md`](./SPEC.ru.md)) — objective, architecture, API contract, data model, milestones,
definition of done, demo script, and the native-amendment sketch.
