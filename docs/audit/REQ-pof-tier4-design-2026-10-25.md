# REQ — proof of funds, Tier 4: a circuit that proves control of unspent notes ≥ X without revealing nullifiers (design)

Sent 2026-10-25 from the implementation session to the audit session, verbatim, on the project lead's instruction
after the lead's ruling and the audit's reversal of the earlier design verdict. Nothing built; the RESP is a
design ruling with implementation pins.

```text
Your reversal is recorded verbatim (docs/audit/RESP-pof-design-reversal-2026-10-25.md on PR #85, the four PoF INDEX rows annotated "mechanics only; design REJECTED until a nullifier-free circuit"). The lead has now asked me to design the correct thing WITH you. This is a DESIGN-REQ for Tier 4 — a proof of control of unspent notes ≥ X that reveals no nullifier, no commitment, no total — against the lead's bar: no human as a link, no regression of graph-hiding. Please rule on the statement and the questions; implementation is post-event and gets its own slices.

== Statement the circuit proves (k note slots, fixed) ==
Public inputs: R_S (the pool's note-commitment root at state S), N_S (the root of the nullifier set at the same state S — see D1), X (threshold, u64), c = H(challenge) (BLAKE2b-256 of the canonical challenge bytes borrower|X|L|E|lender, reduced to a Pallas field element). Nothing else is public; k is a circuit constant.
For each slot i with witness (spending key sk, note_i = (addr_i, v_i, ρ_i, ψ_i, rcm_i), Merkle path π_i in the commitment tree, non-membership witness ω_i in the nullifier tree):
 (a) ownership — ak, nk, rivk derived from sk in-circuit (as Orchard's spend half), ivk = CommitIvk(ak, nk, rivk), addr_i's pk_d = [ivk] g_d — i.e. the note is addressed to a key this prover holds;
 (b) membership — cm_i = NoteCommit(addr_i, v_i, ρ_i, ψ_i, rcm_i) and MerkleCRH-path π_i leads to R_S (Sinsemilla, as Orchard);
 (c) nullifier, never exposed — nf_i = DeriveNullifier(nk, ρ_i, ψ_i, cm_i) computed in-circuit (Poseidon + fixed-base mul + Extract_P, bit-exact with orchard::Note::nullifier);
 (d) unspent at S — non-membership of nf_i in the nullifier set at S: an indexed (sorted, "next-pointer") Merkle tree over all nullifiers the pool has ever seen, Poseidon-hashed; ω_i = a leaf (a, next) with path to N_S and the in-circuit range checks a < nf_i < next (next = ∞ sentinel for the largest);
 (e) dummies — a slot with v_i = 0 skips (b) and (d) (Orchard's dummy-spend rule) so k is a maximum, not the count;
 (f) distinctness — nf_1 < nf_2 < … over the non-dummy slots (strict order enforced in-circuit; dummies placed last), so no note is counted twice;
 (g) threshold — Σ v_i ≥ X, each v_i and the difference range-checked to 64 bits.
Verifier (a node) checks the proof against ITS OWN (R_S, N_S) for one state S — the two roots must come from the same log position — and c against the challenge it recomputes (as today). Verdict as today; the on-ledger credential path as today. What the lender receives is (proof, public inputs): no nullifiers, no commitments, nothing linkable to any later spend — the package is safe to hand over and to re-verify later against the same S.

== Design decisions I want ruled ==
D1 — The nullifier set as an indexed Merkle tree in every node, built deterministically from the log (insert in record order), root N_S part of the state at every log index alongside R_S; reconstructible by anyone from the public log (so a third party can re-verify "as of S" by rebuilding). Not anchored on-ledger (optional later: add N to the anchor memo).
D2 — Snapshot semantics are now CRYPTOGRAPHIC: "controlled ≥ X at state S" is what the statement proves; nothing the holder does afterwards is needed or useful; a later spend of the proven notes is unlinkable because nothing of them was revealed. This replaces self-rotation entirely (no rotation step exists).
D3 — No signatures: the proof is bound to the challenge by the public input c; a third party presenting the proof cannot change c or X; a node's verdict names (X, c, S). No value commitments, no binding signature, no rk — the Orchard authorization machinery is not needed for a read-only statement.
D4 — A new Halo 2 (IPA, Pallas) circuit with its own proving/verifying key; NOT an Orchard Action; it is not a BundleRecord and cannot reach any apply path (different type); the #31 hash-domain inventory gains its challenge-hash domain. Gadgets: halo2_gadgets (Sinsemilla Merkle, Poseidon, ECC) + Orchard's NoteCommit/CommitIvk/DeriveNullifier gadgets if orchard 0.15 exposes them (checking the feature flag now; else ~300 lines vendored under its licence), bit-exactness against orchard tested on random notes.
D5 — Cost: per slot ≈ one Orchard spend's worth of constraints (ownership + membership + nullifier) plus a Poseidon path of depth ~32 and two 255-bit comparisons; k = 4 slots ≈ one 4-action Orchard bundle: seconds in the browser on 16 threads, verification tens of ms on a node; the node's ingress bounds (256 KiB, 4 permits) carry over.
D6 — The shipped nullifier-revealing path: the endpoint, the challenge plumbing, the bounds and the sidecar kind are reusable with the proof type swapped; the revealing proof type itself is removed from the product path (what the owner shows at the demo is his call; the repo labels it as rejected until Tier 4).

== Questions ==
Q-T4-1 Is the statement (a)–(g) complete and sufficient for "controls unspent notes worth ≥ X at S"? Anything missing (e.g. must we also bind the note's ρ/ψ to the tree position to prevent a note being "re-addressed"? Orchard's note-commit already binds them)?
Q-T4-2 Non-membership by an indexed Merkle tree at the SAME log position as R_S: any concern about the pairing (a node that answers with roots from two different positions) — I intend to make (R_S, N_S) one atomic snapshot object in the verifier state so they cannot be mixed.
Q-T4-3 Leaks: public (X, c, S, k). Does the circuit size k leak anything worth hiding? Does the node's verdict (ok at S) leak more than today's? Timing of the request itself is the only metadata (as today: lender/borrower/X in clear to the node operator — now with NO note-level linkage behind it).
Q-T4-4 Is binding the challenge as a public input (D3) sufficient, or do you want a knowledge-of-sk signature as well (I see none needed: ownership is proven in-circuit)?
Q-T4-5 Dummy slots and the strict-order distinctness (e),(f): is "nf ordering" the right anti-double-count, or do you prefer distinct commitments? (Nullifier order also keeps dummies trivially excluded.)
Q-T4-6 Anything you want pinned now for the eventual IMPL slices: bit-exactness of in-circuit nf with orchard's; the atomic (R, N) snapshot; the circuit's own proving-key domain; no path from this proof into any apply path; mutation rows on each clause (a)–(g).
Q-T4-7 Interim: the lead has rejected every operational mitigation (rotation, as-of-L on the old proof); do you want the repository to disable the revealing endpoint outright (feature-flagged off by default) until Tier 4, or is the label enough for a testnet demo? That is the owner's call to make, but your recommendation goes to him.
```

Gadget visibility, checked after sending: orchard 0.15.5's `note_commit`, `commit_ivk` and `derive_nullifier`
gadgets are `pub(in crate::circuit)` even behind the `unstable-voting-circuits` feature; the slot gadgets are
re-implemented on `halo2_gadgets` 0.5 with bit-exactness tests (pin 1 of the RESP).
