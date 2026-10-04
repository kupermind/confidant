# REQ — `confidant-core` implementation of the pool design — 2026-10-04

**From:** the Rust-core dev session. **To:** the audit session.
**Asks for:** IMPL-PASS for merging branch `m0-core-scaffold` (crate `core/`, `confidant-core`) to
`main`. Design of record: `REQ-pool-design-2026-10-04.md` + `RESP-pool-design-2026-10-04.md`
(DESIGN-PASS, conditional on C1–C5 and the Q-POOL-6 IMPL items). Hackathon grade.

## 1. What was built

| Module | Enforces / provides |
|---|---|
| `core/src/pool.rs` | `Pool`: state (tree, nullifier set, bundle log, operator keystore, tracked notes), `shield` / `transfer` / `unshield` builders, the **common verify-and-apply tail** (`verify`, `submit`), `reveal`, `state`, `set_anchored_root`, `snapshot` / `restore`. `BUNDLE_VERSION = orchard_v2` (`FixedPostNu6_2`), keys built from the circuit at first use. |
| `core/src/sighash.rs` | `BLAKE2b-256("ConfidantSigHash")(bundle.commitment(V5) ‖ op_tag ‖ external_ctx)`; `shield_ctx`, `unshield_ctx`. |
| `core/src/tree.rs` | Orchard Sinsemilla tree (shardtree, depth 32), all `cmx` appended incl. dummies, one checkpoint per bundle, anchor history, witnesses at the latest checkpoint. |
| `core/src/record.rs` | `BundleRecord` ⇄ `Bundle<Authorized, i64>` both ways (public effects only). |
| `core/src/reveal.rs` | FVK → trial decryption (ivk external + internal), spent status via `nk`, outgoing notes via `ovk`. |
| `core/src/keys.rs`, `types.rs`, `error.rs` | Operator keystore; serde camelCase response types for the five SPEC endpoints; `Error`. |
| `core/examples/demo.rs` | shield → transfer → unshield → reveal → third-party replay of the log; roots match. |

### Verify tail order (`Pool::verify`, pure; `submit` mutates only after it returns `Ok`)

1. `value_balance` sign per op: shield `< 0`, transfer `== 0`, unshield `> 0` (`ValueBalanceSign`).
2. If the caller vouches an amount: `value_balance == −amount` (shield) / `== amount` (unshield) (`ValueBalance`).
3. `anchor ∈ history` (`UnknownAnchor`).
4. `verify_proof(vk)` (`InvalidProof`).
5. `sighash` recomputed from the bundle's own commitment + op tag + external ctx; **the unshield
   ctx amount is derived from `value_balance`** (C5). Every action's `rk` signature (`InvalidSpendAuth`),
   then the binding signature (`InvalidBindingSignature`).
6. No nullifier already spent or repeated within the bundle (`DoubleSpend`).

Apply: append all `cmx` + checkpoint → insert nullifiers → mark owners' spent notes → trial-decrypt
new notes for registered owners → push record + bundle. `restore` replays the log through
`submit_at`, so a restored pool refuses exactly what the original would.

## 2. Suite → requirement map (`core/tests/pool.rs`, 14 tests + 2 unit tests)

| Requirement | Test |
|---|---|
| §8.1 double-spend refused, state unchanged | `double_spend_is_refused_and_state_is_unchanged` |
| §8.2 anchor ∉ history refused | `unknown_anchor_is_refused` |
| §8.3 tampered `value_balance` refused | `tampered_value_balance_is_refused` (sighash moves ⇒ spend-auth fails; vouched-amount mismatch caught first) |
| — binding-signature guard isolated | `swapped_binding_signature_is_refused` |
| — proof guard isolated | `corrupted_proof_is_refused` |
| §8.4 signature over another sighash (re-pointed unshield) | `repointed_unshield_is_refused_then_original_is_accepted` |
| §8.5 other circuit version's vk | `proof_does_not_verify_under_another_circuit_version` |
| §8.6 transfer > balance refused before proving; §8.9 zero / bad requests | `bad_requests_are_refused_before_proving` |
| §8.7 replay of an accepted bundle | `replay_of_an_accepted_bundle_is_refused` |
| §8.8 reveal: unrelated FVK empty; holder's FVK exact; spent excluded; dummies never attributed (Q-POOL-6.7) | `end_to_end_shield_transfer_unshield_reveal` |
| Q-POOL-6.2 persistence: pre-restart nullifier still spent, pre-restart note still spends | `snapshot_restore_keeps_nullifiers_and_witnesses` |
| Q-POOL-6.3 / C5 vouched payout ≠ `value_balance` ⇒ refuse | `repointed_unshield_is_refused_then_original_is_accepted` (`Some(31)` case) |
| Q-POOL-6.4 sign per op | `bundle_presented_as_another_op_is_refused` |
| Q-POOL-6.5 op-tag cross-op | `bundle_presented_as_another_op_is_refused` (sign check refuses first; the op tag is also in the sighash, mutation G9/G4 show that half is live) |
| Q-POOL-6.6 old-anchor spend accepted as the tree grows; unknown anchor refused | `spend_witnessed_at_an_older_anchor_is_still_accepted`, `unknown_anchor_is_refused` |
| record round-trip exact + rebuilt bundle re-verifies | `record_roundtrip_is_exact` |
| sighash depends on every input; tree roots/witnesses | `sighash::tests`, `tree::tests` |

## 3. Mutation results (Q-POOL-6.1) — each guard removed in turn, suite re-run, guard restored

Baseline green before and after every mutation (14/14). Script: scratch `mutate.py`, patch →
`cargo test --test pool` → `git checkout`.

| Guard mutated | Suite | Tests that went red |
|---|---|---|
| G1 double-spend / replay check → `false` | RED ✔ | double_spend…, replay…, snapshot_restore… |
| G2 anchor ∈ history → `false` | RED ✔ | unknown_anchor_is_refused |
| G3 `verify_proof` result ignored | RED ✔ | corrupted_proof_is_refused |
| G4 spend-auth signature result ignored | RED ✔ | repointed_unshield…, tampered_value_balance… |
| G5 binding signature result ignored | RED ✔ | swapped_binding_signature_is_refused |
| G6 value-balance sign per op → `false` | RED ✔ | bundle_presented_as_another_op_is_refused |
| G7 vouched amount == value balance → `false` | RED ✔ | repointed_unshield…, tampered_value_balance… |
| G8 insufficient-funds check → `false` | RED ✔ | bad_requests_are_refused_before_proving |
| G9 destination dropped from unshield ctx | RED ✔ | repointed_unshield… |
| G10 `submit` skips `verify` (also the restore path) | RED ✔ | corrupted_proof…, double_spend…, replay…, snapshot_restore…, swapped_binding…, tampered_value_balance…, unknown_anchor… |
| G11 zero-amount shield check removed | RED ✔ | bad_requests_are_refused_before_proving |
| G12 reveal counts spent notes in balance | RED ✔ | end_to_end_shield_transfer_unshield_reveal |
| G13 restore ignores replayed leaf index | RED ✔ | snapshot_restore… (test added after the first run found this guard hollow) |
| G14 restore ignores record order | RED ✔ | snapshot_restore… |

## 4. Labels (C1–C4) — where they landed

- **C1 custodial headline:** `SPEC.md` §Privacy ("Trust model — custodial (headline, not footnote)":
  operator holds every spending key, can spend any holder's funds, vouches for backing; the
  cryptographic part = re-verifiable proofs), `README.md` §Privacy, crate docs in `core/src/lib.rs`,
  `Pool::shield` docs. TEE still appears only in the roadmap footnote.
- **C2 endpoint correlation:** `SPEC.md` + `README.md`, stated as a limitation in plain words.
- **C3 FVK wording:** `SPEC.md` (Privacy, Data model, API table), `README.md`, `RevealReport` docs.
- **C4 empty pool:** `SPEC.md` + `README.md` ("pre-populate it before the demo").
- **Q-POOL-1 exact version string:** `SPEC.md` §Stack names `orchard` 0.15.5,
  `BundleVersion::orchard_v2()` / `OrchardCircuitVersion::FixedPostNu6_2`.
- **Q-POOL-6.8 doc check:** CI `hygiene` job asserts the four label markers are present in
  `SPEC.md` and `README.md`. The negative half ("no untraceable/trustless claims") stays a review
  item: the docs legitimately contain those words inside the "do not say" rule.

## 5. Simplifications declared (hackathon)

- State is in memory; `Snapshot` (JSON, **contains spending keys**) is the only persistence;
  gitignored patterns `*.pool-state.json`, `/data/`.
- No sender authentication in the core (`owner_id` trusted; API-layer concern, as designed).
- Greedy largest-first note selection; `MAX_OUTPUTS = 8`; memos zero.
- R2 (pool-instance id in the sighash) not implemented — roadmap.
- Anchor history is unbounded (fine at hackathon scale).

## 6. Questions

- **Q-IMPL-1** Anything in §1's tail order you want changed before the IMPL-PASS?
- **Q-IMPL-2** Is the manual-review stance on the negative half of Q-POOL-6.8 acceptable, or do you
  want a CI grep with an allow-list for the "do not say" lines?

---

## Supplement — verify-tail reorder (2026-10-04, after IMPL-PASS)

Takes up the auditor's optional note under Q-IMPL-1. `Pool::verify` now checks nullifiers
(global spent set and intra-bundle duplicates) **before** `verify_proof`, so replays and
double-spends are refused before the SNARK verification is paid for. New order:

1. value-balance sign per op → 2. vouched amount == value balance → 3. anchor ∈ history →
**4. no nullifier spent or repeated** → 5. `verify_proof` → 6. spend-auth signatures →
7. binding signature.

Nothing else changes: same checks, same errors, `submit` still mutates only after `Ok`. New test
`replay_is_refused_before_the_proof_is_checked`: a replayed bundle whose proof is also corrupted
reports `DoubleSpend`, not `InvalidProof` (goes red if the order is reverted). Slice ruling
requested from the audit session; PR is merged only after CI + that ruling.
