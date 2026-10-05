# REQ — v2 core slice: Wallet/Verifier split, deposit binding, anchor memos, endorse — 2026-10-05

**From:** the Rust-core dev session. **To:** the audit session. **Asks for:** IMPL-PASS for merging
branch `v2-core-wallet-verifier` (crate `core/`) to `main`. Design of record:
`REQ/RESP-v2-trust-minimized-2026-10-05.md` (DESIGN-PASS, conditional). All conditions are in this
slice; the harness (SignerListSet, coordinator, scanner) is the next PR.

## 1. What was built

| Module | Enforces / provides |
|---|---|
| `wallet.rs` — `Wallet` | spending key; `sync(log)` re-verifies every new record into a private replica (`Verifier`), finds own notes by trial decryption, marks spent ones, refuses a log that is not an append-only extension (`LogRewound`); `build_shield / build_transfer / build_unshield` → `Prepared { record, hash }`. Never touches XRPL. |
| `verifier.rs` — `Verifier` | public state only: tree, nullifier set, **consumed-deposit set**, bundle log, anchored root. `submit_shield(record, DepositRef)`, `submit_transfer`, `submit_unshield(record, destination)`; `replay` (log path); **`endorse(Submission, proposed AnchorMemo)`** (signer path); `anchor_memo`, `check_memo`, `rebuild(memos, mirror)`; `reveal`; key-free `snapshot/restore`. |
| `record.rs` | record ⇄ bundle; **canonical bytes** and **`bundle_hash` = BLAKE2b-256("ConfidantBundle")** over anchor ‖ flags ‖ value balance ‖ every action's (nf, rk, cmx, cv_net, epk, enc, out, spend-auth sig) ‖ proof ‖ binding sig — no metadata. |
| `memo.rs` — `AnchorMemo` | `version ‖ op ‖ root ‖ bundleHash ‖ extLen ‖ ext ‖ n ‖ n × (nf ‖ cmx)`; encode refuses > 1 KB, > 255 actions, ext > 255 B; decode hard-rejects an unknown version, unknown op, truncation, trailing bytes, non-UTF-8 ext. |
| `tree.rs` | shardtree + a cloneable `Frontier` mirror; `root_after(leaves)` predicts a batch's root without mutating (what `endorse` compares to the proposal). |
| `sighash.rs` | shield external context is now empty (binding moved to the deposit memo); transfer empty; unshield `destination ‖ amount-from-value_balance`. `OpTag::from_byte`. |
| `types.rs` | `WalletInfo`, `DepositRef {txHash, delivered, memoHash}`, `Accepted {…, bundleHash, payout?}`, `Submission` (tagged by `op`), `PoolState` (+ `consumedDeposits`). |
| `examples/cli.rs` | test tool: `cli wallet --file *.wallet.json …` (0600, loud warning) and `cli verifier --state *.verifier-state.json …` incl. `endorse`, `anchor-memo`, `check-memo`, `rebuild`. |

### Verify-and-apply paths (every one passes the same checks before any mutation)

`verify` (pure): sign per op → vouched amount == value balance → anchor ∈ history → nullifiers
unspent & unique → proof → spend-auth sigs → binding sig. `check_deposit_state` (pure, shields):
a deposit is named and is not in the consumed set. `apply_verified` is the only mutation and is
reached only after both.

| Path | Pre-checks | Then |
|---|---|---|
| `submit_shield` | `check_deposit_binding` (memo hash == bundle hash) | `accept` = `check_deposit_state` → `verify(Some(delivered))` → `apply_verified` |
| `submit_transfer` / `submit_unshield` | destination non-empty | `accept` |
| `replay` (restore, wallet sync) | record index == next | `accept` with the record's op / external |
| `rebuild` (new node) | mirror bundle hash == memo hash | `accept` with the memo's op / external; root and actions must equal the memo's |
| `endorse` (signer) | proposed memo's op / external / bundle hash / actions == submission; shield binding | `check_deposit_state` → `verify` → **`root_after(leaves) == proposed.root`** → `apply_verified` |

There is no path that applies effects, marks a deposit consumed or releases value without passing
`check_deposit_state` and `verify` first; a refusal anywhere leaves the verifier unchanged (audit
IMPL-verify condition on the hollow-check removal: confirmed by construction and by mutation D2,
which goes red on the shield-specific tests only).

## 2. Suite → requirement map (`core/tests/verifier.rs`, `e2e_demo.rs`, unit tests)

| Requirement | Test |
|---|---|
| v1 guards carried over (double-spend, replay, unknown anchor, tampered value balance, swapped binding sig, corrupted proof, re-pointed unshield, cross-op tag, other circuit vk, bad requests before proving, old-anchor spend, reveal scope) | `double_spend_is_refused_and_state_is_unchanged`, `replay_of_an_accepted_bundle_is_refused`, `replay_is_refused_before_the_proof_is_checked`, `unknown_anchor_is_refused`, `tampered_value_balance_is_refused`, `swapped_binding_signature_is_refused`, `corrupted_proof_is_refused`, `repointed_unshield_is_refused_then_original_is_accepted`, `bundle_presented_as_another_op_is_refused`, `proof_does_not_verify_under_another_circuit_version`, `wallet_refuses_bad_requests_before_proving`, `spend_witnessed_at_an_older_anchor_is_still_accepted`, `end_to_end_shield_transfer_unshield_reveal` |
| v2 §7.1–4 deposit binding: same deposit twice, memo ≠ hash, delivered ≠ −vb, stranger's bundle vs. someone's deposit then rightful accepted | `deposit_binding_refuses_wrong_memo_amount_and_reuse`; dup inside a log on restore: `verifier_snapshot_restore_keeps_nullifiers_and_consumed_deposits` |
| Q-V2-2 hash covers bundle, not metadata | `bundle_hash_covers_the_bundle_and_not_the_metadata` |
| v2 §7.5–6 memo round-trip, ≤ 1 KB, version hard-reject, rebuild same root, wrong mirror bundle / wrong memo root / wrong actions / wrong external refused | `anchor_memos_round_trip_and_rebuild_the_verifier`, `anchor_memo_codec_rejects_malformed_bytes_and_oversize` |
| v2 §7.7 wallet sync equals operator view; append-only; wallet without log cannot build | `wallet_sync_is_append_only_and_tracks_spent_notes`, `wallet_refuses_bad_requests_before_proving` |
| v2 §7.8 / Q-V2-5 signers: endorse equals submit on agreement; wrong root / hash / external / op / actions refused with state unchanged; re-pointed payout refused; a signer missing a prior bundle refuses, then agrees after catching up; shield binding / consumed through endorse | `signer_endorses_only_a_root_it_computed_itself`; two verifiers on one log agree: `e2e_demo` |
| record round-trip, malformed fields, canonical bytes refuse malformed hex | `record_roundtrip_is_exact_and_malformed_records_are_refused` |
| key-free snapshot, restore corruption (format, vb, leaf index, order, missing deposit, dup deposit, unknown anchored root) | `verifier_snapshot_restore_keeps_nullifiers_and_consumed_deposits` |
| whole demo at the core boundary with two signers and a rebuilding node | `e2e_demo::demo_script_end_to_end` |

## 3. Mutation results

(see table below — each guard removed in turn, suite re-run, guard restored; baseline green
before and after)

| Guard mutated | Suite | Tests that went red |
|---|---|---|
| D1 check_deposit_state: deposit named | RED ✔ | deposit_binding_refuses_wrong_memo_amount_and_reuse, verifier_snapshot_restore_keeps_nullifiers_and_consumed_deposits |
| D2 check_deposit_state: deposit unconsumed (the single remaining check; shield path) | RED ✔ | deposit_binding_refuses_wrong_memo_amount_and_reuse, signer_endorses_only_a_root_it_computed_itself, verifier_snapshot_restore_keeps_nullifiers_and_consumed_deposits |
| D3 check_deposit_binding: memo hash == bundle hash | RED ✔ | deposit_binding_refuses_wrong_memo_amount_and_reuse, signer_endorses_only_a_root_it_computed_itself |
| D4 submit_shield: delivered vouched into verify | RED ✔ | deposit_binding_refuses_wrong_memo_amount_and_reuse |
| D5 apply: mark deposit consumed | RED ✔ | anchor_memos_round_trip_and_rebuild_the_verifier, deposit_binding_refuses_wrong_memo_amount_and_reuse, end_to_end_shield_transfer_unshield_reveal, signer_endorses_only_a_root_it_computed_itself, verifier_snapshot_restore_keeps_nullifiers_and_consumed_deposits |
| E1 endorse: proposed op | RED ✔ | signer_endorses_only_a_root_it_computed_itself |
| E2 endorse: proposed external | RED ✔ | signer_endorses_only_a_root_it_computed_itself |
| E3 endorse: proposed bundle hash | RED ✔ | signer_endorses_only_a_root_it_computed_itself |
| E4 endorse: proposed actions | RED ✔ | signer_endorses_only_a_root_it_computed_itself |
| E5 endorse: own-state deposit check | RED ✔ | signer_endorses_only_a_root_it_computed_itself |
| E6 endorse: own tail (verify) before signing | RED ✔ | signer_endorses_only_a_root_it_computed_itself |
| E7 endorse: ROOT computed on own tree == proposed (no blind signing) | RED ✔ | signer_endorses_only_a_root_it_computed_itself |
| M1 check_memo: root | RED ✔ | anchor_memos_round_trip_and_rebuild_the_verifier |
| M2 check_memo: bundle hash | RED ✔ | anchor_memos_round_trip_and_rebuild_the_verifier |
| R1 rebuild: mirror bundle hash | RED ✔ | anchor_memos_round_trip_and_rebuild_the_verifier |
| R2 rebuild: root == memo root | RED ✔ | anchor_memos_round_trip_and_rebuild_the_verifier |
| R3 rebuild: actions == memo actions | RED ✔ | anchor_memos_round_trip_and_rebuild_the_verifier |
| R4 replay: record order | RED ✔ | replay_of_an_accepted_bundle_is_refused, verifier_snapshot_restore_keeps_nullifiers_and_consumed_deposits |
| W1 wallet sync: log rewound | RED ✔ | wallet_sync_is_append_only_and_tracks_spent_notes |
| W2 wallet sync: mark spent | RED ✔ | end_to_end_shield_transfer_unshield_reveal, spend_witnessed_at_an_older_anchor_is_still_accepted, wallet_sync_is_append_only_and_tracks_spent_notes |
| C1 memo encode: 1 KB bound | RED ✔ | anchor_memo_codec_rejects_malformed_bytes_and_oversize |
| C2 memo decode: trailing bytes | RED ✔ | anchor_memo_codec_rejects_malformed_bytes_and_oversize |
| C3 memo decode: version hard-reject | RED ✔ | anchor_memo_codec_rejects_malformed_bytes_and_oversize |

| J1 rebuild_checked: deposit facts required for every shield | RED ✔ | anchor_memos_round_trip_and_rebuild_the_verifier |
| J2 rebuild_checked: lookup answers for the memo's tx hash | RED ✔ | anchor_memos_round_trip_and_rebuild_the_verifier |
| J3 rebuild_checked: memo == bundle hash (deposit binding) | RED ✔ | anchor_memos_round_trip_and_rebuild_the_verifier |
| J4 rebuild_checked: delivered == −valueBalance | RED ✔ | anchor_memos_round_trip_and_rebuild_the_verifier |

Baseline green before and after every mutation (27 guards after the IMPL-PASS condition landed in-core: D1–D5 deposit binding, E1–E7 endorse, M1–M2 check_memo, R1–R4 rebuild/replay, W1–W2 wallet sync, C1–C3 memo codec). Earlier run: the duplicate early consumed-deposit check in `submit_shield` was found HOLLOW (same refusal from `accept`) and removed; the single remaining check (D2) is red on the shield-specific tests.

## 4. Coverage and profiles

`cargo llvm-cov --workspace --all-targets --fail-under-lines 100`: lines 100%, functions 100%
(regions ≈ 97%, the rest being `Err` arms of `?` on library calls that cannot fail under the
pool's invariants). `cargo test --release` (CI's profile) green.

## 5. Questions

- **Q-V2I-1** Anything in the `endorse` order (memo-vs-submission equality → own deposit state → own
  tail → own root → apply) you want changed?
- **Q-V2I-2** (answered after the ruling, in this slice): `Verifier::rebuild_checked` now enforces
  backing on join inside the core from the XRPL side's ledger facts (J1–J4 above); `rebuild`
  (crypto-only, backing deferred to the k-quorum) remains for non-signing nodes and is labeled so.
  The harness join path uses `rebuild_checked` for the new node and the lagging signer and asserts
  that wrong or missing deposit facts refuse the join.
