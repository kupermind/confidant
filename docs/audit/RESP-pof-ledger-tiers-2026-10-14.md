# RESP — proof of funds on the ledger (Tiers 1–3): design view

From the audit session, 2026-10-14, verbatim; also posted on PR #27. A design view on the proposal
as described — not an authorization to ship; the firsthand IMPL-PASS is owed on the built slices
when their REQ reaches the audit on-site (A first). To write into the design before Saturday: the
three Q-PF-1 pins, the two Q-PF-2 Tier-3 caveats, the Q-PF-3 no-ship-without-IMPL-PASS rule, the
expired-Credential reserve path, the rate limit on `POST /pof/verify`.

```text
For the owner, verbatim. This is my design view on the proposal as described; the firsthand IMPL-PASS is owed on the built slices when the REQ reaches me on-site (A first), and nothing below is an authorization to ship.

Q-PF-1 — YES, the binding rule is right and it's the crown shape: a sidecar signs only on ITS OWN node's verdict, computed from that node's state, never derivable from the coordinator's claim (ask-the-node, like /fees). Stand-alone kind, no memos, Fee/Sequence/LLS pinned as for payouts, tx shape pinned against the verdict — all agreed. Three pins I'd make explicit and non-negotiable:
 (i) the URI must be the node's OWN recomputation H(X‖L‖E) from the (X,L,E) it verified — never the coordinator's hash echoed back; the node signs its own computation or it refuses.
 (ii) the nullifiers-still-unspent check must be RE-RUN at signing time, and this is mandatory, not advisory. The gap between POST /pof/verify and the CredentialCreate signature is a live window, and a cached verdict replayed after the funds moved is the attack it closes. The verdict cache is only safe if the sign-time re-check gates it.
 (iii) write the semantic line into the design: the Credential proves "the borrower CONTROLLED X as of ledger index L," not "controls it now." Between sign-time and the lender's read the holder can spend, and without Tier 2 that window is unguarded — this is the notarization-is-not-backing distinction, and a lender who treats an unexpired PoF Credential as current solvency is trusting the borrower not to have spent. Base PoF is a point-in-time attestation; say so on the object's meaning, not just in the Tier-2 stretch note.

Q-PF-2 — YES, and it needs two distinct lines on the Tier-3 slide, not one:
 (1) arbiter = quorum widens k-signer collusion from "move the pool account's public XRP" to "co-sign, with either the lender or the holder, the spend of a specific shielded collateral note." Same k, materially larger blast radius; it belongs next to the custody caveat, stated as its own capability.
 (2) the joint FullViewingKey (ak‖nk‖rivk) hands the lender — a group member — permanent visibility into that collateral note, past the loan term. In a system whose thesis is hiding the graph, Tier 3 leaks one note's graph to a counterparty forever. The privacy claim must be stated as not covering Tier-3 collateral — a security/privacy claim that silently excludes a tier is the kind that decays; write the exclusion now.

Q-PF-3 — realistic, with one hard rule that overrides the schedule: a new SIGNING path ships only with its own firsthand IMPL-PASS completed inside the window; a READ gate does not carry that risk.
 • A (credential-gated shielding) is a Ledger read + refuse-at-submit/endorse, fail-closed — same shape as the screening gate already IMPL-PASS'd, and the sidecar needs nothing new. Realistic by Saturday.
 • Tier 1 (credential-pof) is a new signing path with the Q-PF-1 pins + the sign-time re-check; it does NOT ship unless A lands AND Tier 1 gets its own IMPL-PASS in-window. If the IMPL-PASS doesn't fit, it is slide-only — the flow-diagram fallback is the correct outcome, not a failure. A demo deadline never ships an unverified signing path.
 • Tier 2 (credential-revoke) is another signing path plus a node state-model change (watch list, revocation): stretch at most, same no-ship-without-IMPL-PASS rule.

To write into the design before Saturday: the three Q-PF-1 pins (node-recomputed URI; mandatory sign-time nullifier re-check; the point-in-time semantic line); the two Q-PF-2 Tier-3 caveats; the Q-PF-3 no-ship-without-IMPL-PASS rule. Two more, both yours/the owner's lane but worth a line each: expired PoF Credentials consume owner reserve on the pool account — give an anyone-deletes-expired path or the reserve ratchets upward with issuance (accounting, not trust-path, but it touches the pool-excess story); and POST /pof/verify is a new unauthenticated ingress that forces a proof verification per request — bound the verify cost / rate-limit per node, or a flood of bogus records griefs the verifiers (opsec, not trust-path). The "record never persisted or logged, only the nullifier watch list" property is the right privacy posture — keep it as a hard invariant on that endpoint.

Send A's REQ when it's built and I'll IMPL-PASS it firsthand first.
```
