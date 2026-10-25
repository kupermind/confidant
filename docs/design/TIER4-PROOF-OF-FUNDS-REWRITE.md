# Tier-4 Proof-of-Funds — design, rewrite guide, and the rules that must not be violated

**Status:** design ruled by the audit session (2026-10-25), implementation hand-off for the Confidant
implementer session. The currently-shipped proof-of-funds (the nullifier-revealing proof) is **rejected
at the design level** and must be removed from the product path. This document is the authoritative
target to implement against. Implementation is post-event and lands in slices; **each slice gets its own
firsthand IMPL-PASS from the audit session before it merges** (ship rule — a new signing/verification
path never ships unverified).

---

## 0. THE CHARTER — rules that are FORBIDDEN to violate

These are hard gates, not trade-offs. A design or an implementation that touches any of them is rejected
outright; there is no "documented known cost", no "but the mechanics are clean", no operational
mitigation that buys an exception. Read them before writing a line.

### 0.1 The two absolute prohibitions (owner, 2026-10-25)

- **P1 — A human may never be a link in a security or privacy property.** If preserving a guarantee
  depends on the user *doing something* — rotating notes, acting in a certain order, remembering a
  hygiene step — the design has already failed. A user action is not a security control. The shipped PoF
  failed here: its only defence against linkage was the holder self-rotating the proven notes, in order.
  Forbidden.
- **P2 — A new feature may not regress a guarantee the system already had.** The feature must produce its
  proof *without weakening any prior property*, or the feature does not exist. Confidant's load-bearing
  property is **graph-hiding**: an operator cannot link a deposit to a later spend. Any feature that lets
  an operator make that link — even "mitigated" — is rejected. The only acceptable fix is the design that
  preserves the property *by construction*; mitigations ("valid as of ledger L", atomic rotation,
  state-at-L on the old proof) do **not** count.

Corollary for the implementer: the proof must reveal **nothing that recurs when the note is later
spent** — above all, **no nullifier**. If any public output of the proof reappears at spend time, the
link exists and the design has failed.

### 0.2 Soundness invariants — each is load-bearing; violating one forges funds or breaks the signal

- **C1 — Bit-exact nullifier.** The nullifier derived inside the circuit MUST equal
  `orchard::Note::nullifier` bit-for-bit, proven by a test over random notes. If it differs by one bit,
  the unspent check (C3) is performed against a nullifier that will never appear at spend time — it
  proves nothing. This is the unspent lynchpin.
- **C2 — The two roots are one atomic snapshot.** The commitment root `R_S` and the nullifier-set root
  `N_S` MUST come from the same log position `S`, carried as one snapshot object, and the verifier MUST
  reject any unpaired pair. If they can drift, a note spent between `S1` and `S2` is membership-valid at
  `R_S1` and non-membership-valid at `N_S2` → a spent note proves unspent.
- **C3 — The nullifier tree equals the spent set, exactly.** `N_S` MUST commit to *exactly* the
  nullifiers the pool has seen as of `S` — the same set `Verifier::is_spent` answers from — rebuilt
  deterministically from the log. A single missing nullifier = a spent note that proves unspent. Pin a
  test that the indexed tree's membership equals the authoritative spent set across a full log replay.
- **C4 — The proof can never reach an apply path.** The PoF proof is its own type — **not** an Orchard
  Action, **not** a `BundleRecord` — and cannot be decoded or applied as a spend anywhere. Enforce by
  type AND prove it: drive a PoF proof through every mutating entrypoint and assert it is refused (the
  shape of the #31 apply-path-guards test). A survived mutation here is a spend forged from a read proof.
- **C5 — Threshold arithmetic sized to the real range.** `Σ vᵢ` over `k` slots can reach `k·2⁶⁴`. Range-
  check the **sum** and the comparison to the sum's true range, not to 64 bits — a 64-bit difference
  check is both a latent false-refusal for large legitimate balances and a possible wrap that passes a
  short sum as `≥ X`. (This is the XRPL 64-bit-sum-overflow lesson; do not repeat it in-circuit.)
- **C6 — `k` must never force pre-consolidation.** If a holder has to consolidate notes to fit them into
  the `k` fixed slots before they can prove, that consolidation is a spend — it reveals those nullifiers
  AND is a user action — which reintroduces exactly the failure this whole rewrite removes (P1 + P2).
  Size `k` so a holder proving a realistic `X` never needs to consolidate first; never make consolidation
  a precondition of proving.
- **C7 — Freshness binds the snapshot to the challenge.** "Unspent as of `S`" is only a present-tense
  signal if `S` is recent. Bind the proof's `S` to the challenge's ledger index `L`, and keep the node's
  existing recency window, or a now-spent note proves "unspent as of an old `S`".

### 0.3 What the charter does NOT relax

The properties already audited and passed on the surrounding machinery stay in force and must be
preserved by the rewrite: the node recomputes the challenge binding itself (never echoes a caller's
bytes); every ledger read fails closed (refusal, never a derive-anyway); the sidecar signs only on its
own node's verdict (no-blind-sign); the ingress bounds (body cap + concurrency permits) stay; the honest
label stays honest. Swapping the proof must not weaken any of these.

---

## 1. Why the shipped proof-of-funds is rejected (so it is not rebuilt by habit)

The shipped proof reused the unchanged Orchard **unshield-bundle** shape. An unshield bundle carries a
Spend action per note, and each Spend **reveals that note's nullifier** to every verifying node (the core
verify checks the nullifier unspent — it must see it). A nullifier is exactly what reappears when the
note is later actually spent. So a verifying operator who saw the PoF (with its borrower / lender /
amount / timestamp metadata) can, when the note is spent later, **link the proof to the real spend** —
breaking graph-hiding (P2). The only defence offered was the holder self-rotating the proven notes after
the lender checked, in order — a human as the last link (P1). Both prohibitions violated. No amount of
clean fail-closed / no-blind-sign / bounded-issuance machinery rescues it: it is a privacy regression by
construction. It must leave the product path.

---

## 2. The correct design — a dedicated zero-knowledge proof that reveals no nullifier

A new Halo 2 (IPA, Pallas) circuit, with its own proving/verifying key, proving the statement below. It
is **not** an Orchard Action and **not** a `BundleRecord` (C4).

**Public inputs (everything the verifier and the lender see):**
- `R_S` — the pool's note-commitment tree root at state `S`.
- `N_S` — the root of the nullifier set at the **same** state `S` (one atomic snapshot with `R_S`; C2).
- `X` — the threshold (u64).
- `c` — `H(challenge)`: BLAKE2b-256 of the canonical challenge bytes `borrower|X|L|E|lender`, reduced to
  a Pallas field element. (Same challenge the shipped read side already recomputes.)
- `k` is a **circuit constant** (the maximum number of note slots), not a public value and not the count.

**Private witnesses, per slot `i` (spending key `sk`, note `(addr, v, ρ, ψ, rcm)`, commitment Merkle
path `πᵢ`, nullifier non-membership witness `ωᵢ`):**

- **(a) Ownership.** Derive `ak, nk, rivk` from `sk` in-circuit (Orchard's spend half);
  `ivk = CommitIvk(ak, nk, rivk)`; prove the note's `pk_d = [ivk] g_d`. The note is addressed to a key
  this prover holds, and the same `nk` is used for the nullifier in (c) — so ownership and the nullifier
  are the same key; a prover cannot use another's note.
- **(b) Membership.** `cmᵢ = NoteCommit(addr, v, ρ, ψ, rcm)` and the Sinsemilla Merkle path `πᵢ` leads to
  `R_S`. (NoteCommit binds `addr/v/ρ/ψ` into `cm`, and membership binds `cm` to its position — so a note
  cannot be "re-addressed"; nothing further to bind.)
- **(c) Nullifier, never exposed.** `nfᵢ = DeriveNullifier(nk, ρ, ψ, cmᵢ)` computed in-circuit and kept a
  **private witness** — it is not a public input and not an output. (C1: bit-exact with orchard.)
- **(d) Unspent at `S`.** Non-membership of `nfᵢ` in the nullifier set at `S`: an **indexed** (sorted,
  next-pointer) Merkle tree over every nullifier the pool has seen, Poseidon-hashed, root `N_S`. The
  witness `ωᵢ` is a leaf `(a, next)` with a path to `N_S` and in-circuit range checks `a < nfᵢ < next`
  (`next = ∞` sentinel for the largest). Proves `nfᵢ` is absent — the note is unspent — without revealing
  `nfᵢ`. (C3: the tree equals the spent set.)
- **(e) Dummies.** A slot with `v = 0` skips (b) and (d) (Orchard's dummy rule), so `k` is a maximum, not
  the count — this is what hides how many real notes the holder has.
- **(f) Distinctness.** `nf₁ < nf₂ < …` strict order over the non-dummy slots (dummies placed last), so a
  note cannot be counted twice.
- **(g) Threshold.** `Σ vᵢ ≥ X`, with the sum and the comparison range-checked to the sum's true range
  (C5).

**What the verifier does** (unchanged in spirit from the shipped read side): checks the proof against
**its own** `(R_S, N_S)` for one state `S`, and `c` against the challenge it recomputes itself. The
verdict and the on-ledger XLS-70 credential path are as today.

**What the lender receives:** `(proof, public inputs)`. No nullifier, no commitment, no total — nothing
linkable to any later spend. The package is safe to hand over and to re-verify later against the same
`S`. The snapshot "controlled ≥ X at `S`" is now a **cryptographic** fact; nothing the holder does
afterwards is needed or useful — **self-rotation ceases to exist** (P1 satisfied), and a later spend is
unlinkable (P2 satisfied).

No value commitments, no binding signature, no `rk`: the Orchard authorization machinery is for moving
value and is not needed for a read-only statement. Challenge binding is by the public input `c` alone
(the proof is invalid for any other `c`), and ownership is proven in-circuit — no separate signature.

---

## 3. What to rewrite, component by component

1. **New circuit crate/module** (Halo 2, IPA, Pallas): the statement in §2. Gadgets from `halo2_gadgets`
   (Sinsemilla Merkle, Poseidon, ECC) plus Orchard's `NoteCommit`/`CommitIvk`/`DeriveNullifier` gadgets
   if `orchard 0.15` exposes them (check the feature flag; otherwise vendor the ~300 lines under its
   licence). Its **own** proving/verifying key and a distinct hash-domain. Bit-exactness of the in-circuit
   nullifier against `orchard::Note::nullifier` is a build gate (C1).
2. **Indexed nullifier tree in the verifier** (`core`): maintain the nullifier set as an indexed
   (sorted, next-pointer) Poseidon Merkle tree built deterministically from the log in record order,
   exposing a root `N_S`. It must stay equal to the authoritative spent set `is_spent` answers from (C3).
   Reconstructible by anyone from the public log, so a third party can re-verify "as of `S`".
3. **Atomic `(R_S, N_S)` snapshot** (`core`): make the two roots one object in the verifier state, taken
   at one log position; expose it as the thing a PoF is verified against; reject unpaired roots (C2).
   (Optional, later: add `N` to the anchor memo so the pairing is witnessed on-ledger too.)
4. **Swap the verification** (`backend/node`): `verify_proof_of_funds` now verifies the new proof against
   `(R_S, N_S, X, c)` instead of decoding an unshield bundle. Keep the freshness binding `S ↔ L` and the
   recency window (C7). Keep fail-closed on every ledger read.
5. **Remove the revealing proof from the product path**: delete the unshield-bundle PoF proof type from
   the feature; keep the endpoint / challenge plumbing / ingress bounds / sidecar `credential-pof` kind /
   XLS-70 credential issuance (all already audited) and point them at the new proof. **Until the new
   circuit lands, the revealing endpoint is feature-flagged OFF by default** (the audit recommendation to
   the owner; enabling it for a demo is his explicit, deliberate choice — never the shipped default).
6. **Hash-domain inventory** (`SPEC`): add the new circuit's challenge-hash domain to the repository
   inventory alongside the core domains (the inventory is the complete list; the `core/src`-only scan
   stays scoped — name this domain explicitly).

Everything in `backend/quorum` (coordinator + sidecar) and the credential path needs no trust-change — it
already verifies against the node's own verdict; it just verifies a different, non-leaking proof.

---

## 4. Suggested slice order (each merges only on its own firsthand IMPL-PASS)

1. Indexed nullifier tree + `N_S` + the atomic `(R_S, N_S)` snapshot in `core`, with the tree==spent-set
   replay test (C2, C3). No circuit yet.
2. The circuit: statement §2 with the bit-exact-nullifier test (C1), the threshold range-sizing (C5), and
   a mutation row per clause (a)–(g).
3. Wire `verify_proof_of_funds` to the new proof + the freshness binding (C7); the apply-path isolation
   test (C4); feature-flag the old path off (step 3.5 can be done first and immediately).
4. End-to-end on the quorum + the holder wallet proving in-browser; re-confirm the surrounding audited
   properties (challenge recompute, fail-closed reads, no-blind-sign, ingress bounds) still hold.

Flagging the revealing endpoint off (part of 5/3.5) can and should land immediately, ahead of the
circuit work.

---

## 5. Test obligations (the IMPL-PASS gates)

- Bit-exact in-circuit nullifier vs `orchard::Note::nullifier` on random notes (C1).
- Indexed-tree membership == authoritative spent set across a full log replay (C3).
- `(R_S, N_S)` rejected when unpaired / from different positions (C2).
- PoF proof refused at every mutating entrypoint — a read proof can never apply as a spend (C4).
- Threshold: a sum that wraps a 64-bit boundary does not pass as `≥ X`; a large legitimate sum is not
  falsely refused (C5).
- `k` capacity: a holder with a realistic balance proves without any consolidation step (C6).
- Freshness: a proof against a stale `S` is refused; `S` is bound to the challenge `L` (C7).
- A mutation on each circuit clause (a)–(g) is killed.
- The surrounding audited properties regression-tested unchanged (challenge recompute, fail-closed ledger
  reads, no-blind-sign field equality, ingress bounds).

---

## 6. What explicitly stays the same

The challenge semantics and canonical bytes, the honest label ("controlled ≥ X at `S` — an underwriting
signal, a snapshot, not a lock, not proof the borrower owns the notes"), the XLS-70 credential issuance
and its no-blind-sign shape check, the ingress bounds, and the node-recomputes-the-challenge rule. The
rewrite changes *what is proven and what is revealed*, not the plumbing around it.

---

*Authored by the audit session as the implementation target. Questions on the statement or the pins go
back to the audit session; the owner rules anything touching the charter (§0) or the demo/deploy posture
of the revealing endpoint.*
