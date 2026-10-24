# RESP — proof of funds, Tier 4 (no nullifier revealed): DESIGN-PASS with nine implementation pins

From the audit session, 2026-10-25, verbatim. The design of record is `docs/PROOF-OF-FUNDS.md` §5; the pins are
§5.5 there. Implementation is post-event, per slice, each with its own firsthand IMPL-PASS under the ship rule.

```text
This is the right design. It meets the lead's bar on both prohibitions: the proof reveals no nullifier, no commitment, no total, so a later spend is unlinkable — graph-hiding is preserved — and D2 makes the snapshot cryptographic, so there is no rotation and no human in the loop. I verified the premise firsthand: today `state().root` = `tree.current_anchor()` only, and nullifiers are a bare HashSet — so D1's indexed nullifier tree with a root N_S is genuinely new infra, not a relabel. Soundness and leak analysis, then your questions, then the pins.

**Soundness of (a)–(g):** sound for "controls unspent notes worth ≥ X at S." (a) ties the note's pk_d to a key derived from the prover's sk, and (c) derives nf from the same nk — so ownership and the nullifier are the same key; a prover can't borrow another's note. (b)+NoteCommit bind addr/v/ρ/ψ/rcm into cm and cm to its tree position, so "re-addressing" is impossible — that answers Q-T4-1: nothing more to bind, NoteCommit already does it. (d) is the standard indexed-tree non-membership and is sound **conditional on two things I'm making hard gates** (below). (f) nf strict-order prevents double-count (distinct notes → distinct nf → orderable; same note twice → not strictly orderable). (g) threshold — see the range-sizing pin.

**Leak (Q-T4-3):** public is (R_S, N_S, X, c, k). R_S/N_S are pool-wide and already public; X and c the node already sees (the accepted Tier-1 request metadata); **k is a constant with dummies padding, so it leaks nothing about the real note count** — that's exactly why the dummies matter. The verdict (ok at S) leaks nothing more than today. The only metadata is the request itself, with NO note-level linkage behind it. The regression is gone. ✓

**Your questions:**
- **Q-T4-1 — complete?** Yes, with the two soundness pins below. Re-addressing handled by NoteCommit+membership+ownership; no extra ρ/ψ-to-position binding needed.
- **Q-T4-2 — the (R_S, N_S) pairing: HARD GATE, mandatory.** Make them one atomic snapshot object taken at one log position; the verifier rejects any unpaired roots. The exact attack if they drift: a note spent between S1 and S2 is membership-valid at R_S1 and non-membership-valid at N_S2 → a spent note proven unspent. This is the same class as the perp two-tree catch (membership tree vs nullifier tree must be the SAME state). Your intent is right; I'm ruling it non-negotiable.
- **Q-T4-3 — leaks:** covered above; nothing to hide in k. One privacy caveat lives in k's SIZE — see the pins.
- **Q-T4-4 — c-binding sufficient?** Yes, no extra signature. Ownership is proven in-circuit; c as a genuine public input makes the proof non-reusable for a different c by a relayer (Halo2 public inputs are in the verification equation). A knowledge-of-sk signature would be redundant. Just ensure c is a real public input bound in the transcript, not a free/un-constrained one.
- **Q-T4-5 — nf-ordering vs distinct commitments?** nf-ordering, it's correct and cleaner (it also excludes dummies trivially, and (d) already computes nf). Keep it.
- **Q-T4-6 — pins:** yes — enumerated below.
- **Q-T4-7 — interim:** the owner's call, and my recommendation to him is **disable the revealing endpoint by default (feature-flag off), not merely label it.** The lead ruled the revealing proof FORBIDDEN, not weak; a label in the repo doesn't stop a running node from serving it, and enforcement must match a categorical prohibition. If he wants to show the flow at the demo he enables it deliberately for that; the shipped default is off. Label = documentation; flag-off = enforcement. I'm sending him this.

**IMPL pins (gates for the eventual slices — each is load-bearing):**
1. ⭐**Bit-exact nf.** The in-circuit DeriveNullifier must equal `orchard::Note::nullifier` bit-for-bit, tested on random notes. If it differs by a bit, (d) proves non-membership of a phantom nf and the unspent proof is meaningless. This is the unspent lynchpin #1.
2. ⭐**Atomic (R_S, N_S)** — Q-T4-2, one snapshot, rejected if unpaired.
3. ⭐**The indexed nullifier tree must equal the spent set** — N_S must commit to EXACTLY the nullifiers `is_spent` knows (same set, no divergence), rebuilt deterministically from the log. Non-membership is only as sound as the tree's completeness: a single missing nullifier = a spent note provable unspent. Lynchpin #2. Pin a test that the tree's membership == the HashSet across a log replay.
4. ⭐**No apply path.** The proof type is not a BundleRecord and can never be decoded/applied as a spend — type-enforced AND mutation-tested the way #31's apply-path-guards test drives a foreign object through every mutating entrypoint → rejected. A survived mutation here is a spend forged from a read proof.
5. **Threshold range-sizing (item-344 lesson).** Σv_i can reach k·2^64; a difference range-checked to 64 bits is wrong both ways — a latent false-refusal for large legitimate sums and a potential wrap. Size the comparison to the sum's true range (or bound total pool value < 2^64 and prove it). Range-check the SUM, not just each v_i and the difference.
6. ⭐**k vs forced consolidation = a privacy RE-introduction.** If a holder must consolidate notes to fit them into k slots before proving, that consolidation is a spend that reveals those nf AND is a human step — the exact failure we're removing. Size k so a holder proving a realistic X never needs to pre-consolidate; never make consolidation a precondition of proving.
7. **Freshness binding.** "Unspent as of S" is only a current signal if S is recent: bind the proof's S to the challenge's L and keep the node's check_window, else a now-spent note proves unspent against an old S.
8. Distinct proving-key domain; the #31 hash-domain inventory gains the challenge-hash domain (and this time it lands — I'll diff it at the ref).
9. Mutation row on EACH clause (a)–(g) + pins 1–7; a survived mutation on any clause is an unsound proof.

D3/D4/D5/D6 all ruled sound: dropping value-commitments/binding-sig/rk is correct for a read-only statement; the new Halo2 circuit as a non-Action non-record type is the right isolation; reusing the #67/#75 scaffolding with the proof swapped and removing the revealing proof from the product path is the right migration. Implementation is post-event, per-slice, each with its own firsthand IMPL-PASS under the ship rule. Good work — this is the correct feature.
```
