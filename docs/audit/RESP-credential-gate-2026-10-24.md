# RESP — credential gate (XLS-70, hackathon feature A): node side

From the audit session, 2026-10-24 (event day), verbatim; firsthand review of branch
`credential-gate-node` at head `2a2f6ae` (PR #59). Ruling: **IMPL-PASS** for the gate as a read-only
deposit check, for the devnet event. Required before production or any untrusted-operator / real-value
use: judge `Expiration` against the validated ledger's `close_time` instead of the node's system clock
(Q-CG-1). Follow-ups: the second-depositor node-level test (M8), the stranded-deposit refund pin
(Q-CG-3). The live `credential-check.mjs` result is owed to the audit after deploy.

```text
RESP — credential-gate node side (PR #59, head 2a2f6ae), firsthand IMPL-PASS. Reviewed at the ref, not on your summary.

VERDICT: IMPL-PASS for the gate as a read-only deposit check, for the devnet event — with Q-CG-1 (close_time) as the one fix I want before any non-devnet/real use, and two follow-ups. The owner may merge #59 and deploy for the event.

What I verified firsthand (the fail-closed substance, since a read gate's whole job is to refuse):
- THE critical fail-open axis is CLOSED: the subject the gate checks is `deposit_source(&tx)` where `tx = self.ledger.tx(tx_hash)` (node.rs :322/:336/:342) — the LEDGER's own answer for that tx, never the client body. So a client cannot present someone else's credentialed account; the lookup is keyed by the real on-ledger sender, and check() re-compares the returned entry's Subject against exactly that account (credential.rs). A deposit from an account with no credential fetches None → refused.
- Fail-closed in every direction I pushed: entry None / entryNotFound → refuse ("none on the ledger"); any OTHER lookup RPC error → `Err(e)` → NodeError::Ledger → 502, never accept (rpc.rs credential() `match`: only `entryNotFound` becomes Ok(None), everything else is Err); wrong (Subject/Issuer/CredentialType) → refuse; lsfAccepted clear → refuse; Expiration ≤ now → refuse; gate off → pass. An Ok-but-no-`node`-field response also → None → refuse. That's the whole lattice closed.
- Per-node, both paths: the gate is the last step of screened_deposit, called from vouched_deposit, called from submit_shield (:361) AND endorse(Submission::Shield) (:404) — so every node runs it against its OWN validated-ledger read, and the submitting node and each endorsing node all gate independently. No node vouches for an ungated deposit.

Q-CG-4 — sidecar needs NOTHING new: CONFIRMED firsthand. The gate lives in the vouch path, so a gated-out deposit never becomes something a sidecar is asked to sign — the existing "the sidecar signs nothing its node did not accept" covers it, and the anchor/payout/fee-sweep kinds are untouched. A is a read gate, as stated.

Q-CG-1 — close_time, and this is the ONE fail-open in the gate: Expiration is judged `exp <= ripple_now()` (system clock). A node clock running BEHIND would accept an EXPIRED credential — a false ACCEPTANCE on the compliance boundary, the exact fail-open a read gate must not have. Judge against the VALIDATED LEDGER's `close_time` instead: it matches rippled's own semantics (parent close time), it removes the clock-skew entirely (the node uses the ledger's time, not its host's), and it's cheap because the node already reads the validated ledger (the same `ledger`/ledger_entry path). Ruling: this is REQUIRED before production or any untrusted-operator / real-value use; for the DEVNET event it may ship on the system clock with the NODE-OPERATOR "keep the clock synced" line as a documented stopgap (immaterial on controlled devnet nodes, and moot entirely if the demo's credentials carry no Expiration). But it's small and it closes a real fail-open, so my lean is: do close_time now — don't carry a compliance-gate fail-open as clock-hygiene prose.

Q-CG-2 — fine. The nodes fail closed (403) regardless of the UI, so the security property holds; /config swallowing /policy to `credential: null` is a UI-only degradation, not a gate hole. Do NOT make /config fail — that breaks the whole UI for a worse outcome. Optional UX nicety, non-blocking: show "eligibility: unknown" rather than hiding the row, so a user isn't surprised by a 403 they weren't warned about.

Q-CG-3 — the re-check at endorse is CORRECT (don't shield a now-revoked party; fail-closed). The thing to PIN is downstream of the gate, not in it: a deposit already PAID whose shield is then refused at endorse (credential revoked or expired mid-flow) must be RECOVERABLE. Resubmission (#56) covers the temporary case (expiry renewed); a PERMANENT revocation needs a refund path (§4.3a) or the user's deposit is stranded. Confirm a permanently-refused deposit's funds are recoverable; if §4.3a isn't in the event scope, LABEL it (deposit-at-risk-if-revoked-mid-flow-without-refund). Non-blocking for the devnet event (worthless coin + §4.3a is a known candidate), but pin it — stranding a paid deposit is a real fund-safety gap for production.

M8 (your survived mutation) — I agree with your read and it's a SHOULD-FIX, not a block. The lookup keyed by the real depositor is the actual gate and it's fail-closed; the Subject re-check is a belt, and M8 surviving means that belt isn't pinned at the node level (your single-depositor test can't distinguish a literal from the depositor). The failure direction is a false REFUSAL, never a false acceptance, so the security property holds — but a survived mutation is an untested protection site, so add the second-depositor node-level test to pin it. Not blocking.

Honest label — correct and keep it exactly: "gatekeeping on a trusted issuer's say-so at the public boundary; nothing about anyone's notes; not a KYC'd pool." The gate proves the issuer vouched, not any identity/compliance fact — that's the honest claim, and it doesn't outrun what the code does.

So: IMPL-PASS, merge and deploy for the devnet event. Before production (or the first time this faces untrusted operators / real value): close_time (Q-CG-1). Follow-ups: the M8 second-depositor test, and the Q-CG-3 stranded-deposit refund pin. Send me the live credential-check.mjs result (fresh-account shield → 403 naming the credential; issue + accept; same record+deposit → anchored) when it exists — that's the last firsthand leg, the gate refusing and then passing on a real ledger. Commit this RESP to docs/audit verbatim as usual.
```
