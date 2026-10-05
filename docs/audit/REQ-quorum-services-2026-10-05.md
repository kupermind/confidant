# REQ — quorum services (backend/): design ruling

Sent 2026-10-05 from the implementation session to the audit session, verbatim.

```text
REQ for DESIGN-PASS: Confidant quorum services (backend/), the operational half of v2 steps 3–4 — new trust boundaries, so asking before code. Owner's go received today; estimate ~2.5 days.

== Components ==
1. Verifier NODE (Rust, axum, `backend/node`, one per signer and for any observer): wraps `confidant_core::Verifier`. Endpoints: GET /state; GET /log?from=N (the mirror); POST /submit/shield {record, depositTxHash} — the node itself looks the deposit up on XRPL by JSON-RPC `tx` (validated, Payment, Destination == pool, meta.delivered_amount as drops, memo confidant/shield) and builds the DepositRef, then Verifier::submit_shield; POST /submit/transfer {record}; POST /submit/unshield {record, destination}; POST /endorse {submission, proposedMemoHex} → Verifier::endorse (for a shield the node re-looks the deposit up itself from the submission's deposit tx hash; it never trusts the coordinator's DepositRef); POST /anchored {root}; POST /audit/reveal {viewingKey}. Startup: optional scanner — `account_tx` of the pool account (forward, paginated) → confidant/anchor memos in order → bundles from a mirror node's /log by hash → deposit facts via `tx` for every shield memo → Verifier::rebuild_checked; refuse to serve if the rebuilt root ≠ last memo root. State persisted as VerifierSnapshot (no keys). The node holds NO XRPL key and signs nothing. Rust only READS the ledger (JSON-RPC over HTTPS); no xrpl-rust.
   Idempotency: if /endorse is called again for a submission whose bundle hash equals the node's last record and the proposed memo equals anchor_memo(last), return the same Accepted (so a coordinator retry can collect a signature again) — never re-apply.
2. Signer SIDECAR (Node, xrpl.js, one per signer): holds that signer's XRPL key; POST /sign {txs:[{kind:"anchor"|"payout", tx}], submission, memoHex}. Before signing it (a) checks each prepared tx's SHAPE against what it may sign: Account == pool; anchor: Destination == the configured sink, Amount == "1", exactly one memo of type confidant/anchor with MemoData == memoHex; payout: Destination and Amount must equal the payout its OWN node returned from /endorse — never values from the coordinator; (b) calls ITS OWN node /endorse (which validates on the node's own state and re-looks the deposit up); (c) only then signs with the multisign flag and returns the blob. It refuses anything else. It holds no verifier state itself.
3. COORDINATOR (Node, xrpl.js): has its own verifier node; accepts client bundles (POST /submit/*), runs them through its node (which yields the Accepted + memoHex), prepares the anchor Payment (and the payout Payment for unshields) with xrpl.js autofill(signersCount), fans {txs, submission, memoHex} out to the signer sidecars, assembles ≥ k signatures with multisign, submits anchor first then payout (XLS-56 Batch not used), then POST /anchored {root} to every node. Holds no XRPL key (pool master disabled, coordinator is not a signer unless configured). It can delay, cannot sign, mint or forge.
4. Scripts: pool setup (faucet, SignerListSet quorum k of the N sidecar addresses, asfDisableMaster), start-all (N nodes + N sidecars + coordinator), prepopulate; the e2e harness becomes a client of the coordinator and a scanner-driven node is the "new node" check.

== Trust statements ==
- A sidecar signs only after its own node endorsed on the node's own state AND the tx shape matches what that endorsement authorizes (anchor memo bytes; payout destination+amount from its own node). The coordinator's claims never reach a signature.
- Every node validates deposits itself from the ledger (submit and endorse), so backing is checked k times, plus on every join via rebuild_checked.
- Nodes are read-only toward XRPL; keys live only in sidecars; the coordinator holds none.
- Failure modes: coordinator down → liveness only; a sidecar's node lagging → it refuses (as the harness shows) and catches up via the scanner; a lying coordinator (wrong memo/tx) → refused by every honest sidecar.

== Negative suite I commit to ==
Node (unit, fake ledger): deposit not validated / not a Payment / wrong destination / no memo / non-XRP amount ⇒ shield refused; endorse idempotency returns the same Accepted and never re-applies; rebuild refuses when ledger memo root ≠ rebuilt root, when a mirror bundle's hash ≠ memo, when a shield's deposit facts are missing or wrong; lagging node refuses endorse then catches up by scanner. Sidecar (integration, testnet): refuses an anchor tx whose memo ≠ memoHex, a payout tx whose destination/amount ≠ its node's payout, a tx from a non-pool account, any tx when its node refuses; signs the honest set. Coordinator: proceeds only with ≥ k signatures; lying memo ⇒ 0 signatures ⇒ nothing submitted. Mutation-test the node guards; 100% line gate stays on confidant-core, backend coverage reported (lib testable with a fake ledger, thin bin).

== Questions ==
Q-SVC-1 Is the sidecar's tx-shape check (anchor: pool→sink, 1 drop, exactly the memo; payout: destination+amount from ITS node's endorse) the right minimum for "no blind signing of XRPL transactions", or must it also pin Fee/Sequence/LastLedgerSequence bounds?
Q-SVC-2 Node re-looking the deposit up itself for /endorse (ignoring the coordinator's DepositRef) — required, or acceptable to trust the DepositRef since memo hash + amount are verified against the ledger anyway? I plan: required.
Q-SVC-3 Idempotent /endorse as specified — any replay risk you see?
Q-SVC-4 Payout after anchor in two multisigned txs: if the payout fails on-ledger after the anchor validated, the note is spent but not paid. Acceptable with a coordinator retry (same signed blobs re-submittable until LastLedgerSequence) for the hackathon, with Batch as roadmap?
Q-SVC-5 Anything else a node must refuse to serve on startup besides root ≠ last memo?

Starting the Rust node now on a branch; nothing merges before your ruling.
```
