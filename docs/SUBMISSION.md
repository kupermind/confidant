# Submission text — AI & Fintech XRPL Hackathon, NYC, Oct 24–25 2026

The three answers the submission form asks for, kept here as the single source so the form, the slides and the
README say the same thing. **Living document:** update the "Status" line and the counts whenever something ships;
the honest-label rules of `SPEC.md` apply to every sentence (never "trustless", "untraceable", "fully anonymous",
"private from the operator"; the quorum is federated; shield/unshield amounts and addresses are public; screening
and the credential gate are checks at the public boundary, not proofs about any note).

Formal name everywhere in this text: **Confidant**. The stylized wordmark `C0nf!d@nt` is for the logo only.

Limits (from the form): short description ≤ 200 characters · description ≤ 5000 · technical description ≤ 1000.
Markdown is accepted in all three. Counts are of the text between the fences (`wc -m` on each block): 177 / 4997 / 989 on 2026-10-25; the Mastercard form's two answers: 2885 / 2972.

**Status (2026-10-25, day 2, night):** everything below is **live on the XRPL testnet** at `https://confidant.im` — the
pre-existing pool (baseline tag `pre-hackathon`), plus, built on-site and audited: the credential gate (A) with a demo
issuer service so anyone can enter from a phone; proof of funds with the quorum-signed on-ledger Credential (C), one
button for the holder and one address for the lender; the under-the-hood page; the Mastercard-track relay and agent
with proof-of-funds underwriting and signed receipts anyone can re-check (four live runs, the last two with the
receipt). **Ruling of 2026-10-25 (lead, audit concurring):** the proof of funds as built reveals the proven notes'
nullifiers to the signers' nodes, so it is a **demo of the flow, labelled as degrading the holder's privacy — not the
product feature**; the accepted design (`docs/design/TIER4-PROOF-OF-FUNDS-REWRITE.md`) is a circuit that proves
control of unspent notes ≥ X without revealing nullifiers, the committed next step; the demo nodes run the revealing
path only with an explicit flag. The texts below say so.

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

1. **Credential-gated entrance (XLS-70).** A pool can require that a depositing account holds an accepted Credential from an issuer the pool trusts. Every node reads it from the ledger itself at submit and at endorse; a sidecar signs nothing its own node did not accept; `Expiration` is judged by the ledger's own clock. KYC in demo mode: a demo issuer registers anyone who asks, rate-limited (open registration — the model an exchange uses, where an account is opened first and KYC raises its limits); the holder's account accepts the credential with its own signature, under one press; the gate on the ledger is real. It is an engineering switch per pool — gatekeeping on a trusted issuer's say-so at the public boundary, nothing about anyone's notes.

2. **Proof of funds for a lender — the flow, built end to end, and its honest verdict.** A holder proves "I control at least X XRP in the pool" against a challenge (borrower account, a request id, ledger window) without revealing more than X. The proof is the same Orchard circuit under its own signature domain; it is *refused by every apply path* (it can never move funds) and checked *read-only* by every node — nothing is logged, nothing persisted. The quorum then **records the verdict on the ledger** as an expiring XLS-70 Credential on the borrower's account (every signer re-verifies the proof on its own node at signing time and signs only its node's own fields), the borrower confirms with their signature, and a loan broker reads one credential — no file, no API. The holder presses one button; the lender types one address. The credential says "controlled ≥ X as of ledger L", not "controls it now": an underwriting signal, a snapshot, not a lock. **What we found and say plainly:** on the unchanged circuit the proof reveals the nullifiers of the proven notes to whoever holds it (the signers' node operators), so the holder's *next spend* of those notes becomes linkable to the proof; the wallet therefore refreshes its notes by itself right after proving. A privacy property that depends on a refresh step is not a property of the protocol — so this flow ships as a **demo, labelled as degrading the holder's privacy**, and the committed next step is a circuit that proves control of unspent notes without revealing nullifiers.

**What makes it special.** Compliance and privacy are usually traded against each other; here the ledger's own primitives — Credentials, SignerLists, memos — carry the compliance, and the zero-knowledge pool carries the privacy. Every node is independent and ledger-reconstructible; every signing path is audited and mutation-tested before it ships; every surface says plainly what it does and does not prove — including relabelling a feature the day our own audit found its privacy cost.

**Also built: the Mastercard track.** An agent proves its shielded funds, then pays RLUSD over x402 to relay a private transfer; a receipt signed by both parties checks from public data.

**Honest limits.** The pool account and proof verification are federated (k of N): collusion of k signers can drain the pool account but cannot spend anyone's notes. Shield and unshield amounts and addresses are public by construction; an empty pool has no privacy. The quorum exists because today's XRPL has no on-ledger Halo 2 verifier. Testnet only.

**Links.** Live app `https://confidant.im` · how it works `/how-it-works.html` · lender page `/lender.html` · under the hood `/issuer.html` · repository: design `SPEC.md`, audit record `docs/audit/`, demo script `docs/DEMO.md`.

FYI, our crypto/ZK lead was unable to join the hackathon in person and is attending remotely.
```

## Technical description (≤ 1000 characters)

```markdown
Rust core (`confidant-core`): Orchard/Halo 2 circuit reused unchanged; a `Wallet` builds, proves and signs bundles; a `Verifier` re-verifies proofs, nullifiers and the commitment tree, emits anchor memos and rebuilds state from the ledger. Rust verifier nodes (axum) read XRPL by JSON-RPC only, hold no keys; Node.js signer sidecars (xrpl.js) each hold one SignerList key and sign only what their own node endorsed; a coordinator multisigns and submits. Browser wallet = the core as WebAssembly (multithreaded proving, ~3 s per proof). Hackathon additions: XLS-70 credential gate (ledger-read, fail-closed, expiry by the ledger's clock); proof of funds (own op tag refused by every apply path, read-only verify, challenge bytes recomputed per node, bounded ingress; demo-grade until a nullifier-free circuit); quorum-signed `CredentialCreate` with node-derived fields; an x402 relay paid in RLUSD on XRPL EVM. 100% core / 95% node coverage gates, mutation tables, audit REQ/RESP per slice.
```

## Mastercard AP4M form

Two long answers (≤ 3000 characters each). Counts are of the text between the fences. The short one (≤ 900) for the
sponsor's other field is in `hackathon/ap4m/STAGE.md` §3, with the evidence links.

### 3. Project Description (≤ 3000)

```markdown
Confidant is a compliant shielded pool on the XRP Ledger (testnet). XRP is shielded into a pool account, transferred privately inside it with Halo 2 zero-knowledge proofs built in the user's browser (the Orchard circuit, reused unchanged), and unshielded to any XRPL address. Sender, receiver and amount of an inside transfer never reach the ledger; only a 32-byte anchor memo does. A k-of-N quorum of independent verifier nodes (an XRPL SignerList on the pool account, master key disabled) re-verifies every proof on its own state and signs only roots it computed itself; every accepted operation is anchored in a memo, so anyone can rebuild the pool's state from the ledger alone. Compliance uses the ledger's own primitives: an XLS-70 credential gate at the entrance (every node reads the credential from the ledger before it signs) and a proof of funds — "I control at least X XRP in the pool" — that the quorum writes on the holder's account as an expiring XLS-70 credential, which a lender reads by address: no file, no API.

For the Mastercard track we built the agentic side on top. A scripted agent holds shielded funds and needs to pay a supplier without publishing it. Privacy is a metered service: the agent pays per relay, in RLUSD over x402 through AP4M, to a relay that forwards its zero-knowledge transfer to the quorum. Before it may pay, the agent proves its shielded funds to the quorum and the relay underwrites it from the ledger; after the anchor, a receipt signed by the merchant and countersigned by the agent binds the paid claim, the bundle and the credential, and a small verifier re-checks it from public data only. Live on testnet: four full runs on 2026-10-25, the last two with the proof of funds and the receipt, 80–92 s from start to RLUSD settlement.

Built at the hackathon (the pool itself pre-dates it and is tagged as the baseline): the credential gate and its demo issuer service, the proof of funds and its on-ledger credential, the one-button holder flow and the lender page, the AP4M relay, agent, receipts and verifier. Every signing path was audited and mutation-tested before it shipped.

Honest limits: the pool account is federated (k signers can drain it, never spend a note); shield and unshield amounts and addresses are public; the proof of funds is demo-grade (the proof reveals the proven notes' nullifiers to the signers' nodes, so the wallet refreshes its notes right after; the accepted design is a nullifier-free circuit, the committed next step); the KYC gate is real and runs in demo mode: the demo issuer's admission policy is open registration (it issues the entry credential to anyone who asks, rate-limited — the model an exchange uses, where an account is opened first and KYC raises its limits), and a production deployment runs its own issuer policy against the same gate; the metered payment is public on AP4M's rail; testnet only; the AP4M kit is for the hackathon.
```

### 4. How does your solution use Mastercard AP4M / Agentic Payments? (≤ 3000)

```markdown
AP4M is the payment rail of the relay, used through the hackathon SDK unmodified, live on XRPL EVM Testnet (chain 1449000):

1. The relay is an x402 merchant (the kit's createX402Merchant): GET /resource answers 402 with the price (0.1 RLUSD), the PaymentLedger and the channel binding.
2. The agent reads the 402 and decides under its own policy — price ≤ its maximum (0.25 RLUSD) and budget left on its funded channel — with no human in the loop. The refusal path is real: on an over-cap channel it stops with exit code 3 and signs nothing (the ledger would refuse the voucher anyway: settled ≤ redeemed ≤ cap).
3. It pays by signing an EIP-712 voucher on its channel (the private key never leaves the laptop); the voucher is redeemed on the PaymentLedger — VoucherRedeemed(channel, claim), the on-chain proof.
4. The relay accepts only what it reads on-chain — the VoucherRedeemed event for (channel, claim), the amount ≥ the price, the claim unused — never the agent's word; then it forwards exactly one bundle to the pool's quorum, which anchors it on XRPL L1 (memo: root and bundle hash; no sender, receiver or amount).
5. Settlement is AP4M's: RLUSD reaches the merchant payout address; the evidence page shows the receipt.

What we add on top of the rail is the agentic part. Machine-to-machine underwriting: before paying, the agent proves "I control ≥ X XRP of shielded funds" to the pool's quorum, which writes an expiring XLS-70 credential on the agent's XRPL account; the relay serves only an account that carries one, read from the ledger against its threshold. And a receipt a judge can verify alone: the relay signs {claim, channel, bundle hash, anchor, agent account, credential URI} with the merchant payout key (the 402's payTo), the agent countersigns with its channel key and its XRPL key, and verify-receipt.mjs re-checks both signatures, the VoucherRedeemed event, the pool's anchor memo naming the bundle hash, and the credential — from public data only. Why signatures: the kit fixes the resource at /resource and derives claim ids from the voucher alone, so the payment itself cannot carry the bundle hash; we say so.

Evidence (2026-10-25): run 4, the relay on the demo host behind TLS, 80 s wall — VoucherRedeemed 0x565e3c2a…, anchor 302AB16D…, payout 0x6a06e410…, receipt verified 10/10; run 3 (92 s), runs 1–2 and the over-cap refusal, with full links: hackathon/ap4m/README.md.

Against the criteria: autonomy — the agent reads the 402 and decides inside a policy; use case — the payment is load-bearing (no payment, no relay, no private settlement); on-chain proof — VoucherRedeemed at the PaymentLedger; RLUSD settlement — the payout address received it; live demo — the whole flow in under two minutes; creativity — a privacy service metered per use, underwritten by a zero-knowledge proof of funds, with receipts across two ledgers. Honest line: the metered payment is public, the settlement is private, the proof of funds is demo-grade.
```

## Logo

`docs/assets/logo.svg` (source) and `docs/assets/logo.png` (1024 × 1024, rendered from it in Chrome with the real font):
a shield — the pool — holding a transfer graph drawn as fading dashed edges, only the endpoints visible; the stylized
wordmark `C0nf!d@nt` (logo only, per the branding rule; the second glyph is a zero and carries a slash through its
counter so it does not read as an O) and the line "SHIELDED POOL · CREDENTIAL GATE · PROOF OF FUNDS" — the pool and the
two hackathon features, named as features. Violet `#7919FF` on deep ink, the UI's accent. Upload the PNG to the form (the
SVG loads Google Fonts for the wordmark and positions the slash for that font; the PNG has it all baked in).
`docs/assets/icon.svg` is the shield alone — the website icon, served as `frontend/favicon.svg` with PNG fallbacks on
every page.

## Change log

- 2026-10-24 — first draft, end of day 1: A and C live; Mastercard-track relay code complete.
- 2026-10-24 (later) — remote-attendance note in the description; logo added (`docs/assets/`).
- 2026-10-25 — the proof-of-funds ruling (demo-grade, privacy cost stated) and the Mastercard-track run folded in; counts updated.
- 2026-10-25 (later) — the one-button Prove funds / one-field lender page (`docs/UI-RULE.md`) reflected in the description; counts updated.
- 2026-10-25 (evening) — the entry credential issued under the hood; the issuer page became the under-the-hood page; counts updated.
- 2026-10-25 (night) — logo: slashed zero, the tagline names the two features, the shield becomes the site icon.
- 2026-10-25 (late night) — status refreshed (issuer service, one-button flow, Mastercard receipts, Tier-4 doc, the flag); the description gains the Mastercard sentence; the Mastercard AP4M form's two long answers added.
- 2026-10-25 (morning) — the audit's label for the demo issuer on every surface: KYC in demo mode — the gate is the real system, the issuer's admission policy allow-by-default for the demo; counts updated.
- 2026-10-25 (day 2) — demo-issuer label reframed per the project lead: open registration (the exchange model — an account is opened first, KYC raises its limits); the "placeholder / standard-practice / not-a-vetting / outside-scope" phrasing dropped; the gate stays the real system, the honest fact (registers anyone who asks, rate-limited) kept.
