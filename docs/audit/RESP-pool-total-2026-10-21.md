# RESP — pool total in `GET /fees` (`outstandingBacking`), PR #37

From the audit session, verbatim, 2026-10-22. The owner's REQ (`REQ-pool-total-2026-10-21.md`) had not
reached the audit session; the dev session relayed it together with a review finding (a lock-order
inversion in `Node::fees()`), fixed in PR #40 (into `pool-total`) at `58c4c05`.

## 1. Ruling on the relayed REQ: trust-path DESIGN-PASS as a read-only report; the deadlock finding confirmed

```text
On PR #37 "Pool total in the UI" (branch pool-total):

(1) The REQ did NOT reach me this session — no REQ-pool-total-2026-10-21 landed here (same commit-pending situation as before: your permission classifier can't write my RESP into docs/audit, so the owner pastes it; correctly not routed to me). So I'm treating your message as the REQ relay. RULING: **trust-path DESIGN-PASS as a read-only report.** Reasoning:
- No trust-path change: nothing signs, applies, or refuses differently; no new ingress (the field rides GET /fees); coordinator passes it through. Same class as GET /config / GET /fees (read-only/public-only) I already passed.
- The one auditor question on a privacy product — does the new field leak anything PRIVATE — checks out NO. `outstanding_backing` = −Σ value_balance over the verifier's accepted bundles = the net total shielded in (shields minus unshields). Every shield and unshield is a public Payment to/from the pool account, and the fee rate is a public genesis constant, so an observer can already derive this aggregate from the ledger. Crucially it is an AGGREGATE — it does NOT expose the private per-holder split, which stays in the notes. No privacy regression. (This is also the exact figure the Q-FEE-3 sweep belt computes as the conservation floor, from sealed verifier records in i128 — faithful internal state, honest to surface.)
- Test shape is right: asserting 59_750_000 + the camelCase wire name, and M1 (field filled with 0) is the correct hollow-test that the value is really computed not stubbed; M2 (rename dropped) guards the wire name. Good.

ONE LABELING NOTE (minor, non-blocking): "pool total (public)" is the user NOTE-BACKING total, which is NOT the pool account's raw XRPL balance — the raw balance is higher by the slack I flagged in Q-FEE-3 (operator funding + accrued-unswept fees + refused-deposit-awaiting-refund). That's arguably the RIGHT number to show as "pool total" (user backing, not operator float), but make sure the UI copy / tooltip doesn't imply it's the account's whole balance. "total shielded / backed" would be unambiguous.

(2) Your deadlock finding is CORRECT and I agree with "merge only with the fix." It's a textbook AB-BA lock-order inversion: the established order (submit_*/endorse) is VERIFIER (`self.lock()`) → FEES (`lock_fees()` via accrue); `fees()` as written takes FEES first (`let l = self.lock_fees()`) then VERIFIER (via `outstanding_backing()`'s `self.lock()`) = the reverse, so a GET /fees racing a submit/endorse can deadlock the node. Liveness, not safety (two std::sync::Mutex, no await held across — confirmed, no .await inside either critical section). Your fix is right and I can say WHY it works: `outstanding_backing()` acquires AND RELEASES the verifier lock within the call, so computing `let backing = self.outstanding_backing();` BEFORE `let l = self.lock_fees();` means `fees()` never holds both locks at once — it breaks the cycle rather than just reordering it, and it matches the ordering `pool_excess()` already uses. Reuse the local, don't re-call.

ONE ADDITION I'd want for the ruling (durable, so this class doesn't recur): now that 3+ sites take both mutexes, DOCUMENT the canonical order as an invariant — a one-line comment at each mutex field / lock site: "always acquire self.lock() (verifier) before self.lock_fees()." A lock-order bug is exactly the kind that reappears silently when a fourth site is added; the fix closes this instance, the documented invariant closes the class. (Optional, heavier: a tiny `with_locks()` helper that acquires in the fixed order so a caller can't invert it — but the comment + this fix is right-sized for now.)

Net: trust-path PASS (read-only, no privacy regression); merge WITH your deadlock fix + a documented lock-order invariant; tighten the UI label. I'll do a firsthand IMPL-PASS of the fixed commit at the ref when it lands (confirm the fix is in and the field is computed from sealed state, not host-reported) — ping me with the commit. Nothing here is owner-gated or a trust-path change, so no custody surface to escalate.
```

## 2. Firsthand IMPL-PASS at `58c4c053f0c7ce7a48862bfbf0ae1392e930cc62` (PR #40)

```text
Firsthand IMPL-PASS at 58c4c053f0c7ce7a48862bfbf0ae1392e930cc62 (SSH clone of pool-total-lockorder, HEAD matched). The deadlock fix is correct AND complete — merge on local-green.

VERIFIED at the ref (backend/node/src/node.rs):
- fees() (157-178): line 158 pool_excess() (acquires+releases verifier internally), 163 `let backing = self.outstanding_backing();` (verifier lock taken AND released inside the call → returns u64), 164 `let l = self.lock_fees();`, and the report uses the local (`outstanding_backing: backing`, 169). So fees() holds AT MOST ONE mutex at any instant — it doesn't reorder the hold, it eliminates the simultaneous hold. Correct.
- COMPLETENESS (I enumerated every lock site, not just the reported one — a lock-order fix is only real if NO site inverts): the sole places the fee-ledger lock is taken are lock_fees()@116 (accrue, called while the verifier guard v is held in submit_*/endorse → V→F, canonical), fees()@164 (verifier already released → F alone), and the sweep path@201 (outstanding_backing released @193 before lock_fees @201 → F alone). Every both-locks-held site is submit_*/endorse = V→F (333→336, 353→357, 410→427 via accrue). NO site anywhere takes fee-then-verifier (F→V). So the inversion class is closed entirely, not just the fees() instance.
- Invariant documented at BOTH helpers (lock_fees 96-98, lock 231-232) AND the code matches the doc — I confirmed by the enumeration above, so it's a real invariant, not a written warning only. Good wording ("never call anything that takes lock() while holding this guard" is exactly what keeps fees()/sweep safe).
- outstanding_backing() (135-144) UNCHANGED and still sealed-state, not host: `-Σ (value_balance as i128)` over `self.lock().records()`, `u64::try_from(sum.max(0)).unwrap_or(u64::MAX)` — i128 sum, clamped ≥0, from the verifier's accepted bundles. So the field surfaces the same conservation figure the sweep belt uses; no new trust input.

Trust-path: still PASS (read-only field, no sign/apply/refuse change, sealed-state aggregate, no per-holder split exposed) — unchanged from my earlier ruling; this commit only moves a lock and relabels UI.

Label: I take "Total shielded — N XRP (backed, public)" + the note-backing tooltip as you describe — it's frontend copy, outside the trust path, so I did not line-read it; the wording matches my note (it's the note-backing total, not the account's whole balance). Non-blocking either way.

Gate: per the owner's Idea-1 (local-CI while Actions billing is down — he told me directly), local-green is the gate this window, so your cargo fmt + clippy -D warnings + node 10/10 (release) are what I rely on; GitHub CI ignored as the lead said. (FYI from my side, unrelated to your PR: the owner disabled Actions on the 77ph enclave repos to stop accumulation — that's a 77ph-account action; Confidant is under kupermind, separate.)

Verdict: IMPL-PASS; merge PR#40 on local-green, and the owner folds it into pool-total with his one click. Nothing else owed from me on this. Ping me if a core/trust-path type moves.
```
