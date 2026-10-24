# RESP — credential gate: `Expiration` judged against the ledger's clock (Q-CG-1), second depositor (M8)

From the audit session, 2026-10-24 (event day), verbatim; firsthand review of branch `credential-close-time`
at head `26f7379` (PR #72). Ruling: **IMPL-PASS — Q-CG-1 is closed**, M8 pinned; "valid now" (the latest
validated ledger's close time) confirmed as the right clock. Consequence to keep in view: a credential that
lapses between submit and endorse strands a paid deposit at endorse — the Q-CG-3 refund pin (§4.3a) becomes
more load-bearing; before production, non-blocking on devnet.

```text
════════ #72 — Q-CG-1 (credential gate vs ledger clock) + M8: IMPL-PASS ════════
Verified at 26f7379. screened_deposit's gate block (node.rs:336–348): account from `deposit_source(&tx)` (the ledger's answer), `credential(account,issuer,type)` `?`, `validated` `?`, `now = ledger_close_time(validated)` `?`, `check(account, entry, now)` — Expiration judged against the validated ledger's close_time, keyed on the ledger's depositor. Both new reads fail closed (NodeError::Ledger → 502, never an acceptance), same arm as the lookup. `ripple_now` is GONE from node.rs (the grep is empty) — off the trust path entirely. rpc.rs ledger_close_time reads the `ledger` command's close_time else a ledger error. M8 keyed on deposit_source. MT1–MT5 confirmatory. **Q-CG-1 is CLOSED.**

Your design question (b): **"valid now" (latest validated) is right — confirmed**, and your reasoning is the correct one. The pool vouches at submit AND re-checks at endorse; a credential valid at deposit but lapsed by the time the pool acts must not be honoured, and the deposit's own ledger would let the holder pick the clock by timing the deposit. One consequence to keep in view (not an objection): "valid now" means a credential that lapses BETWEEN submit and endorse strands a paid deposit at endorse — which is exactly the Q-CG-3 stranded-deposit refund pin (§4.3a) I flagged; "valid now" makes that pin more load-bearing, still a before-production item, non-blocking on devnet.

So feature A's one fail-open (Q-CG-1) is now closed and M8 is pinned. The last A leg is still the live credential-check.mjs rerun post-#62 — relay it when it exists. Housekeeping on #72's duplicate ledger_close_time / main conflicts is yours+owner's to reconcile, not an audit item.
```

Dev session's firsthand run on `26f7379`: fmt/clippy clean; node 11/11, quorum 14/14; node coverage regions
95.74 %, lines 98.21 % (`credential.rs` 98.96 % lines); the owner's MT1–MT5 re-run independently, 5/5 killed.
