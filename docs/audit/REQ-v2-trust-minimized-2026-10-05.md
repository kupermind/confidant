# REQ — v2: trust-minimized architecture (design) — 2026-10-05

**From:** the Rust-core dev session. **To:** the audit session. **Asks for:** DESIGN-PASS (or
CONDITIONAL) for the v2 design before its core slices land. Owner's decision 2026-10-05: do steps
1–4 this week. Design text: `SPEC.md` §v2, `SPEC.ru.md` §16, status `ROADMAP.md` §5a.

## 1. Goal and ceiling

Leave the single trusted operator (v1: custodial) and reach decentralization **comparable** to
Railgun/Tornado on today's XRPL. Ceiling without an amendment: nobody on-ledger verifies a Halo 2
proof, so proof verification and the pool account sit with a **k-of-N quorum** of independent
verifiers. Honest label after v2: non-custodial notes; federated custody and verification; state
reconstructible from the ledger; collusion of k can drain the pool account but cannot spend
anyone's notes.

## 2. Components

- **Wallet** (client, local process): holds the spending key; tracks its notes by trial-decrypting
  the public log; keeps a replica of the commitment tree (rebuilt from the log) to compute
  witnesses; builds, proves and signs bundles. Never talks to XRPL itself (the FE does).
- **Verifier** (any party, N instances): tree, nullifier set, **consumed-deposit set**, bundle
  log, the v1 verify-and-apply tail unchanged, anchor-memo codec, rebuild from memos + a mirror.
  Holds no keys; its state is public.
- **Coordinator** (any party): assembles ≥ k signer signatures over the multisigned anchor /
  payout transactions and submits them.

## 3. Deposit binding (the new mechanism)

Problem: once the client builds the shield, the deposit tx hash is public and a stranger could
present *some* bundle against it. v1 did not have this because the operator linked deposit and
note.

Design: the wallet builds the shield bundle first (output to its own address) and computes
`bundleHash = BLAKE2b-256(personal "ConfidantBundle")(canonical bundle bytes)` — anchor, flags,
value balance, every action's public parts, proof, binding signature; no metadata. The deposit
Payment carries memo `confidant/shield` = `bundleHash`. A verifier accepts a shield only if:
(a) the API layer / signer saw the deposit **validated** on-ledger as a Payment to the pool
account; (b) `delivered_amount == −valueBalance`; (c) memo hash == hash of the submitted bundle;
(d) the deposit tx hash is not in the consumed-deposit set; then the v1 tail; then the deposit is
marked consumed. The per-op anchor memo for a shield carries the deposit hash, so the ledger alone
shows which deposit backs which shield.

Consequence: the shield's sighash external context can no longer be the deposit tx hash (the
bundle must exist before the deposit is sent). It becomes **empty** for shields; the binding
deposit ↔ bundle moves to the memo hash + the consumed set + the anchor memo. Transfer and unshield
sighash contexts are unchanged (unshield still binds destination ‖ amount from `value_balance`).

## 4. State on the ledger

One anchor memo per accepted op, type `confidant/anchor`, ≤ 1 KB:
`version(1) ‖ op(1) ‖ root(32) ‖ bundleHash(32) ‖ extLen(1) ‖ ext ‖ n(1) ‖ n × (nullifier(32) ‖ cmx(32))`
with `ext` = deposit hash (shield, 32 B) / ASCII destination (unshield) / empty (transfer).
Proof bytes live off-ledger, served by any verifier (`GET /log`) or mirror, addressed by hash.

Rebuild by a new node: read the pool account's transactions → decode memos in order → fetch each
bundle by hash → check hash → run the v1 verify tail with `op`/`ext` from the memo → apply → the
resulting root must equal the memo's root. A new node therefore trusts: the circuit, and that the
k-signed anchors were produced by signers who validated the deposits (it can re-validate deposits
itself via `account_tx`, which the harness does).

## 5. k-of-N verifiers

Pool account: `SignerListSet` with quorum k of N signer keys, then `AccountSet asfDisableMaster`.
Every signer runs its own Verifier (own state, rebuilt from ledger + mirror), independently
re-checks the proof and, for shields, the deposit on-ledger and its memo binding, and only then
signs the anchor (and the payout for unshields). The coordinator submits with ≥ k signatures.
XLS-56 Batch for atomic anchor + payout where enabled on testnet; otherwise anchor first, payout
second (the sighash already binds destination and amount). Hackathon shape: 3 signers, quorum 2,
run as independent verifier states inside the testnet harness.

## 6. Relaying

Any client submits to any verifier; any party runs a verifier from the ledger + a mirror. The
coordinator can delay, not steal / mint / forge. The DoubleSpend-vs-InvalidProof oracle caveat from
PR #2 is unchanged: the nullifier set stays public (now on-ledger), so no new leak.

## 7. Negative suite I commit to (in addition to v1's)

1. Same deposit presented for two shields ⇒ second refused (consumed set), state unchanged.
2. Deposit memo hash ≠ bundle hash ⇒ refused. 3. `delivered_amount ≠ −valueBalance` ⇒ refused.
4. A stranger's bundle against someone else's deposit ⇒ refused (hash mismatch); the rightful
   bundle is then accepted. 5. Anchor memo round-trip exact; size ≤ 1 KB at MAX_OUTPUTS.
6. Rebuild from memos + mirror reaches the same root; a mirror bundle whose hash ≠ memo hash ⇒
   refused; a memo whose root ≠ replayed root ⇒ refused. 7. Wallet sync: notes/spent status equal
   to v1's operator view; a wallet that never saw the log cannot build (no witnesses). 8. Two
   verifiers fed the same log have identical state; a verifier that refused a bundle diverges and
   refuses to sign (harness, testnet). Every new guard mutation-tested; 100% line coverage kept.

## 8. Questions

- **Q-V2-1** Dropping the deposit tx hash from the shield sighash (§3) in exchange for memo hash +
  consumed set + anchor memo — sound? Anything else a shield must bind?
- **Q-V2-2** `bundleHash` definition and personalization (§3) — any field that must or must not be
  inside?
- **Q-V2-3** Anchor memo format (§4) — fields, ordering, the 1 KB bound, versioning byte.
- **Q-V2-4** What a rebuilding node is entitled to trust (§4) — is "k-signed anchor + own proof
  re-verification + optional deposit re-validation" the right statement?
- **Q-V2-5** Signer duties (§5): each re-checks proof + deposit before signing; coordinator-owned
  ordering. Anything the signer must additionally check (e.g. memo root vs own state before
  signing the anchor)?
- **Q-V2-6** Label wording in §1 — acceptable?
- **Q-V2-7** Order of delivery for a one-week window: core (Wallet/Verifier + deposit binding +
  memo codec + rebuild) first as one slice, then the harness (signers, scanner). Objections?
