# RESP — proof of shielded funds (hackathon feature C): the core slice

From the audit session, 2026-10-24 (event day), verbatim; firsthand review of branch `pof-core` at head
`c74ba33` (PR #61), then of the fix at `e751480` (PR #63, merged into the branch). Ruling: **IMPL-PASS,
unconditional**, for the core slice under DESIGN-PASS conditions 2–4 (`RESP-pof-design-2026-10-14.md`;
condition 1 landed in PR #31). The one gap found by the dev session's code map — `rebuild_inner` decoded
the mirror's record outside `pool_bundle`, so a proof-of-funds record under a genuine-looking pool memo
was refused by signature domain rather than by type — was made a merge gate, fixed (one line + two test
rows) and re-verified. X3 (amount dropped from `pof_ctx`) is recorded as an equivalent mutant with the
auditor's reasoning; an optional post-event row would pin that belt.

## 1. Conditional ruling on `c74ba33`

```text
RESP — proof-of-funds core slice (PR #61, head c74ba33), firsthand. Reviewed at the ref.

VERDICT: IMPL-PASS for the core slice under conditions 2–4, CONDITIONAL on the one fix below. Merge once the rebuild_inner fix + its test row land and your independent re-run confirms; I'll re-verify the one-liner. The slice is otherwise clean and I'm passing conditions 2–4 on my firsthand code map plus your 12/12 mutation table and 100% coverage.

Condition 2 completeness (the owner's 3rd ask — confirmed firsthand, and this is where the gap is): `bundle_from_record` is called at EXACTLY three sites — pool_bundle (:306, after the op refusal at :303–304), verify_proof_of_funds (:323, the PoF checker, correct to decode a PoF there), and rebuild_inner (:648, direct). The five pool apply paths (submit_shield/endorse/submit_transfer/submit_unshield/replay) all route through pool_bundle, which refuses op==ProofOfFunds by TYPE before decode; verify() (:236) refuses by type before the sign check; the memo path refuses op byte 4 (your M3/M4 pin it). So five of six apply paths refuse by type. rebuild_inner is the sixth and it's the gap you found: it decodes the mirror's record directly, so a PoF RECORD placed in the mirror under a genuine pool memo (op 1–3) is refused only by SIGNATURE DOMAIN, not by TYPE — fail-closed, but exactly the thing condition 2 said to avoid on every path.

So I'm making your should-fix a merge GATE, and here's why it's a block and not a follow-up despite being fail-closed: condition 2's whole point is defense-in-depth — the type separation must NOT rest solely on the signature domain holding; one path (rebuild_inner) resting on it is precisely the hole the condition exists to prevent, and this slice IS the condition-2 slice. The fix is one line (route :648 through `v.pool_bundle(&record)`) plus the test row you named (PoF record in the mirror under a pool memo → NotAPoolOperation), you already have it, and it makes the stated invariant true everywhere rather than true-on-5-of-6. Land it and re-verify to me; that's minutes, not a delay.

Owner's 2nd ask — amount == value balance exactly: your read is correct and I confirm it. The bundle's vb is the X the holder chose with change to self, so "≥ X" is the statement about the balance (vb = X with change-to-self ⇒ the holder controlled ≥ X), and the verifier binds X by computing pof_sighash from the amount IT was given, not from the record — a record claiming a different X fails the signature; the record's own vb is bound by the binding signature (bvk = Σcv − ValueCommit(vb)), so a lied vb fails there. Exact-equality is right. One pin, from your own observation: add a vb == 0 == amount test so the amount>0 arm is independently pinned — a vb=0 proof is a vacuous "controls ≥ 0" and harmless (and the node slice refuses X=0 from the challenge), but condition 4 is every error arm, so pin it. You're already running that.

Conditions 3/4 — break-tests both directions and 12/12 mutations (M6/M7 the domain/ctx, M8–M12 the verify checks, M1–M5 the type refusals) plus 100% core coverage: met on your table and my code map. I'll finalize when your independent re-run lands (fmt, clippy -D, release tests, 100% llvm-cov, and the mutation re-run with the amount==0 and distinct-nullifier extras). Send those + the rebuild_inner fix and it's a clean PASS.

Honest label — correct and matches the code: "underwriting signal, snapshot, not a lock, not proof that a named account owns the notes." Hold it exactly on the lender surface when Tier 1 adds one. The challenge-binding (borrower account / ledger index / expiry / lender id) is correctly Tier 1's job, not the core — and if Tier 1 ships tonight, I gate it on the six pins, with pin 5 (unspent re-checked AT SIGNING time — the TOCTOU window between /pof/verify and the signature) and pin 3 (the node recomputes the URI, never echoes the coordinator's) weighted hardest.

Rebase the INDEX.md conflict as housekeeping (not my concern). So: fix rebuild_inner, send the re-run, and this is an unconditional PASS. Good slice — the domain separation, the exact-X binding, and the type refusals are right; the one gap is the one you'd already spotted.
```

## 2. Final ruling on `e751480` (PR #63 merged into the branch)

```text
RESP (finalize) — proof-of-funds core slice, PR #61 + the #63 fix (head e751480). Re-verified the one-liner firsthand.

GATE CLEARED → UNCONDITIONAL IMPL-PASS for the core slice under conditions 2–4.

What I re-verified at e751480:
- rebuild_inner :650 is now `let bundle = v.pool_bundle(&record)?;`, and `bundle_from_record` in verifier.rs now appears at ONLY two sites — :306 inside pool_bundle (after the op==ProofOfFunds → NotAPoolOperation at :304) and :323 inside verify_proof_of_funds (the PoF checker, which correctly decodes a PoF). The direct decode in the rebuild path is gone. So condition 2 — "refused by TYPE before any signature, on EVERY apply path" — is now true everywhere: all six apply paths (the five via pool_bundle plus rebuild) refuse by type, verify() refuses at :237 before the sign check, and the memo path refuses op 4 (M3/M4). No path is left resting on the signature domain.
- The test rows pin it: row (1) rebuild AND rebuild_checked with a PoF record served under a genuine pool memo → NotAPoolOperation (and the rebuild_checked lookup is written to panic if consulted, proving only shield memos reach it — a clean demonstration that the refusal is upstream of any mirror fetch); row (2) a vb=0 bundle relabelled ProofOfFunds → ValueBalance{expected:0,..} before any signature. Reverting the fix → killed by row (1); X1 (amount==0 guard) → now killed by row (2). Your re-run (100% lines 1302/1302, 100% functions, all core tests green in release incl proof_of_funds 2/2, clippy clean) is accepted.

X3 (your surviving mutant — pof_ctx drops the AMOUNT, keeps the challenge): I agree it is an EQUIVALENT MUTANT, and I verified the equivalence rather than take it. The attested amount is pinned by two bindings independent of the sighash: the record's value_balance is bound by the binding signature (bvk = Σcv − ValueCommit(vb)), and verify enforces vb == amount, so the attested amount == vb == the real value balance. Dropping X from pof_ctx therefore cannot produce a wrong-amount acceptance — a bundle presented with a different amount fails at ValueBalance before any signature, and a proof can't be reused for X′ because vb ≠ X′ fails the same check. So X in the sighash is a redundant (belt) binding; removing it is behaviorally equivalent. Record it as an equivalent mutant with that reasoning. ONE optional follow-up, post-event not weekend: an adversarial row that constructs vb=X signed over pof_ctx(challenge, X′≠X) and verifies with amount X, asserting InvalidSpendAuth — that PINS the belt (the X-in-sighash binding) so a future refactor can't silently remove BOTH the sighash-X and lean on an unchanged vb-check. Worth a row later; not a gap and not blocking.

Honest label still correct and matches the code. So: merge #63 into pof-core, approve #61, commit this RESP verbatim; rebase the INDEX.md conflict as housekeeping. C core is done.

Next: send me A's live credential-check.mjs result (fresh-account shield → 403 naming the credential, issue + accept, same record+deposit → anchored) — the last firsthand leg for A. And if A landed tonight, Tier 1 (credential-pof) comes to me against the six pins, with pin 5 (unspent re-checked AT SIGNING, the TOCTOU window) and pin 3 (the node recomputes the URI, never echoes the coordinator) weighted hardest. Good work — the one gap was the one you'd already spotted, and the fix makes the invariant true rather than merely fail-closed.
```

## 3. The dev session's independent mutation table (for the record)

Run one at a time in a separate worktree, release, each file restored from memory and verified
byte-identical. On `c74ba33`: the owner's M1–M12 all killed (M8 scoped to `verify_proof_of_funds`, the
same line exists in `verify()`); X1 `amount == 0` guard removed → survived, killed on `e751480` by the
value-balance-zero row; X2 distinct-nullifier check removed (compiling variant) → killed by the
duplicated-action row; X3 amount dropped from `pof_ctx` → survived, ruled an equivalent mutant above;
R1 the `rebuild_inner` fix reverted → killed by the forged-memo row.
