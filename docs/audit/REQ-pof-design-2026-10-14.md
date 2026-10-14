# REQ — proof of shielded funds (design review of the owner's spike, PR #29)

Sent 2026-10-14 from the implementation session to the audit session, verbatim. A design review of
the repo owner's pre-event prototype for hackathon option C (ROADMAP §0, PR #27); nothing merged.

```text
REQ (design review only, no merge, no slice yet): PR #29 "SPIKE (not for merge): proof of shielded funds, Orchard circuit unchanged" — the repo owner's pre-event feasibility prototype for the hackathon's option C (ROADMAP §0 in PR #27: a holder proves to a lender that they control unspent pool notes worth ≥ X without revealing notes or total; verdict may become an XLS-70 Credential). The owner asked for an external design review; I'm relaying his five questions plus my own read. Branch spike/proof-of-funds, +168/−5 in core/src/sighash.rs, verifier.rs, wallet.rs, core/tests/pof_spike.rs.

== Design (as implemented in the spike) ==
Build: Wallet::build_proof_of_funds(amount, challenge) = the unshield path (inputs ≥ X, change back to self, value_balance = X, external = "") but signed over a separate sighash: pof_sighash(bundle_commitment, pof_ctx) with BLAKE2b-256 personalization "ConfidantPoF_v1_" and ctx = len(challenge) ‖ challenge ‖ X; both the spend-auth signatures (rk) and the binding signature are over this sighash. The proof itself is the ordinary Orchard Action proof over the same anchor.
Check: Verifier::verify_proof_of_funds(record, amount, challenge), read-only: value_balance == X (X > 0) → anchor known → nullifiers not in the spent set and pairwise distinct → verify_proof(vk) → spend-auth sigs under the PoF sighash → binding sig under the PoF sighash. Mutates nothing.
Domain separation claim: no pool operation computes that personalization, so the same record POSTed as an unshield/transfer fails InvalidSpendAuth (asserted in the test; nothing applied).
Test (release, laptop): prove 1.3 s, verify 10 ms, 2 actions, ≈ 18.6 KB JSON; holder with 150 proves 120 (reveals X, not the total); wrong challenge → InvalidSpendAuth; wrong X → ValueBalance; cannot build above balance; a keyless verifier replayed from the public log reaches the same verdict; after the proven notes are spent the old proof fails with DoubleSpend.

== Owner's questions ==
Q-POF-1 Is the domain separation sufficient? The bundle (proof, nullifiers, rk, cv_net) is a valid Orchard bundle under another sighash; we rely on spend-auth (needs ask) and binding (needs Σ rcv) signatures being unforgeable under the pool sighash.
Q-POF-2 Range argument for "≥ X": value balance = Σ v_old − Σ v_new over u64 note values with ≤ a few actions, no wrap. Anything in Orchard's ValueSum / cv_net handling that breaks this?
Q-POF-3 Privacy: the verifier learns the nullifiers, so the later spend of those notes is linkable to the holder. Proposed mitigation: the wallet rotates the notes to itself right after proving. Better ideas?
Q-POF-4 Transferability: a proof shows that someone holding ≥ X answered the challenge, not that this someone is the borrower. Binding the challenge to the borrower's XRPL account signature narrows but does not close it. Acceptable for an underwriting signal, or a blocker?
Q-POF-5 Snapshot vs lock: funds can move right after. A pool-level lock would need the quorum to enforce encumbrance (trust path). Out of scope for the event?

== My read (dev session) ==
(a) Separation: the pool's verify tail recomputes sighash(commitment, op, external_ctx(op, external, vb)) with the pool personalization; the PoF record carries op = Unshield and external = "", so a POST /submit/unshield recomputes a different message → both signature checks fail → refused before apply. Fine as long as no future pool op ever adopts the PoF personalization or a ctx that collides (the ctx has a length prefix, so challenge ‖ X is unambiguous). I would make the lender's challenge include the lender identity, the borrower's XRPL account and a validated ledger index / expiry, so a proof is replayable only within a window, and have the lender re-verify at decision time.
(b) Range: Orchard constrains each note value to 64 bits in-circuit and ValueSum is i128-wide at the bundle level; with ≤ ~15 actions Σ cannot wrap; the binding signature ties Σ cv_net to value_balance = X; non-negative outputs give Σ inputs ≥ X. I see no hole; the one thing to pin is that the verifier rejects value_balance ≠ X as i64 exactly (it does).
(c) Linkage: inherent to revealing nullifiers; the self-rotation mitigation turns it into "one later self-transfer is the holder's", which the public log already shows as an anchor; acceptable for a demo with the label. A circuit-level PoF (no nullifier reveal) is out of the reuse-Orchard-unchanged scope.
(d) Transferability: an underwriting SIGNAL, not an identity proof — must be labeled; binding the challenge to the borrower's XRPL signature + the Credential being issued to that same account is the practical narrowing.
(e) Snapshot: lender re-verifies at decision time; any lock is a quorum/trust-path change → out of scope for the event, say so.
(f) Hygiene: the record must never be sent to a coordinator by the UI (the honest-label line: "a proof of funds is not an operation"); and this is core/ — if option C is chosen, the on-site implementation gets the usual slice (coverage to 100 %, mutation table on the five checks, wasm export, REQ).

Nothing to merge now; a DESIGN-PASS / conditions / FAIL on the approach is what the owner wants before Oct 24. I'll post your verdict on PR #29 and record REQ/RESP under docs/audit when it comes.
```
