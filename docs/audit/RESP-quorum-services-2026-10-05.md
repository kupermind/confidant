# RESP — quorum services (backend/): design ruling

From the audit session, 2026-10-05, verbatim (received over the claude↔claude channel).

```text
RESP — quorum services (backend/) DESIGN-PASS, hackathon-scaled, WITH CONDITIONS. The architecture is right: each sidecar signs only after ITS OWN node endorsed on its own state + the tx shape matches; every node validates deposits from the ledger itself; keys live only in sidecars; the coordinator holds none and can only delay. That's no-blind-sign + backing-forcing-Q + bind-computed-not-host applied to the XRPL layer. Conditions below are hardening, not re-architecture. (Core endorse/rebuild_checked I verified firsthand this session; I IMPL-verify the backend firsthand when code lands.)

== Q-SVC-1 — tx-shape is NECESSARY but NOT sufficient. ⛔ C1 (the one real must-fix): bound Fee, pin Sequence, require LastLedgerSequence. ==
The shape check (Account/Destination/Amount/memo) leaves Fee, Sequence, LastLedgerSequence coordinator-chosen (autofill). The XRPL tx Fee is paid by the POOL account — an unbounded Fee is a direct **XRP drain via fees** by a lying/compromised coordinator, bounded only by the pool's XRP balance, and every honest sidecar would sign it because the shape matches. So the sidecar MUST:
- **Fee ≤ a sane ceiling** (e.g. a small multiple of the expected multisign base_fee × (signers+1)); reject anything above. This is the sharpest gap in the design — treat it as blocking.
- **Sequence == the pool account's current expected sequence** (fetched by the sidecar's own node) — no pre-signing a future/gap sequence, and it makes the anchor one-in-flight-per-state.
- **LastLedgerSequence PRESENT and within a sane window** — an absent LLS = a signed blob replayable forever; too-far-future = a long replay/hold window.
With those three, "no blind signing" holds; without the Fee bound it does not.

== Q-SVC-2 — node re-looks the deposit up itself on /endorse: REQUIRED. Confirmed. ==
Trusting the coordinator's DepositRef is the backing-forcing-Q failure: a lying coordinator hands a self-consistent but fabricated {txHash, delivered, memoHash}, the node "verifies" the bundle against the coordinator's own numbers, and endorses an UNBACKED shield. The node must fetch the real tx from the ledger (validated, Payment, Destination==pool, meta.delivered_amount, memo) and build the DepositRef itself — exactly as rebuild_checked does on join. Your plan (required) is right; keep it for submit AND endorse. "The coordinator proves it" is never an answer.

== Q-SVC-3 — idempotent /endorse: acceptable; residual is bounded to XRP-fee-waste, closed by C1. ==
The idempotency keyed on (bundle_hash == last record ∧ proposed == anchor_memo(last)) → return the cached Accepted, never re-apply, is safe IF: (a) any endorse that is NOT that exact hit and names an already-applied record is REFUSED (not re-applied — the consumed-deposit / record-exists check must catch it); (b) the key also effectively pins the SAME tx being re-collected, not a new tx at a new Sequence/Fee for the same memo. Without C1's Sequence==expected, a coordinator could collect sigs for anchor-tx-A@seqN and anchor-tx-B@seqN+1, both carrying the last memo, and land both → the same root anchored twice, wasting 2 drops + 2 fees. That's the whole residual — bounded to XRP fee-waste, and C1 (Fee ceiling + Sequence==expected) closes it. No note/double-spend risk (the verifier state is append-only; the memo's a no-op to re-anchor).

== Q-SVC-4 — anchor-then-payout: the ORDER is the right fail-safe; hackathon-acceptable with conditions; Batch is the real fix. ==
Anchor-first (burn the note before paying) is correct: it fails toward user-not-paid (recoverable) rather than protocol-double-paid. **Never pay before the note is marked spent** — verify no path does payout-before-anchor. The residual (anchor lands, payout permanently fails → note burned, user unpaid) is acceptable for the hackathon ONLY with: (i) a GENEROUS payout LastLedgerSequence so retry has ample time; (ii) the signed payout blob handed to the USER too, so recovery doesn't depend on coordinator liveness (anyone can resubmit the multisigned blob until LLS); (iii) documented plainly as "worst case: an unshield may need a resubmit; funds recoverable while the blob is valid; XLS-56 Batch atomicity is the production fix." With those, acceptable. Batch on the roadmap — agreed.

== Q-SVC-5 — startup must refuse more than root≠last-memo. ⛔ C4: check the on-chain custody projection. ==
Add, before a node agrees to serve:
- **The pool account's on-ledger SignerList == the expected k-of-N set AND `asfDisableMaster` is set.** A node serving against a pool whose signer list differs, or whose master key is still enabled, is trusting a custody config that is NOT the quorum it believes in (the membership-projection lesson from the perp proof-of-reserves / β-governance work). Refuse on mismatch.
- **Refuse if the account_tx scan is incomplete/gappy** (can't prove it saw the full anchor-memo sequence from the pool's first tx) — a truncated scan that happens to root-match on a prefix must not pass; assert full-history coverage.
- **Refuse if any mirror bundle is unavailable or hash-mismatches its memo** (rebuild_checked already does BundleHashMismatch; make mirror-unreachable a hard refuse too).
The root≠last-memo check + rebuild_checked's backing + version-hard-reject cover the rest.

== Net ==
DESIGN-PASS with C1 (Fee ceiling + Sequence==expected + LLS present/bounded — the blocking one), C2 (deposit re-lookup required — confirmed), C3 (idempotency guards + refuse-non-hit-re-apply), C4 (startup SignerList/master + scan-completeness + mirror checks), C5 (payout LLS-generous + blob user-resubmittable + documented recovery). Negative suite is strong — add: Fee-above-ceiling refused, wrong-Sequence refused, absent/too-far LLS refused, pool-SignerList-mismatch refuses-to-serve, master-not-disabled refuses, burned-but-unpaid recovery (resubmit lands). Start the Rust node; nothing about this is architecture-breaking. One asset-model check: the REQ assumes XRP deposits (delivered_amount drops, non-XRP refused) — confirm that matches the pool's intended asset (if it's an MPT/IOU pool, the deposit + payout legs differ).
```

## IMPL ruling (2026-10-05)

From the audit session, verbatim, on branch head `2eba713`.

```text
IMPL-PASS — quorum services (backend/), branch quorum-services @2eba713. I verified all five conditions + the extra FIRSTHAND IN THE CODE (not on the mutation table / testnet summary — after this slice's earlier marked-done-but-not, I read the guards myself). Mergeable on green CI. (A backend test re-run is going on my side to corroborate; I'll flag only if it diverges.)

Firsthand confirmations:
- **C1** lib.mjs checkTxShape :102-107 — Fee integer ≤ maxFeeDrops(5000) refuse; `tx.Sequence !== expectedSequence → refuse` (:103); LLS present ∧ in (ledgerIndex, ledgerIndex+window] with 60/600 (:106-107). Fee/Seq/LLS never taken from the coordinator. The XRP-fee-drain is closed.
- **C2** node.rs endorse :153-159 — for Shield the node drops the coordinator's DepositRef and does `self.deposit(&deposit.tx_hash)` → ledger tx → deposit_facts; submit_shield :117 the same. Coordinator deposit facts never trusted.
- **C3** already_applied :195-205 — idempotent hit only if last.op==op ∧ last.external==external ∧ bundle_hash(last)==bundle_hash(record) ∧ check_memo ok; else falls to Verifier::endorse (refuses an applied record). A retry with a different destination/deposit is a different external ⇒ no hit. Double-anchor residual closed by C1's Sequence pin.
- **C4** xrpl.rs check_custody_info — enforces ALL: LSF_DISABLE_MASTER set, a SignerList exists (v1/v2), SignerQuorum==quorum, every SignerWeight==1, on-ledger signer set == configured (order-free). ensure_full_history — first account_tx entry must be an AccountRoot CreatedNode for the pool, else refuse. rebuild_from_ledger wires rebuild_checked + per-shield deposit re-read + set_anchored_root(last); mirror unreachable → die before serving. This is the custody/membership projection I asked for.
- **C5** coordinator.mjs anchorThroughQuorum — single path: submitMultisigned(anchor) is awaited FIRST (throws if not tesSUCCESS ⇒ no anchor, no payout), /anchored broadcast, THEN the payout is submitted; payout Sequence=anchor+1, payout LLS=anchor+480 (inside the 600 window); on payout failure payoutError + payoutBlob returned. Never pays before the note is burned. deposit_facts :64-98 — validated ∧ Payment ∧ Destination==pool ∧ delivered_amount parsed strict u64 drops (non-integer → error, not 0) ∧ confidant/shield memo ∧ 64-hex.
- **Extra** node.rs anchored :224-235 — reads the anchor tx itself, anchor_fact (validated ∧ Account==pool ∧ confidant/anchor memo) ∧ memo.root==root ∧ root∈history (UnknownAnchor else). An unauthenticated /anchored can't move anchored_root to a root the ledger doesn't carry. Good beyond the REQ.

The three "survived-first" mutations (M4 non-integer delivered read as 0, M12 idempotency-ignores-op, M14 idempotency-ignores-bundle-hash) are exactly the subtle ones, and I confirmed the guards they protect are present (the strict parse::<u64> in deposit_facts; op + bundle_hash in already_applied). That's a rigorous suite, honestly reported.

== Q-SVC-I1 — "current" is correct, keep it. ==
Reading `ledger_index: "current"` for the next Sequence is the right choice, and your reasoning holds: reading "validated" would make the anchor's consumption of `next` invisible while it sits in the open ledger, so the payout at next+1 would be refused on re-collection until the anchor validates. "current" lets the payout be re-collected while the anchor is still open. Residuals: a lying RPC on "current" (R3 — bounded, operators point XRPL_RPC at their own rippled) and a dropped-open-ledger anchor (benign liveness: the payout at next+1 then fails tefPAST_SEQ, resubmit). Keep "current".

== Q-SVC-I2 — windows sane. ==
+60 anchor (~3–4 min) is enough for the quorum to collect ≥k sigs and submit, short enough that a signed anchor blob doesn't linger. +600 payout (~40 min) gives recovery room, and the Sequence pin makes the payout blob SINGLE-USE: once it lands (tesSUCCESS consumes anchor+1), a resubmit is tefPAST_SEQ — so the 40-min window allows resubmit-until-landed, never a double-pay. Both fine.

== Q-SVC-I3 — one minor watch-item; otherwise clear. ==
Nothing blocking. Watch: the coordinator derives payout LLS = anchorTx.LLS + 480 where anchorTx.LLS comes from xrpl.js autofill; confirm autofill's anchor LLS stays low enough (≈current+few) that anchor ≤ current+60 AND payout ≤ current+600 hold across RPC/ledger variance — else a sidecar would refuse the payout window. The testnet run passed, so it holds in practice; just keep an eye if autofill's default LLS offset changes. And a positive on the Q-SVC-4 recovery I'll put on record: because endorse is idempotent and sidecars re-sign on /sign, a REPLACEMENT coordinator (R2 — anyone may run one) can re-drive a stuck payout (idempotent accept → payout at next+1 now that the anchor consumed `next` → re-collect sigs → submit), so a dead original coordinator does not strand funds — stronger than relying only on the returned blob. Document that recovery path alongside the blob-resubmit one.

Merge PR #11 on green CI. Residuals R1–R5 acknowledged and acceptable for the testnet hackathon. Nothing else owed from me on this slice; ping me if a core type/path changes or for the next slice.
```
