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
| v2 core slice (Wallet/Verifier, deposit binding, memo codec, rebuild, endorse) | `REQ-v2-core-impl-2026-10-05.md` | `RESP-v2-core-impl-2026-10-05.md` | IMPL-PASS @ `134dab3`; Q-V2I-2 met in-core via `rebuild_checked` (confirmed firsthand post-merge); release assert reported in PR #8 had not landed — fixed in PR #10, confirmed firsthand @ `cd1b404` |
| Quorum services (`backend/`: verifier node, signer sidecars, coordinator, scanner) | `REQ-quorum-services-2026-10-05.md` | `RESP-quorum-services-2026-10-05.md` | DESIGN-PASS, conditional C1–C5 (C1 sidecar Fee/Sequence/LLS, C2 deposit re-lookup, C3 idempotency, C4 startup custody + scan completeness + mirror, C5 payout recovery); **IMPL-PASS @ `2eba713`** (all five conditions verified firsthand in code; Q-SVC-I1 keep `current`, I2 windows sane, I3 watch autofill LLS offset — pinned explicitly after the ruling; replacement-coordinator recovery path documented); PR #12 deploy manifests: security-delta pass clean, INFO on viewing-key handling → documented in `docs/API-BOUNDARY.md` |
| Resilient xrpl.js client (`backend/quorum/lib.mjs` `resilientClient`: reconnect, retry, interrupted submit resolved by hash) | FYI + retry-trap confirmations (messages, 2026-10-06) | `RESP-quorum-services-2026-10-05.md` §resilientClient pass | IMPL-PASS @ `f38fe58`, off the trust path; PR #16 reconnect fix (`ensure()`: disconnect before connect) confirmed clean firsthand @ `c6108d4` |
