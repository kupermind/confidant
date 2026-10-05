# Audit cycle index

Cycle: `REQ-<slug>-<date>.md` from the dev session → `RESP-<slug>-<date>.md` from the audit session
(committed verbatim by the dev session; the audit session has no write access). Verdicts:
DESIGN-PASS / IMPL-PASS / CONDITIONAL / FAIL. No core module merges to `main` without its IMPL-PASS.

| Unit | REQ | RESP | Verdict |
|---|---|---|---|
| Pool design (Orchard circuit reuse, tail, sighash, auditor key) | `REQ-pool-design-2026-10-04.md` | `RESP-pool-design-2026-10-04.md` | DESIGN-PASS, conditional C1–C5 (all applied) |
| `confidant-core` M0 implementation | `REQ-pool-core-impl-2026-10-04.md` | `RESP-pool-core-impl-2026-10-04.md` | IMPL-PASS @ `aba71fe` |
| Verify-tail reorder (nullifiers before proof) | `REQ-pool-core-impl-2026-10-04.md` §Supplement | `RESP-pool-core-impl-2026-10-04.md` §RESP (slice) | PASS @ `a728aed`, caveat: revisit if the bundle log ever becomes private |
| Full line coverage + verify step-2 tightening (`VouchedAmountNotApplicable`) | `REQ-pool-core-impl-2026-10-04.md` §Supplement 2 | `RESP-pool-core-impl-2026-10-04.md` §RESP (slice, PR #3) | PASS @ `97eab23` (stands as corrected: `debug_assert!` removed before merge, auditor acknowledged); note: every caller of `external_ctx` owes the sign check |
| v2 trust-minimized architecture (Wallet/Verifier, deposit binding, anchor memos, k-of-N) | `REQ-v2-trust-minimized-2026-10-05.md` | `RESP-v2-trust-minimized-2026-10-05.md` | DESIGN-PASS, conditional: signer validation (`endorse`) in core, version byte hard-reject, label refinements |
