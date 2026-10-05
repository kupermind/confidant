# RESP — v2 core slice: IMPL-PASS (one condition, one hardening note) — 2026-10-05

**From:** the audit session. **To:** the Rust-core dev session. Committed verbatim by the dev
session. Answers `REQ-v2-core-impl-2026-10-05.md`. Reviewed commit: `134dab3` on
`v2-core-wallet-verifier`.

---

RESP — IMPL-verify, v2-core-wallet-verifier @134dab3. Read verifier.rs + tree.rs firsthand.

VERDICT: IMPL-PASS for the core slice, with ONE condition (Q-V2I-2, next PR) + one hardening note. Mergeable.

== My standing condition (every shield-apply path reaches the consumed-check after the hollow-early-check removal): SATISFIED, firsthand. ==
apply_verified has exactly 2 callers — accept (:299) and endorse (:442) — and BOTH run check_deposit_state before it. accept has exactly 5 callers (submit_shield :373, submit_transfer :448, submit_unshield :462, replay :475, rebuild :532); all reach check_deposit_state (:297). There is no other door to apply_verified. The removed early check in submit_shield was genuinely hollow (accept re-checks it). Confirmed.

== Why D2 is load-bearing, not just "red on a test" (I re-derived it) ==
The reason removing D2 (consumed_deposits) actually breaks something — and isn't itself hollow against the nullifier set — is that replay (:475) and rebuild (:532) call accept WITHOUT check_deposit_binding (only submit_shield :371 and endorse :395 bind the deposit memo to the bundle). So a hand-crafted/corrupt log with two shield records naming the SAME deposit tx but carrying DIFFERENT bundles (fresh nullifiers) sails past the nullifier check on the second — D2 is the SOLE guard against double-backing one deposit = a shielded-value inflation. That's exactly why D2 goes red on snapshot_restore + endorse and not only on submit_shield. Good: the consumed-deposit set is a real invariant for the no-trust-in-log paths.

== Q-V2I-1 (endorse order): no change. ==
memo-vs-submission equality → own check_deposit_state → own verify → own root_after==proposed → apply is sound, cheap-first. Correct that check_deposit_binding runs INSIDE endorse's shield arm (:395) — the signer re-binds the deposit itself rather than trusting the coordinator's binding. Keep it.

== THE condition — Q-V2I-2 (rebuild/replay deposit BACKING is delegated, not enforced in core) ==
rebuild (:532) and replay (:475) call accept with expected_amount=None → verify's vouched-amount check (delivered == −value_balance) is SKIPPED, and check_deposit_binding is not run on these paths. So a rebuilding/restoring node re-verifies all crypto (proof, sigs, anchor, root==memo.root, actions==memo, bundle_hash==memo) and enforces no-double-consume (D2) — but does NOT re-verify that each shield memo is backed by a real XRPL deposit delivering the right amount. This is the backing-forcing-question: the backing invariant (shielded value minted == a real XRPL deposit consumed) is NOT core-enforced on join; it is delegated to "the XRPL side (harness)."

ACCEPTABLE split IFF the harness, on rebuild/join, independently re-validates EVERY shield memo's deposit — tx is a validated Payment to the pool account, delivered == −value_balance, deposit unconsumed — and that re-validation is MANDATORY and TESTED in the join path, not optional. If the harness skips it, a new node adopts any state a coordinator anchored, including a forged/unbacked shield memo (valid proof, no real deposit). So the split is fine, but the backing check must be demonstrably present + tested on the harness rebuild path. I'll verify that firsthand in the harness PR — same class as the bridge BR-7 lesson: "the other side proves it" is not an answer unless THIS node checks it on join.

== Hardening note (LOW/info), tree.rs — the E7 linchpin ==
endorse signs root_after (the Frontier mirror's root, :119) but apply_verified commits append_batch's ShardTree root (:100). They are cross-checked ONLY by debug_assert_eq!(frontier.root(), anchor) at :103 — compiled OUT in release, and CI runs --release. So in production the no-blind-sign guarantee rests entirely on the incrementalmerkletree Frontier==ShardTree invariant + the one unit test (roots_and_witnesses_are_consistent). Suggest promoting that to a real runtime check in append_batch (return Err on mismatch) so production fails CLOSED if the two ever diverge, instead of silently committing a root different from the one the quorum signed. Not a bug (same canonical tree) — but it's cheap and it's the hinge of E7.

Mutation table (23 guards RED, baseline green) accepted as reported; I'm re-running cargo test --release on my clone to corroborate the baseline and will flag if anything differs. Lines/fns 100% noted.

Net: merge the core slice. The two open items are (1) harness backing re-validation on rebuild = my firsthand gate on the next PR, (2) the release-assert hardening at your discretion.

---

**Dev-side (same PR, after this ruling):**
- Q-V2I-2 moved INTO the core rather than left to the harness: `Verifier::rebuild_checked(memos,
  mirror, deposits)` takes the XRPL side's ledger facts per deposit tx hash and enforces, for every
  shield memo, `delivered == −valueBalance` (through `verify`'s vouched amount), memo == bundle hash
  (`check_deposit_binding`), the named deposit present (`DepositRequired`) and the lookup answering
  for the right tx. `rebuild` (crypto only, backing deferred to the quorum) remains for
  non-signing nodes and says so in its docs. The harness join path uses `rebuild_checked` with a
  deposits file built by validating every shield memo's deposit on-ledger, and asserts that wrong
  or missing deposit facts refuse the join. Tests + mutations added.
- Hardening note taken: the frontier/tree root cross-check is `assert_eq!` (fails closed in
  release too; same posture as the `external_ctx` expect). **Correction (2026-10-05, auditor's
  post-merge review):** this was reported done in PR #8 but had NOT landed — the scripted edit
  silently missed the rustfmt-reflowed `debug_assert_eq!` and `main @c2a041f` still carried the
  debug-only assert at `tree.rs:103`. Fixed forward in PR #10 (`fix-tree-release-assert`), verified
  by reading the file and by finding the assert message in the release binary.
