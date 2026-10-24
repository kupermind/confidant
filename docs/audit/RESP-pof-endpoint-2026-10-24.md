# RESP — proof of shielded funds (hackathon feature C): Tier 1, the read side (`POST /pof/verify`)

From the audit session, 2026-10-24 (event day), verbatim; firsthand review of branch `pof-endpoint` at head
`f59bdb1` (PR #67). Ruling: **IMPL-PASS**, nothing owed. (The same message carried the ruling on the signing
path, PR #69; that part is recorded in `RESP-pof-credential-2026-10-24.md`.)

```text
Both ruled firsthand at the refs (f59bdb1 / 02691ce), not from the code map. Two verdicts.

════════ #67 — PoF READ SIDE: IMPL-PASS ════════
Verified firsthand, every load-bearing property:
- **Req 4 (freshness):** check_window judged against `ledger.validated_ledger_index()` the node reads ITSELF (node.rs:279), never the request's. Both subtractions are underflow-safe by ordering — `validated − ledgerIndex` only after `ledgerIndex > validated` is rejected; `expiry − ledgerIndex` only after `expiry ≤ ledgerIndex` is rejected (pof.rs:64,69,77,82). Edges judged.
- **Req 6 (read-only):** the `_permit` RAII guard spans the whole fn (released on every `?` and the final return); the single `.await` (validated index) is BEFORE the verifier-lock block, so the std-mutex verifier lock holds no await; `lock_fees()` is never touched (no lock-order hazard); no persist/log/file anywhere on the path — the verdict is the only output, echoing public fields + hash + the node's own root.
- **Ingress-cost:** POF_PERMITS=4 `try_acquire` → Busy(429), never queues; 256 KiB body cap at the route (413 before parse). Both confirmed.
- **pin-3:** node recomputes canonical bytes from the request's fields; client never sends the signed bytes. And the lender `'|'`+control exclusion (pof.rs:58) closes delimiter injection — since NO field can contain `|`, the canonical string is collision-free, so the node's recomputation binds exactly these semantics.
- **Amount from the request** drives both `vb==amount` and the sighash (node.rs:283-286). **X-address refusal is real:** `canonical()` re-encodes any input to classic 'r…' form, so an X-address never equals its input → `Some(_)` → refused; garbage → None → refused. **nginx** UI-origin exposes exactly `pof/verify`+`state` per node — no write path reachable from the browser.

Your (a)-(d): (a) windows/permits as fixed defaults — fine for the event; note the recency window is a sanity floor, not the lender's risk policy (the lender has validatedLedger+challenge in the verdict and applies its own), so hard-coding is harmless now; operator flags are a production nicety. (b) lender-in-clear to nodes — accepted for Tier 1, but it IS a metadata leak to the node OPERATORS (they learn lender↔borrower↔amount↔L); keep that in the honest label (it's not hidden from them), time-stable. (c) `root` leaks nothing — it's the whole-pool commitment, identical regardless of which proof was checked, already public via /state. (d) exposing /state per node — read-only public facts, adds only "which node answered," which is exactly the k-of-N evidence the lender wants. **#67 is clean; nothing owed.** (Your 12/12 + 14/14 re-run + pof.rs/http.rs 100% match what I read — confirmatory, not the basis.)
```

Dev session's firsthand run on `f59bdb1`: fmt/clippy clean; node 12/12, wallet 2/2 (release); node coverage
regions 95.94 %, lines 98.35 % (`pof.rs`, `http.rs` 100 %); the owner's 14 mutations re-run independently,
14/14 killed. Label to keep (from (b)): the lender id, borrower, amount and ledger window are visible to the
node operators a proof is posted to; not hidden from them.
