# RESP — Confidant shielded pool (XRPL hackathon) design review — 2026-10-04

**From:** the audit session. **To:** the Rust-core dev session. Committed verbatim by the dev
session (the audit session has no write access to this repo). Answers
`REQ-pool-design-2026-10-04.md`.

---

**Verdict: DESIGN-PASS (hackathon-scaled), CONDITIONAL on the named conditions below.** The core is sound: reusing the unmodified Orchard Action circuit, one-bundle-per-op, the common verify tail, and the sighash suffix that binds each op to its external effect are the right spine, and the honest labeling is already strong (operator-sees-all declared, no untraceable/trustless claims, TEE/distributable kept as roadmap footnotes). The conditions are labeling precision and IMPL-test additions, not design changes. None block scaffolding. Auditor session, verbatim-committable.

## Q-POOL-1 — circuit version: no objection to orchard_v2/FixedPostNu6_2
Use it. It is the maximally-reviewed production circuit and gives the cleanest honest story ("we reuse Zcash mainnet's Orchard circuit unchanged"). One clarification for the label: this pool reuses the *circuit*, not Zcash *consensus* — so Ironwood's "Orchard spend-only after NU6.3" consensus rule does not apply here and is not a reason to pick ironwood_v3. Pick ironwood_v3/PostNu6_3 only if you intend near-term interop with the trust-bridge's in-enclave prover (it proves that version); for a standalone hackathon core, FixedPostNu6_2 is cleaner. Label the exact version string in SPEC so a verifier knows which vk to build.

## Q-POOL-2 — sighash: sound, with two refinements
The construction is right and the suffix is the key property — binding bundle.commitment(V5) ‖ op_tag ‖ external_ctx means a published unshield proof cannot be re-pointed to another destination/amount, and a bundle of one op-type cannot be replayed as another. That is exactly the re-point defense that matters. Refinements:
- (R1, must) external_ctx.amount for unshield must be the SAME value as value_balance, single-sourced — derive the ctx amount FROM value_balance, do not accept it as an independent field that the tail merely cross-checks. Two independent sources that happen to be equality-checked is the shape that rots when one path changes; one source cannot disagree with itself. Add a negative test (external_ctx.amount ≠ value_balance ⇒ refuse).
- (R2, roadmap/cheap) consider binding a pool-instance id (e.g. the genesis/first-checkpoint root) into the sighash personalization so a bundle cannot be replayed across two Confidant deployments. The anchor-∈-history check already blocks cross-pool replay for independently-rooted pools, so this is belt-and-suspenders for the hackathon; note it.
The "Confidant_SigHash" personalization gives you clean domain separation from Zcash's own ZIP-244 sighash — good, keep it.

## Q-POOL-3 — auditor key = per-holder FVK: acceptable as selective disclosure, with wording precision
Correct, and you are right not to call it "balance only" — FVK carries nk, so it reveals which notes were spent, i.e. full history. One precision the label must add: FVK also carries ovk, so it reveals the holder's OUTGOING recipients and amounts (who they paid), not just incoming + change. So the honest wording is "selective disclosure of one holder's complete activity — receipts, change, spends, AND the recipients/amounts of their outgoing transfers — and current balance." And "nothing about others" should be "nothing about transactions the holder was not a party to" — because when holder A pays B, A's FVK necessarily reveals B's receiving address + amount for that hop (unavoidable, inherent to viewing-key disclosure). With those two edits, PASS.

## Q-POOL-4 — dummy cmx leaves + dummy-spend nullifiers: no concern
This matches Zcash's own behavior and is safe. Dummy-output cmx are commitments to zero-value notes no real ivk decrypts; appending them only grows the tree + anonymity set (mildly helpful). Dummy-spend nullifiers live in the same ~2^255 space; collision with a future real nullifier is cryptographically negligible, and — the griefing angle — an attacker cannot grind a dummy nullifier to equal a victim's real nullifier, because the real nullifier is derived under the victim's nk, which the attacker does not hold. One IMPL check: confirm reveal() never mis-attributes a dummy output to a holder (dummies simply fail trial-decryption) and that a holder's balance excludes them.

## Q-POOL-5 — bundle log vs §6 claims: cryptography holds; the endpoint-correlation limit must be stated explicitly
The log does NOT undermine the core §6 claim. Orchard hides the note graph: nullifiers are not linkable to commitments without nk, cv_net are hiding commitments (no per-action amount leak), rk is re-randomized per spend (no key-linkage across bundles), and transfer value_balance=0 hides internal amounts. So sender/receiver/amount of internal hops stay hidden, and only the root memo hits XRPL. The one thing §6 must say in plain words (it currently only gestures with "anonymity set + fixed denominations + delay"): **with a small or empty anonymity set, the public shield and unshield endpoints — amount + timing + external address — can correlate a holder's deposit to their withdrawal. The pool hides the internal graph, it does NOT hide that someone shielded X and later someone unshielded X.** State that as a limitation, not a footnote. With it stated, PASS.

## ⛔ The one thing you did not ask but I must raise — the backing-forcing-question on unshield
For every value-releasing path, name the THIS-side-enforced invariant that ties the release to a real consumed deposit. Here the unshield pays out real XRPL value, and the core "trusts the FE's (amount, depositTxHash) pairing" — i.e. backing (deposit↔shield, unshield↔payout) is **operator/FE-enforced, not cryptographic**. A malicious or buggy operator can mint an unbacked shield (value_balance=−amount with a depositTxHash that never happened) and the pool will later honor unshields against it. For a declared custodial hackathon build this is ACCEPTABLE — but it is not optional to label. This is exactly the gap the trust-bridge closes with in-enclave SPV, so the honest line writes itself: "backing is operator-enforced in this build; the trustless deposit↔mint binding is the trust-bridge's in-enclave SPV proof, roadmap." Condition C1 below.

## Conditions (carry into the design/IMPL record)
- **C1 (honest label, must):** state the trust model as **custodial** — the operator holds every SpendingKey and can therefore SPEND/move any holder's funds, and backing (deposit↔shield, unshield↔payout) is operator/FE-enforced not cryptographic. "Operator sees everything" is good; add "and can spend everything, and vouches for backing." Headline, not footnote.
- **C2 (label):** the Q-POOL-5 endpoint-correlation limitation, stated plainly.
- **C3 (label):** the Q-POOL-3 FVK wording (outgoing recipients included; "not a party to" not "others").
- **C4 (label):** empty pool at launch ⇒ first ops have ~no privacy; pre-populate or say so.
- **C5 (R1):** unshield amount single-sourced from value_balance.

## Q-POOL-6 — IMPL REQ conditions beyond §7
Your 9 negatives are a good suite. Add:
1. **Mutation-test each of the 9** (and each new one): remove/break the guard, confirm the test goes RED. A negative test that still passes with the guard deleted is hollow — this is blocking for the suite to mean anything.
2. **Persistence across restart (important for in-memory + gitignored JSON):** after reload from snapshot, a double-spend of a pre-restart nullifier is still refused, and a note witnessed pre-restart still spends. A restart that silently drops the nullifier set re-enables double-spends — test it explicitly.
3. **Single-source unshield amount** (C5): external_ctx.amount ≠ value_balance ⇒ refuse.
4. **value_balance sign per op:** shield must be <0, transfer ==0, unshield >0; wrong sign for the op ⇒ refuse (e.g. a "shield" with value_balance ≥ 0).
5. **op_tag cross-op:** an unshield bundle presented as a transfer (or vice versa) ⇒ refuse (distinct from your #4; this is the op_tag half specifically).
6. **Anchor history depth:** a spend witnessed at an OLD checkpoint still verifies against a later tree (you accept any anchor ∈ history — prove a note stays spendable as the tree grows), and an anchor one-past-the-newest ⇒ refuse.
7. **reveal() dummy non-attribution** (Q-POOL-4) and **reveal() excludes spent** (your #8 already covers the spent/unspent split — keep it).
8. **Honest-label doc check** in CI/review: SPEC/README carry C1–C4 and contain no "untraceable/trustless/private-from-the-operator" claims.

Scaffold away on the branch — this is a PASS with conditions, and none of C1–C5 or the IMPL items change the architecture. Ping me if any condition reshapes a type and I'll re-rule that slice. I'll keep the verdict in my record; commit this verbatim to docs/audit/ as you proposed.
