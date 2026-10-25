# Submission text — AI & Fintech XRPL Hackathon, NYC, Oct 24–25 2026

The three answers the submission form asks for, kept here as the single source so the form, the slides and the
README say the same thing. **Living document:** update the "Status" line and the counts whenever something ships;
the honest-label rules of `SPEC.md` apply to every sentence (never "trustless", "untraceable", "fully anonymous",
"private from the operator"; the quorum is federated; shield/unshield amounts and addresses are public; screening
and the credential gate are checks at the public boundary, not proofs about any note).

Formal name everywhere in this text: **Confidant**. The stylized wordmark `C0nf!d@nt` is for the logo only.

Limits (from the form): short description ≤ 200 characters · description ≤ 5000 · technical description ≤ 1000.
Markdown is accepted in all three. Counts are of the text between the fences (`wc -m` on each block): 177 / 4787 / 1058 on 2026-10-25.

**Status (2026-10-25, day 2):** everything below is **live on the XRPL testnet** at `https://confidant.im` — the
pre-existing pool (baseline tag `pre-hackathon`), plus, built on-site and audited: the credential gate (A), proof of
funds with per-node verdicts and the quorum-signed on-ledger Credential (C), the lender page, and the Mastercard-track
relay (run end to end on 2026-10-25, both receipts). **Ruling of 2026-10-25 (lead, audit concurring):** the proof of
funds as built reveals the proven notes' nullifiers to whoever holds the proof, so it is a **demo of the flow,
labelled as degrading the holder's privacy — not the product feature**; the accepted design is a circuit that proves
control of unspent notes ≥ X without revealing nullifiers (the committed next step). The texts below say so.

---

## Short description (≤ 200 characters)

```text
Confidant — a compliant shielded pool on the XRP Ledger: private transfers inside, a credential-gated entrance, and proof of funds a lender can verify without seeing your notes.
```

## Description (≤ 5000 characters)

```markdown
**The problem.** XRPL's confidential-transfer work (XLS-96) hides *amounts* but leaves the *graph* public: who paid whom, and when, stays on the ledger forever. For a business, a fund or a lender that is the whole story — counterparties, payroll, treasury moves. The usual answer, a mixer, throws compliance away with the graph. Confidant keeps both.

**What Confidant is.** A shielded pool on the XRP Ledger testnet. You *shield* XRP into the pool (a public Payment to the pool account), *transfer* privately inside it — sender, receiver and amount never reach the ledger, only a 32-byte anchor memo does — and *unshield* back to any XRPL address. Every private transfer is a Halo 2 zero-knowledge proof (the Orchard circuit, reused unchanged) built **in your browser**; keys and notes never leave it. A k-of-N quorum of independent verifier nodes (an XRPL SignerList on the pool account, master key disabled) re-verifies every proof on its own state and signs only roots it computed itself. Every accepted operation is anchored in a memo, so anyone can rebuild the pool's state from the ledger alone — we rehearsed that from a public IPFS snapshot. Selective disclosure is built in: a holder hands an auditor one viewing key and that auditor sees exactly that holder's history, nothing else.

**Built at the hackathon (track: Lending & Borrowing).** Two features, designed before the event and *built, audited and deployed on-site*:

1. **Credential-gated entrance (XLS-70).** A pool can require that a depositing account holds an accepted Credential from an issuer the pool trusts. Every node reads it from the ledger itself at submit and again at endorse; a sidecar signs nothing its own node did not accept; `Expiration` is judged by the ledger's own clock. The holder accepts the credential with their own signature in the app; the issuer page is a demo issuer on testnet, the gate on the ledger is real. It is an engineering switch per pool — gatekeeping on a trusted issuer's say-so at the public boundary, nothing about anyone's notes.

2. **Proof of funds for a lender — the flow, built end to end, and its honest verdict.** A holder proves "I control at least X XRP in the pool" against a lender's challenge (borrower account, lender id, ledger window) without revealing more than X. The proof is the same Orchard circuit under its own signature domain; it is *refused by every apply path* (it can never move funds) and checked *read-only* by every node — nothing is logged, nothing persisted. The quorum then **records the verdict on the ledger** as an expiring XLS-70 Credential on the borrower's account (every signer re-verifies the proof on its own node at signing time and signs only its node's own fields), the borrower confirms with their signature, and a loan broker reads one credential — no file, no API. The credential says "controlled ≥ X as of ledger L", not "controls it now": an underwriting signal, a snapshot, not a lock. **What we found and say plainly:** on the unchanged circuit the proof reveals the nullifiers of the proven notes to whoever holds it (the node operators; in the package variant, the lender), so the holder's *next spend* of those notes becomes linkable to the proof. A privacy property that depends on the user refreshing their notes in the right order is not a property of the protocol — so this flow ships as a **demo, labelled as degrading the holder's privacy**, and the committed next step is a circuit that proves control of unspent notes without revealing nullifiers.

**What makes it special.** Compliance and privacy are usually traded against each other; here the ledger's own primitives — Credentials, SignerLists, memos — carry the compliance, and the zero-knowledge pool carries the privacy. Every node is independent and ledger-reconstructible; every signing path is audited and mutation-tested before it ships; every surface says plainly what it does and does not prove — including, when our own audit found a privacy cost in a feature we had just shipped, relabelling it the same day rather than shipping it as a feature.

**Honest limits.** The pool account and proof verification are federated (k of N): collusion of k signers can drain the pool account but cannot spend anyone's notes. Shield and unshield amounts and addresses are public by construction; an empty pool has no privacy. The quorum exists because today's XRPL has no on-ledger Halo 2 verifier. Testnet only.

**Links.** Live app `https://confidant.im` · how it works `/how-it-works.html` · lender page `/lender.html` · issuer page `/issuer.html` · repository with the design (`SPEC.md`), the audit record (`docs/audit/`) and the demo script (`docs/DEMO.md`).

FYI, our crypto/ZK lead was unable to join the hackathon in person and is attending remotely.
```

## Technical description (≤ 1000 characters)

```markdown
Rust core (`confidant-core`): Orchard/Halo 2 circuit reused unchanged; a `Wallet` builds, proves and signs bundles; a `Verifier` re-verifies proofs, nullifiers and the note commitment tree and emits anchor memos; `rebuild_checked` reconstructs state from the ledger. Rust verifier nodes (axum) read XRPL by JSON-RPC only and hold no keys; Node.js signer sidecars (xrpl.js) each hold one SignerList key and sign only what their own node endorsed; a coordinator multisigns and submits. Browser wallet = the core compiled to WebAssembly (multithreaded proving under COOP/COEP, ~3 s per proof). Hackathon additions: XLS-70 credential gate (ledger-read, fail-closed, expiry by the ledger's clock), proof of funds (own op tag refused by every apply path, read-only verify, canonical challenge bytes recomputed by each node, bounded ingress; demo-grade until a nullifier-free circuit), quorum-signed `CredentialCreate` with node-derived fields, an x402 relay paid in RLUSD on XRPL EVM. 100% core / 95% node coverage gates, mutation tables, audit REQ/RESP per slice.
```

## Logo

`docs/assets/logo.svg` (source) and `docs/assets/logo.png` (1024 × 1024, rendered from it): a shield — the pool — holding a
transfer graph drawn as fading dashed edges, only the endpoints visible; the stylized wordmark `C0nf!d@nt` (logo only, per
the branding rule) and the line "SHIELDED POOL · XRP LEDGER". Violet `#7919FF` on deep ink, the UI's accent. Upload the
PNG to the form (the SVG loads Google Fonts for the wordmark; the PNG has them baked in).

## Change log

- 2026-10-24 — first draft, end of day 1: A and C live; Mastercard-track relay code complete.
- 2026-10-24 (later) — remote-attendance note in the description; logo added (`docs/assets/`).
- 2026-10-25 — the proof-of-funds ruling (demo-grade, privacy cost stated) and the Mastercard-track run folded in; counts updated.
