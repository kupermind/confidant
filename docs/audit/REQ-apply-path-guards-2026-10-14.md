# REQ — apply-path signature safety + hash-domain guard (tests only, PR #31)

Sent 2026-10-14 from the implementation session to the audit session, verbatim. The audit's
"step (1)" for hackathon option C — the one piece it said could not be deferred — landed as tests
against the pool as it is; no library code changed.

```text
Owner's decision on option C: (c), as you advised — #29 stays open as the labeled design reference, #30 (the design REQ/RESP) is merged (20c9a9c7), C is built on-site, A before C. The one piece you said cannot be deferred — your step (1) — is done now as TESTS ONLY against the pool as it is (no library code changed): branch apply-path-guards, PR opened, CI running. Tests-only, so no slice by the letter, but it is core/ and it is your condition, so please take a firsthand look and say PASS or what to add; I merge after that + green CI.

== Test 1: both_signatures_are_re_verified_under_the_pool_sighash_at_every_apply_path (core/tests/apply_path_guards.rs) ==
Foreign-sighash bundle: built in the test with orchard's Builder from the wallet's own spending key (notes found by trial decryption of the public log, witnesses from Verifier::witness), over the verifier's current anchor, proven with the pool's proving key, value-balance sign correct for its shape — and SIGNED (spend-auth + binding, apply_signatures) over blake2b(personal = "ForeignDomain_01")(commitment ‖ ctx). Three shapes: unshield-like (vb = 40, external "rDest"), transfer-like (vb = 0), shield-like (vb = −25, dummy spends, with a matching DepositRef so deposit binding passes). Each is run through EVERY state-changing entrypoint and must fail with exactly InvalidSpendAuth, with the verifier byte-identical afterwards (fingerprint = state() + snapshot() JSON):
 - submit_shield / submit_transfer / submit_unshield → accept → verify → (only then) apply_verified;
 - endorse with Submission::Shield / Transfer / Unshield and a memo that describes the submission consistently (so the memo checks pass and the sig check is what refuses);
 - replay (record positioned as the next log entry), restore (snapshot + the record), rebuild and rebuild_checked (anchor memos + mirror + deposit lookup).
Swapped-binding variant: genuine wallet-built unshield / transfer / shield records with the binding signature taken from another genuine record → InvalidBindingSignature at the same entrypoints (spend-auth passes, binding is checked separately). Finally the honest records are accepted (the only difference was the signature domain).
Apply-path map (verifier.rs on main): verify() :226 — spend-auth loop :284, binding :289; accept() :305 calls check_deposit_state :317 → verify :314 → apply_verified :320 (nullifiers inserted :354, leaves appended just before); endorse() :403 → check_deposit_state :450 → verify :448 → apply_verified; submit_shield :382 / submit_transfer :462 / submit_unshield :469 / replay :483 → accept; rebuild_inner :555 → accept; restore :634 → replay. There is no path that reaches apply_verified without verify.

== Test 2: every_blake2b_personalization_and_sighash_context_in_core_is_enumerated_and_unique ==
Reads core/src/**/*.rs: every `blake2b_simd::Params::new()` site has a `.personal(CONST)` (counts equal); every CONST is a `pub const NAME: &[u8; N] = b"..."` defined in the same file; the set of literals == allow-list ["ConfidantBundle", "ConfidantSigHash"] (BTreeMap keys, exact); each personalization is used by exactly one call site; zero-padded-to-16 domains are distinct; the `pub fn *_ctx` set in sighash.rs == ["external_ctx", "unshield_ctx"]. Adding "ConfidantPoF_v1_" or pof_ctx on-site = extending the allow-list on purpose, with the test naming the event that breaks it.

== Mutations (alone, isolated target dir, restored from memory) ==
G1 spend-auth signatures not re-verified in the verify tail → killed; G2 binding signature not re-verified → killed; G3 two hash domains share one personalization (HASH_PERSONALIZATION := "ConfidantSigHash") → killed; G4 the bundle hash loses its .personal() → killed; G5 a new `pof_ctx` fn slipped into sighash.rs → killed. 5/5. clippy -D warnings clean; coverage unchanged (tests only). Docs: SPEC §11 EN/RU, ROADMAP §1/§3.

== One honest limit ==
The foreign bundle differs from a pool bundle only in the signature domain; the test cannot produce a bundle that passes the pool's signature check without the pool sighash (that would be the forgery the test guards against), so "every apply-path re-verifies before consuming" is shown by refusal + unchanged state at each entrypoint and by the call-graph map above, not by a positive oracle.
```
