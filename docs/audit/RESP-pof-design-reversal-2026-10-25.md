# RESP — proof of funds: the audit withdraws its design verdict (REJECTED until a nullifier-free circuit)

From the audit session, 2026-10-25, verbatim, after the project lead's ruling (`docs/PROOF-OF-FUNDS.md`, top;
PR #85, issue #86). The IMPL-PASSes on the proof-of-funds slices (`RESP-pof-core`, `RESP-pof-endpoint`,
`RESP-pof-credential`, all 2026-10-24) stand **only as verification of the mechanics**; the design verdict of
`RESP-pof-design-2026-10-14.md` is **reversed to REJECTED**.

```text
Correction to my proof-of-funds audit — withdraw my verdict, it was wrong at the design level. My DESIGN-PASS and the IMPL-PASSes on #61/#67/#69/#75 stand ONLY as verification of the mechanics (fail-closed reads, no-blind-sign crown, the issuance bound, canonical binding) — they do NOT make the feature acceptable, and I am reversing the design verdict to REJECTED.

Why: PoF reuses the unchanged Orchard unshield-bundle shape, so the proof reveals the proven notes' nullifiers to every verifying node (the core verify checks them unspent — it must see them). When those notes are later spent, the operator links the proof's metadata (borrower/lender/amount/time) to the real spend. That regresses the pool's one load-bearing property — graph-hiding. Before PoF, no pool operation's privacy depended on a user action; PoF's only mitigation is the holder self-rotating the proven notes, in a required order. A human as a link in a security/privacy property is forbidden, categorically, and a new feature that regresses a prior guarantee is a failed design no matter how clean the implementation. I graded the honesty of the label ("snapshot, not a lock") instead of grading the feature against the existing guarantee — that was my miss, and it's a first-order one.

Corrected verdict: PoF REJECTED until a circuit proves control of ≥X WITHOUT revealing nullifiers (Tier-4). Self-rotation, "valid as of ledger L", and any state-at-L scheme do NOT count — a user-rotation step in the UI or docs is not a remediation, it is the failure made operational. The owner has ruled it forbidden. What happens to the merged/deployed PoF code is his to direct — I'm not instructing an action; I'm correcting my audit record so my prior passes don't stand as "PoF is safe." I'm tracking it as failed-until-Tier-4 and will flag any deployed/production path that presents the nullifier-revealing PoF as safe.
```

Dev session's record: the same day's finding that the package flow hands the lender the record (nullifiers to the
lender, contrary to requirement 6) and the two over-claiming labels are in `docs/PROOF-OF-FUNDS.md` and corrected in
PR #85. The ruling in the repository ("no human as the last link", `CLAUDE.md`, `SPEC.md` §8) is the lead's.
