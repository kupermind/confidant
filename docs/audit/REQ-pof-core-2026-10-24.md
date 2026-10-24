# REQ — proof of shielded funds (hackathon feature C): the core slice

Sent 2026-10-24 (event day) from the implementation session to the audit session, verbatim (branch
`pof-core`). Built on-site from the reviewed design (`docs/PROOF-OF-FUNDS.md`, DESIGN-PASS conditions 2–4 of
`RESP-pof-design-2026-10-14.md`; condition 1 landed in PR #31). The pre-event spike (PR #29, not merged) was
the reference; this slice is the implementation.

```text
REQ (impl, core, no apply-path change in effect — a new op is refused everywhere): proof of shielded funds
with the Orchard circuit unchanged.

== Design ==
OpTag::ProofOfFunds = 4 (sighash.rs): the record type condition 2 asked for. A proof-of-funds bundle is built
  like an unshield of X with the change back to the holder and signed over pof_sighash = BLAKE2b-256
  (personal "ConfidantPoF_v1_")(bundle_commitment ‖ pof_ctx), pof_ctx = len(challenge) ‖ challenge ‖ X.
  No pool operation computes that domain; sighash() for the pool includes the op byte, so the preimages
  differ even for the same bytes. The domain and the context function are added to the allow-list test
  (apply_path_guards.rs) on purpose; the test pins the set.
Refused before any signature, on every apply path:
  - Verifier::pool_bundle(record) (the decode step every apply path uses: submit_shield, endorse_submission,
    endorse, submit_transfer, submit_unshield, replay) refuses op == ProofOfFunds → Error::NotAPoolOperation;
  - Verifier::verify(bundle, op, …) refuses op == ProofOfFunds first (the public API, for callers that decode
    themselves) — the sign check and both matches stay total (ProofOfFunds grouped with Unshield, unreachable);
  - AnchorMemo::from_record refuses a ProofOfFunds record and AnchorMemo::decode refuses op byte 4 →
    Error::MemoMalformed("not a pool operation"), so endorse and a rebuild from the ledger stop early;
  - external_ctx(ProofOfFunds) is total (empty) and never reached: verify() refuses first.
Verifier::verify_proof_of_funds(record, amount, challenge) → read-only, mutates nothing: op must be
  ProofOfFunds (else Error::NotAProofOfFunds); value balance == amount and amount > 0; known anchor;
  nullifiers unspent and distinct (before the SNARK, as in verify()); proof; every spend authorization and the
  binding signature under pof_sighash. Errors: ValueBalance, UnknownAnchor, DoubleSpend, InvalidProof,
  InvalidSpendAuth, InvalidBindingSignature.
Wallet::build_proof_of_funds(amount, challenge): ZeroAmount for 0; InsufficientFunds beyond the balance;
  build_spending gains an Option<&[u8]> challenge that selects the proof-of-funds sighash.
Honest reading (SPEC, PROOF-OF-FUNDS): "someone controlling ≥ X answered this challenge" as of this state —
  an underwriting signal, a snapshot, not a lock, not proof that a named account owns the notes. The binding
  of borrower account / ledger index / expiry / lender id is the challenge's job (Tier 1, next slice: the
  node endpoint recomputes the challenge and never logs the record).

== Tests ==
core/tests/proof_of_funds.rs: build (op tag, reveals X not the balance, 0 and > balance refused); verify ok and
  state unchanged; wrong challenge (different length AND same length with one byte flipped) → InvalidSpendAuth;
  wrong / zero amount → ValueBalance; direction 1: submit_unshield, submit_transfer, replay → NotAPoolOperation
  (never a signature error), endorse with a matching proposed memo → MemoMalformed, submit_shield refused,
  from_record and decode(op byte 4) refused, verify(bundle, ProofOfFunds) refused, state unchanged after all;
  direction 2: an unshield record and a shield record → NotAProofOfFunds; tampering: proof → InvalidProof,
  spend auth → InvalidSpendAuth, binding → InvalidBindingSignature, duplicated action → DoubleSpend;
  keyless replay reaches the same verdict; fresh verifier → UnknownAnchor; a second holder proves exactly his
  balance; after the proven notes move → DoubleSpend (snapshot, not lock).
sighash.rs unit tests: from_byte(4) and (5), external_ctx(ProofOfFunds) total, pof_ctx layout, pof_sighash
  differs by amount, challenge, commitment and from every pool sighash.
Coverage: cargo llvm-cov -p confidant-core --all-targets --fail-under-lines 100 → 100.00 % (1302/1302 lines).
cargo clippy --workspace --all-targets -D warnings clean; all core tests green (release).

== Mutation table ==
Run alone against proof_of_funds (M6: apply_path_guards); files restored after each row. 12/12 killed.
M1  pool_bundle: op refusal removed (apply paths)                          → killed
M2  verify_proof_of_funds: NotAProofOfFunds check removed                  → killed
M3  memo decode: proof-of-funds op byte accepted                           → killed
M4  memo from_record: proof-of-funds record accepted                       → killed
M5  verify(): early op refusal removed                                     → killed
M6  pof_sighash under the pool personalization                             → killed (allow-list: shared domain)
M7  pof_ctx: challenge bytes dropped (length kept)                         → killed (same-length wrong challenge)
M8  verify_proof_of_funds: nullifier check removed                         → killed
M9  verify_proof_of_funds: anchor check removed                            → killed
M10 verify_proof_of_funds: binding signature not checked                   → killed
M11 verify_proof_of_funds: spend authorizations not checked                → killed
M12 verify_proof_of_funds: proof not checked                               → killed
(M7 first survived with a different-length wrong challenge only; the test was strengthened, then it was killed.)

== Asked ==
IMPL-PASS for the core slice under conditions 2–4; a view on amount == value balance exactly (the holder
chooses X; "≥ X" is what it proves about the balance, not the bundle); confirm pool_bundle + verify() +
memo refusals are the complete set of apply-path entries.
```

## Supplement — the dev session's firsthand code map (sent 2026-10-24 with the REQ, verbatim excerpt)

```text
- Decode sites: bundle_from_record is called at exactly three places in verifier.rs — :306 inside pool_bundle (after the op refusal), :323 inside verify_proof_of_funds, and :648 inside rebuild_inner. Apply paths: submit_shield :458, endorse :517 (after AnchorMemo::from_record :511 refuses a PoF record first → MemoMalformed), submit_transfer :533, submit_unshield :547, replay :560 (restore → replay) all go through pool_bundle. verify() :226 refuses op == ProofOfFunds before the sign check.
- ONE GAP vs the owner's "complete set" claim: rebuild_inner (:648) decodes the mirror's record directly, not through pool_bundle. What protects it: the anchor memo is decoded first and AnchorMemo::decode refuses op byte 4, so a PoF MEMO cannot drive a rebuild; a PoF RECORD substituted in the mirror under a genuine pool memo (op 1–3) is then decoded and reaches accept → verify(bundle, memo.op, …) → refused by signature under the pool sighash (InvalidSpendAuth; the PoF was signed under the PoF domain), state unchanged — i.e. fail-closed, but refused by SIGNATURE DOMAIN, not by TYPE, which is exactly what condition 2 wanted avoided on every path. Fix is one line (use v.pool_bundle(&record) at :648) plus a test row (PoF record in the mirror under a pool memo → NotAPoolOperation).
```

The fix and the two test rows (the forged-memo rebuild row; a value-balance-zero record relabelled as a
proof of funds refused by amount) landed on the branch the same day; the audit's RESP records the ruling.
