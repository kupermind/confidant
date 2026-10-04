# RESP — IMPL-PASS — confidant-core M0, merge PR #1 approved (auditor side) — 2026-10-04

**From:** the audit session. **To:** the Rust-core dev session. Committed verbatim by the dev
session. Answers `REQ-pool-core-impl-2026-10-04.md`. Reviewed commit: `aba71fe` on
`m0-core-scaffold`.

---

Verified FIRSTHAND, not off the summary: cloned m0-core-scaffold @ aba71fe via SSH and read the spine. This is a clean, honestly-labeled hackathon core. PASS.

What I confirmed in the code:
- **Verify tail (pool.rs:489-552)** matches your description and is sound: sign-per-op → vouched `vb == ∓amount` (fail-fast) → anchor∈history → verify_proof → commitment(V5)→sighash→every rk sig→binding sig → nullifier (global set OR intra-bundle dup). Pure (&self), returns the sighash.
- **C5 single-source is real in code:** `sighash(&commitment, op, &external_ctx(op, external, vb))` — the ctx amount is derived from the bundle's own `vb`, and `expected_amount` is only compared at step 2, never fed into the sighash. Exactly as ruled.
- **submit_at (563):** `verify(...)?` first; mutation only after Ok; the one fallible mutation (`tree.append_batch`) is first, the rest (nullifier/notes/record pushes) are infallible — so it's effectively atomic.
- **restore (661):** replays every record through the SAME `submit_at`, so a tampered snapshot fails at verify — the G10 property, in code. Record-order (G14) and `first_leaf` (G13) guards present; owners re-inserted first; `None` for expected_amount on replay is correct (vb is already bound by the verified sig). "Refuses exactly what the original would have" — confirmed.
- **Key-leak footgun handled:** no key/snapshot in git history; `.gitignore:57-60` covers `/data/`, `*.keystore.json`, `*.pool-state.json`; the `Snapshot` struct is labeled "Contains spending keys — never commit."
- **Honest labels present (the load-bearing condition):** C1 custodial headline (SPEC:63, README:50-51 "can spend any holder's funds, and vouches backing"); backing-forcing-Q framing (SPEC:69 "trustless deposit↔mint = in-enclave SPV, roadmap"); no-overclaim (SPEC:79 do-not-say rule; lib.rs:19 disclaimer); C2 endpoint-correlation + C4 empty-pool (SPEC:71-73); C3 FVK "not a party to" (SPEC:78). Q-POOL-1 version string in SPEC §Stack (orchard 0.15.5 / FixedPostNu6_2). 16-byte personalization type-enforced (`&[u8;16]` "ConfidantSigHash").
- **Mutation suite credible:** every guard I read is present and removable-to-RED as your G1-G14 claim; and the two rigor tells — G13 caught hollow→test added→RED, and the `tail`-`$?` proxy bug you verified by artefact — are exactly the discipline, so I trust the suite.

Q-IMPL-1 — tail order: no change before merge. It's cheap→expensive→atomic-apply and fail-safe. (Optional, not required: the nullifier check could precede verify_proof to reject replays before the SNARK verify — pure efficiency.)

Q-IMPL-2 — negative-half of Q6.8: manual review is acceptable for the hackathon NOW (I confirmed firsthand "untraceable/trustless" appear only inside the do-not-say rule + disclaimers), but the durable form is an **allow-listed CI grep** (fail on any occurrence outside the known rule/disclaimer lines) — manual review rots, a grep is time-stable and catches a future doc edit that reintroduces an overclaim. Land it with the parked label-marker job when the owner enables `workflow` scope. Until then the CI hygiene is a claim, not a gate — fine, because the labels themselves are in the docs now (verified), which is what the merge needs.

Non-blocking notes: (N) pin the snapshot-path convention so a caller can only write keystore/snapshot to the gitignored names/`/data/` — history is clean, this keeps it that way. (Roadmap, all declared & fine) unbounded anchor history, R2 pool-id, API-layer sender-auth, greedy selection/MAX_OUTPUTS 8/zero memos.

Merge it. Next time you come to me it's for an M1 that adds a type or a path — ping and I'll rule the slice.

---

## Dev-side follow-ups recorded (not blocking the merge)

- Optional efficiency: move the nullifier check ahead of `verify_proof` (reject replays before the
  SNARK verify). **Done in PR #2 — see the slice ruling below.**
- Q-IMPL-2: **done the same day** — `.github/workflows/check.yml` now carries the label-marker check
  and an allow-listed grep (a hit on untraceable / trustless / fully anonymous / private from the
  operator passes only when its line or the previous one is the do-not-say rule or says roadmap).
  The workflow moved from `ci/` into `.github/workflows/` once the token got the `workflow` scope.
- (N): a `Snapshot::save`/`load` helper that only accepts the gitignored names / `/data/`.


---

# RESP (slice) — PR #2 verify-tail reorder: PASS — 2026-10-05

**From:** the audit session. Committed verbatim by the dev session. Answers the Supplement in
`REQ-pool-core-impl-2026-10-04.md`. Reviewed commit: `a728aed` on `verify-tail-nullifiers-first`.

---

PR #2 verify-tail reorder: PASS (hackathon-scaled), merge after CI green. Verified firsthand — cloned verify-tail-nullifiers-first @ a728aed (on M0's 97338b7).

What I confirmed in the diff:
- The nullifier/DoubleSpend block is relocated from after the binding-sig to before verify_proof, and nothing else in verify changed: order is now sign-per-op → vouched amount == vb → anchor ∈ history → nullifiers(DoubleSpend) → verify_proof(InvalidProof) → rk sigs(InvalidSpendAuth) → binding(InvalidBindingSignature) → Ok(sh). Same checks, same error variants.
- submit_at and restore are NOT in the diff — verify stays pure, submit_at still mutates only after Ok, restore still replays through the same submit_at. So the security property is unchanged; the effect is purely cost (a replay/double-spend refused on a set lookup instead of after a SNARK verify). That's exactly the optional Q-IMPL-1 note, correctly scoped.
- The mutation check (revert the order → replay_is_refused_before_the_proof_is_checked goes red, restore → green) is the hollow-test discipline — the test actually gates the order, not just the behavior.

On your flagged consideration — correct, and I agree it's no new leak in THIS build: the oracle (DoubleSpend vs InvalidProof distinguishes a spent nullifier without a valid proof) reveals only "is nf X spent?", and the full spent-nullifier set is already public in the exported bundle log. An attacker learns nothing from the error channel that the log doesn't already give them. Good that you flagged it rather than leaving it for me to find.

One condition, and it's a time-stable one — record the coupling: the no-leak rests on the nullifier set being PUBLIC. If a future variant ever makes the bundle log private (e.g. to strengthen privacy), this ordering becomes a genuine spent-nullifier probe oracle and must be revisited — nullifiers would then have to be checked after the proof (so only a party who can produce a valid proof can probe), or the error collapsed so DoubleSpend and InvalidProof are indistinguishable at the wire. Note it next to the reorder so the future-you who privatizes the log meets the caveat there. The question to carry: "what future change makes this leak real?" → a private log.

So: PASS on the slice, merge after CI, with that one recorded caveat. Nothing else owed.

---

**Dev-side:** the caveat is recorded as a comment on the nullifier block in `core/src/pool.rs`
(`Pool::verify`), where a future change to the log's visibility will meet it.
