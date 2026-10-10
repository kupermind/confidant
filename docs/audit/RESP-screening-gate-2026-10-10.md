# RESP — boundary screening gate

From the audit session, 2026-10-10, verbatim.

## CONDITIONAL PASS (design + impl, must-fix: destination canonicalization)

```text
CONDITIONAL PASS — the architecture is sound and both design questions resolve, but there's ONE MUST-FIX before this gate is relied on, confirmed in the code. Verified firsthand at origin/screening-gate (not from the summary).

Q-SCR-1 — /endorse as the binding point: AGREED. screened_deposit/screened_destination are called in endorse() for BOTH ops (node.rs:199/205), so a malicious coordinator cannot route around a signer's list — the real gate is each signer's own node, exactly where it must be. The coordinator is untrusted, so requiring IT to carry a list would be UX (early-reject), not security; don't make it a requirement. The "sent straight to a sidecar → its own node refuses (409)" testnet line is the property that matters.

Q-SCR-2 — standby in validated ledgers from the node's own RPC: ACCEPTABLE, and better than a wall clock. It's ledger-anchored (the same consensus index the deposit's ledger_index comes from), objective, and fail-SAFE on a lagging RPC (a behind view makes the deposit look fewer ledgers back → more conservative → waits, never admits early). It's no new trust (same rippled the node already reads deposit_facts from), and the quorum needs k independent RPC views to pass. check_deposit's `validated < deposit_ledger + standby` with saturating_add is correct.

⛔ MUST-FIX (my Q-SCR-3 answer) — the DESTINATION screen is evadable by address encoding. check_destination does a raw `blocked.contains(destination)` on the CLIENT-SUPPLIED payout string (node.rs:146), and there's no canonicalization in the node src; the OFAC list is classic r-addresses. So a listed party takes an unshield payout by giving their destination as an X-address (or any non-canonical form) that decodes to the listed r-address — the raw string differs, contains() misses, the exit-screen passes. The whole point of (c) "an unshield whose destination is listed" is defeated by a trivial encoding variant. The SENDER side is safe precisely because it isn't client-supplied — deposit_source reads the ledger's Account, which is already the canonical r-address; that asymmetry is the tell.

Fix: canonicalize to the classic AccountID (decode X-addresses, drop any destination tag) on BOTH sides — the list entries at from_file time and the destination at screening time — so list, sender, and destination are all the one canonical form. Name the precondition honestly: whether this is a LIVE end-to-end bypass today depends on whether the payout path accepts an X-address destination (I didn't trace the sidecar's payment construction — you confirm). If it does, it's live; if the payout only accepts classic r-addresses, it's latent. Either way it's a must-fix, because a trust-path gate must not rely on a downstream reject it doesn't own — and an evadable exit-screen contradicts the honest "gatekeeping on public data" label you set. It's a small fix; do it before merge, or at the very least before the real OFAC list is deployed with a documented gap until then.

Minor, confirm not fix: from_file is fail-closed (Err on bad JSON and missing file) — good. Confirm main.rs PROPAGATES that Err (refuses to start), rather than swallowing it into a silently-empty list, which would be screening-off wearing a startup log.

Affirmed and right, so you don't re-touch them: replay NOT re-screening history (rebuild_checked) is the subtle correct call — screening is acceptance-time admission, not a state invariant; re-screening on replay with a changed list would diverge the rebuilt root from the quorum's, and your 185584ms "root equals the quorum's" line is the proof it's separated correctly. Ledger-only inputs (nothing from client/coordinator for the sender/standby) is the crown-shape — bind the decision to evidence the node fetches, not to caller input. R1–R4 are honest and correct; R4 (quorum fails closed toward the strictest k — an op needs k nodes that all accept, so the effective list is the union of the required signers') is the right property: no single node can un-list. The 11 killed mutations incl. the G6/G7 submit-and-endorse-bypass pair and G11 403→409, plus the testnet run showing eve refused at the coordinator AND at a direct sidecar, is the coverage this deserves.

On the fix + green CI: merge. Then the deployed quorum's OFAC list + standby 2 — with the destination canonicalized, the exit-screen actually holds.
```

## IMPL-PASS on the must-fix @ `f50aac3`

```text
IMPL-PASS on the must-fix — verified firsthand at f50aac3, correct and complete. The canonicalization is applied on BOTH sides, which was the thing to get right: canon() at load (new():51 maps every list entry) AND at lookup (is_listed():87 `blocked.contains(&canon(account))`, used by both check_deposit and check_destination), so list, the ledger-read sender, and the client-supplied destination are all compared as the classic AccountID with the tag dropped. account_id() verifies the double-SHA-256 checksum before trusting the payload (line 67), classic and X (0x0544/0x0493) both reduce to the 20-byte AccountID, foreign prefix/flag/alphabet → None — so the X-address spelling of a listed r-address now canonicalizes to the same entry and is blocked. The testnet line (eve as a testnet X-address with tag 7 → 403) is the property demonstrated end to end. sha2 0.10 RustCrypto is a reasonable dependency for this and it's node-side, not in any enclave TCB.

Your precondition answer is the right resolution and I want to name why it's better than "it was latent anyway": the X-address payout was refused before only because xrpl.js autofill rewrote the Destination and the sidecar's shape-rule then saw a mismatch — a DIFFERENT check catching it by accident. A trust-path gate relying on an unrelated layer's side effect is exactly what rots when either side changes. Now the screening gate OWNS its own property and the sidecar string-compare stays as an independent second layer — that's the correct ordering, not redundant.

main.rs propagation confirmed (from_file(...).unwrap_or_else(die), before the ledger opens, exits 1 on missing/bad-JSON with the path in the message) — fail-closed, never a silently-empty list. Good.

And the G15 story is the lesson, not a blemish: a mutation that SURVIVES because your only vector had a broken checksum means the vector was degenerate for that guard, not that the guard works — adding valid-checksum foreign-prefix and flag-2 vectors to kill it is the same "the input has to exercise the path" discipline that the de-coinbasing forced on the bridge side. 15/15 with G12–G15 is the coverage this earns.

ONE MINOR RESIDUAL, not a blocker: an undecodable list entry falls back to matching only its raw self (canon's unwrap_or_else), which means a malformed OFAC entry is silently INERT — it blocks nobody and is still counted in the startup "N listed addresses." On a clean fetch-ofac.mjs output that never bites, but a corrupted or truncated fetch would screen nothing for those entries without anyone seeing it. Recommend a startup WARN that distinguishes decodable from inert entries ("N canonical, M undecodable") so a degraded list is visible rather than a reassuring count — the same reason main.rs dies loudly on a missing file. Your call whether that's this slice or a follow-up; it doesn't gate the merge.

PASS. On green CI, merge, and deploy with the OFAC list + demo list + standby 2 — with the destination canonicalized, the exit-screen holds against the spelling, not just the naive form.
```
