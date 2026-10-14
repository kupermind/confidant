# RESP — apply-path signature safety + hash-domain guard (PR #31)

From the audit session, 2026-10-14, verbatim. PASS, firsthand, merge after green CI. Two notes
logged as their own items (ROADMAP §4.3b): the authority over `set_anchored_root` — resolved
firsthand in the follow-up below (a watch item, no slice) — and the guard's scope (`core/src` only,
extend if another crate ever computes a bundle sighash).

## 1. PASS

```text
PR #31 (apply-path signature-safety + hash-domain allow-list): PASS — firsthand, merge after green CI. I read verifier.rs @527b5f1 and apply_path_guards.rs directly, not your summary; the proof holds and is non-hollow.

What I verified firsthand:
- APPLY-PATH INVARIANT (my condition Q1-ii): apply_verified is the ONLY site that inserts a nullifier (verifier.rs:354) or appends a leaf (:350), and it has exactly two callers — accept (self.verify :318 → apply_verified :319) and endorse (self.verify :452 → apply_verified :462). Every public mutating bundle entrypoint (submit_shield/transfer/unshield, endorse, replay, rebuild, rebuild_checked, restore) funnels through accept or endorse. So there is NO path to consume state without the pool-sighash spend-auth (:283) + binding (:288) re-check. Your call-graph map is accurate and complete.
- COMPLETENESS of the test (the thing I most needed to check, since a guard that covers a SUBSET certifies a subset): run_every_apply_path drives the foreign record through all ten entrypoints — submit_shield, endorse×{Shield,Transfer,Unshield}, submit_transfer, submit_unshield, replay, restore, rebuild, rebuild_checked — which is exactly the mutating bundle-apply set. Nothing in that set is missing.
- NON-HOLLOW: the foreign bundle is a genuine Orchard bundle (real witnesses from v.witness, built to the asserted value_balance, create_proof with the pool pk) signed only over ForeignDomain_01 — so it passes proof+anchor+value and the refusal is necessarily AT the sig check; the fingerprint (state()+snapshot() JSON) asserts byte-identical state after every refusal; the honest records are accepted at the end (positive control); the swapped-binding variant yields InvalidBindingSignature (binding checked separately). The shield-like shape carries a matching DepositRef so the deposit precondition passes and the SIG check is what refuses, not check_deposit_state — good, that was the subtle one. Your G1–G5 mutations are the right hollow-test defense and the test is structurally shaped to kill each (G1→refusal assert, G3/G5→the enumeration asserts); I did not re-run them but confirmed the test would catch them.
- ENUMERATION (Q1-i): globs core/src, asserts every blake2b has a .personal, allow-list exact + one-site-each + 16-pad-distinct + ctx-set fixed. Time-stable: adding "ConfidantPoF_v1_"/pof_ctx on-site is a deliberate allow-list edit, with the test naming the breaking event — exactly the enforced-not-hoped property I asked for.

TWO NOTES TO ADD (neither blocks this merge — the step-1 claim stands as-is):
1. set_anchored_root (verifier.rs:198) is a &mut entrypoint NOT in the test. That is CORRECT for this invariant — it does not reach apply_verified, so "no apply_verified without verify" is untouched by it. But it mutates committed state (the anchored root), so it is a SEPARATE authority question the apply-path test does not cover: who may call set_anchored_root, is it gated, and can a chosen anchored root enable anything (anchor-history / forged-anchor-known). Log it as its own item; not part of step 1.
2. The enumeration globs core/src/**. That is the right scope today (the pool's sighash/sig logic lives in core). If any OTHER crate (node/sidecar/cli) ever computes a bundle sighash, extend the glob there — note it so the guard's scope tracks the code.

The honest limit (no positive oracle — you cannot build a bundle that passes the pool sig-check without the pool sighash, since that IS the forgery) is correctly characterized, and it is adequately compensated by the call-graph completeness I verified firsthand: refusal + unchanged-state at every entrypoint + apply_verified reachable only behind verify is a sound proof of the invariant without a positive oracle. Merge on green CI. This is the one piece that could not be deferred, and it is landed correctly.
```

## 2. Follow-up: note (i) resolved firsthand

```text
Good — and note (i) is now firsthand-RESOLVED on my side, not just reported, so you can downgrade the "small slice if yes" to a watch item. I read verifier.rs @527b5f1: set_anchored_root(:206) refuses any root that is not already is_known_anchor (the same gate verify uses at :258), so it cannot inject a forged or new anchor — only a root the tree already had; and anchored_root is inert — its only uses are the setter, serialization into state()/snapshot(), and the restore() round-trip; verify and every apply path key off self.tree.is_known_anchor, never off anchored_root. So "can a chosen older known root enable anything" = NO today: it is a reported/display field that nothing verifies against. The only thing to carry (time-stable framing): if anchored_root ever becomes an input to a security decision, reopen the older-known-root question then — otherwise no slice needed. Note (ii) (glob scope follows any crate that computes a bundle sighash) is correctly captured in the test's doc comment. Nothing further from me pre-event; I'll take the A then C slices in that order when they come.
```
