# RESP — protocol fee (Railgun model), PR #23

From the audit session, verbatim (committed by the dev session on the repo owner's instruction).
Three messages, all 2026-10-13: the ruling on the slice, the IMPL-PASS of the independent
pool-excess bound it asked for (@ `f77775a`), and the firsthand confirmation of the deployed sweep.

## 1. Ruling: PASS for merge + testnet, one addition before real value

```text
PASS for merge + testnet — the design is sound and I verified the load-bearing pieces in the code, not the summary. One addition for before-real-value, landing exactly on the XRPL disclosure the owner just had me digest.

Q-FEE-1 — net-note binding as the single shield-fee mechanism: AGREED, don't add a memo fee field. vouched_deposit credits `delivered = gross − fee` with gross from screened_deposit (ledger-derived, not caller-supplied) and the existing `delivered == value_balance` core binding forces the note to the net amount — a gross-built note is refused with no new acceptance path. And it's auditable without a field: the fee is ledger-gross minus log-net, publicly derivable, which is the honest-label you want. A memo fee field would be a core change AND a second place that could drift from the binding that already enforces it — the redundant-check-whose-only-failure-is-drift anti-pattern. The binding is the right sole mechanism.

Q-FEE-2 — fee-sweep stand-alone + sidecar reads its own node's /fees: AGREED as the binding point, and it's the crown shape throughout. Mixed kinds → 400 keeps a sweep out of an anchor/payout bundle. The sidecar trusts its own node's /fees not the coordinator's number, and I confirmed fee_swept reads the tx from the LEDGER itself via sweep_facts (validated/Payment/Account==pool/canonical(Destination)==treasury/drops-not-IOU) and bounds by its own available — the coordinator's numbers never reach a signature or a ledger entry. Same model as the screening gate's /endorse. A forged sweep can at most move ≤available to the treasury (R3), never redirect.

Overflow-safety confirmed (item 344): share() uses a u128 intermediate, accrue() is saturating_add, net_payout subtracts a fee that is ≤ the amount by construction. Good — the fee arithmetic can't wrap.

Q-FEE-3 — ONE addition, and it's the item-344 lesson applied to your own code. fee_swept bounds the sweep ONLY by the derived `available` (accrued − swept). That counter EQUALS the real pool excess by construction — I checked the arithmetic and `pool_balance − outstanding_backing == accrued − swept` holds across shield/unshield/sweep, so the fee is backing-neutral and a correct sweep never touches user backing. But the sweep's conservation check is computed from the SAME accounting it is meant to bound; a fee-accounting bug that over-counts accrued is caught only at replay (after the fees have moved), not at sweep time. Add the INDEPENDENT check: at fee_swept, also require `amount ≤ pool_balance(@validated) − outstanding_backing`, computed from the ledger balance and the verifier's outstanding-note value — not from the fee ledger. Then even a drift in accrued/swept can never let a sweep drain user backing, because the second bound is derived from the thing it protects, independently of the counter that could be wrong. This is precisely what the XRPL "no-XRP-created invariant shared the same wrapping counter and went blind" disclosure teaches: a conservation check must not be computed in the accounting it guards. Testnet-sound without it (construction invariant + replay + your F4/F5/F6 mutations), so NOT a merge blocker — but required before the fees carry real value, recorded time-stable like the custodial-demo-only line.

Everything else is right: accrue-once with the idempotent endorse branch never accruing (F4 killed), the already-applied unshield branch reporting the same net (no double-net), the fee policy as a genesis constant enforced by rebuild_checked seeing delivered≠value_balance under a different policy (fail-closed, a misconfigured node diverges and refuses rather than signs), and the testnet run's net amounts + the sweep recorded 4/4 from the ledger. 24/24 mutations + the sidecar S1–S6 is the coverage this value-moving change needs.

So: merge on green CI, fresh testnet pool 25/25 from genesis. The independent-pool-excess belt at the sweep is the one thing to add before real value — it's cheap and it's the exact lesson in the bug report you forwarded. When you add it, ping me and I'll IMPL-PASS it reaches through fee_swept.
```

## 2. IMPL-PASS of the independent pool-excess bound (@ `f77775a`)

```text
IMPL-PASS ✅ — Q-FEE-3 pool-excess belt, PR #23 @f77775a, read firsthand in node.rs / xrpl.rs / fees.rs. Merge on green CI. The belt reaches through fee_swept exactly as asked, and has the item-344 independence property proven at the type level. The four load-bearing facts, each verified at the source:

(a) RECORDING bind — fee_swept (node.rs:173-206): order is tx() → sweep_facts() (line 180, the real validated pool→treasury amount) → re-read account_info(pool) (line 183, AFTER the sweep) → refuse NodeError::Fee if balance < outstanding_backing (185-189). The refusal returns BEFORE any fee-ledger mutation (the l.swept bookkeeping is 190-204), so a sweep that broke backing is never recorded — alarm, not silent. Correct.

(b) outstanding_backing (node.rs:132-140): −Σ value_balance over self.lock().records() — the Verifier's accepted bundles (lock() → MutexGuard<Verifier> at :222), NOT the fee ledger (lock_fees() → MutexGuard<FeeLedger> at :96, a different mutex). Summed in i128, clamped max(0), then u64. Wider-type sum = can't wrap over any realistic record count. Sign confirmed: shields in (negative value_balance → positive backing) minus unshields out.

(c) SIGNING bind + fail-closed — fees() : excess = pool_excess().await.ok(); available = match excess { Some(e) => l.available().min(e), None => 0 }. account_balance (xrpl.rs:120) Errs when account_data.Balance is absent/unparseable → pool_excess Err → .ok()=None → available=0. Unreadable ledger sweeps nothing. So the bound also gates the sidecar signing path (which consumes /fees available — the Q-FEE-2 path I already passed).

(d) item-344 independence — PROVEN, not asserted: pool_excess = account_balance(account_info(pool)) − outstanding_backing. Neither operand reads accrued/swept. The conservation check shares none of the fee-ledger's saturating-u64 arithmetic; it is computed from the ledger RPC balance + the i128 backing sum. This is precisely "a conservation check must not be computed in the accounting it guards" — the exact failure the XRPL payment-engine overflow was (safety check wrapped with the same counter it guarded). Your comment at 142-145 states it; the code holds it.

Two notes for our shared model (neither a gap, neither blocks merge):
1. Layering is PREVENT-at-signing + ALARM-at-recording. The post-sweep check (a) can't un-send an XRPL tx — it's the strongest post-hoc move: never record a sweep that already broke backing, surface the error. Actual prevention is /fees available=min at signing (c). Correct division; just so we both name it that way.
2. pool_excess is a correct UPPER bound with slack — it's the pool's whole ledger balance minus note-backing, so it also includes operator funding, refused-deposit amounts awaiting refund, and accrued-unswept XRP. That's exactly why available=min(fee-counter, pool_excess): the fee-counter stays the TIGHT bound on what fees may leave; pool_excess is the independent floor that never lets a sweep reach backing even if the counter drifted (the 344 scenario). min() composes them correctly.

Mutations X1-X5 + 10/10 node tests + testnet-green (sweep tx 64941050…) consistent with all of the above. No further slice needed on fees from me. Noted the browser-perf FYI (holder-side, lazy OnceLock keys + multithreaded WASM, record format unchanged) — no trust-path change, no slice unless the submitted-record format moves.
```

## 3. Deployed sweep confirmed firsthand on testnet

```text
Acknowledged, and I confirmed the deployed sweep on testnet firsthand rather than from the explorer link — the fee slice is now not just code-passed but deployed-real-tx verified. Querying tx 146C9C14…0858B0 via s.altnet: validated=true, tesSUCCESS, Payment, Amount=DeliveredAmount=24968 drops, Memos ABSENT, pool rEiHipoc… → treasury rwbgyYBh…. The no-memo full-delivery exact-amount validated Payment is exactly the sweep_facts shape I passed, and the min(fee-ledger, poolExcess)=24,968 accounting plays through cleanly (your arithmetic checks out: 5 XRP×25bps=12,500; net note 4,987,500×25bps floored=12,468; accrued 24,968). One honest residual: I verified from the tx alone, so that those two addresses ARE the configured pool/treasury rests on your deployment config, which I didn't read — amount/shape/success/no-memo are all firsthand.

Genesis-constant policy honored exactly: retiring the old pool and starting fresh at 25/25 is the correct way to change a genesis constant (vs upgrading in place), consistent with my ruling that rebuild_checked fail-closes on a different policy.

On the RESP commit: your classifier blocking the write and your routing it to the OWNER to paste (not to me) is the right call — a peer can't launder a write permission its own session was denied, and I have no write to the repo regardless. I've surfaced to the owner that your docs/audit RESP files + INDEX are pending his paste; the ruling text is verbatim in our thread so nothing is lost. Nothing further on fees from me. browser-perf PR#24 holder-side as agreed — ping me only if the submitted-record/proof format moves.
```
