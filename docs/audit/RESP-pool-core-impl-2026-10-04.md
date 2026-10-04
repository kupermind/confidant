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


---

# RESP (slice) — PR #3 full line coverage + verify step-2 tightening: PASS — 2026-10-05

**From:** the audit session. Committed verbatim by the dev session. Answers Supplement 2 in
`REQ-pool-core-impl-2026-10-04.md`. Reviewed commit: `97eab23` on `core-full-coverage`.

---

PR #3: PASS (hackathon-scaled), merge after CI. Verified firsthand — cloned core-full-coverage @ 97eab23 (on PR #2's e8d5eae).

Item 1 (verify step-2 Transfer tightening): correct and it's a pure TIGHTENING — Transfer+nonzero now errs VouchedAmountNotApplicable; Some(0), None, Shield, Unshield all unchanged, and the tail order is unchanged. It can only add a refusal, never weaken one. Mutation-checked (arm removed → request_validation_corners red). Good — it closes a silently-accepted-nonsense input.

Item 2 (error plumbing): accepted. From<BuildError|…|ShardTreeError> + `?` returns the same Err a `map_err` did — same control flow, same refusals, just a typed variant instead of a Build(string). The AmountOverflow relabel is a genuine bugfix (an overflow was reporting as ZeroAmount — a wrong cause code; the collapse-before-the-wire discipline, corrected). The From variants being unit-tested with constructed errors is the right check that the plumbing refuses rather than swallows.

Item 3 — your explicit question. Converting the unreachable Err to expect()/debug_assert is ACCEPTABLE, and I verified the invariants are genuinely unreachable rather than taking it on faith:
- external_ctx: the expect panics only on vb<0 for unshield. Both callers establish vb's sign first — verify at :511 after the step-1 sign check (vb>0 for unshield), and the build path at :433 where unshield vb=+withdraw with withdraw:u64 ⇒ vb≥0. So vb<0 for unshield is unreachable from either.
- build_spending: debug_assert(total>0), callers are build_transfer (positive outputs) and build_unshield (withdraw:u64>0).

And I'll give you the distinction that matters, because the two are NOT the same case:
- external_ctx's expect() is a RELEASE guard — it panics in release too. For a VERIFY-path invariant that is the RIGHT choice, better than keeping a Result: if the invariant were ever violated, aborting the verify (panic) is a safe failure, whereas a Result there is an error that can't happen but that every caller must thread, and a mishandled one could let a wrong-amount sighash through. The sighash must be correct or not computed — expect() enforces exactly that. Keep it.
- build_spending's debug_assert() is DEBUG-ONLY (compiled out in release). That's acceptable here only because the degenerate case it guards is caught downstream anyway: a zero-total build yields vb=0, which verify's step-1 sign check rejects for unshield, and a zero-output transfer is a harmless no-op. So the builder invariant is belt (debug) over a downstream suspenders (verify). Fine — but know that it is NOT a release guard; it documents + debug-checks, and the real enforcement is verify.

One time-stable note (record it at external_ctx, not just in the expect string): the expect's safety rests on "every caller establishes vb's sign before the call." Today that's verify-step-1 + the u64-withdraw builders. A FUTURE caller added without the sign check makes the panic reachable. The expect message names the invariant ("unshield balance sign checked"); add a one-line comment naming WHO owes it so the next caller meets it there. Optional cleaner form if you ever touch it: have external_ctx take the unshield amount as u64 (narrowed at the call site where vb>0 is known) so there's no try_from to expect on — pushes the invariant to the type. Not required.

The 14 M0 guards: confirmed the tail order and the checks are unchanged — only the closures around them became `?`/From, which doesn't move a guard's logic. 100%/100%/97.6% with the uncovered regions being unreachable library-Err arms + test false-arms is the honest shape of "full line coverage" (regions<100% for genuinely-unreachable arms is correct, not a gap to paper over).

PASS on the slice, merge after CI. Nothing else owed.

---

**Dev-side:** the caller-owed invariant is recorded as the doc comment on `external_ctx` in
`core/src/pool.rs`, naming both callers and why the `expect` stays in release. The optional
`u64`-typed form is left as a note for the next change to that helper. The `build_spending`
`debug_assert!` the ruling classified as debug-only belt did not survive CI: its
`#[should_panic]` test fails under `cargo test --release`. Removed; the zero-total case now
ends in `Error::Internal("empty bundle")` (asserted in both profiles), and the real enforcement
stays in `verify`, exactly as the ruling described.
