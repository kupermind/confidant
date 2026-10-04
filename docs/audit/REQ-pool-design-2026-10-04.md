# REQ — Confidant shielded pool on reused Orchard circuits (design) — 2026-10-04

**From:** the Rust-core dev session. **To:** the audit session.
**Grade:** hackathon (AI & Fintech XRPL Hackathon, Oct 24–25, 2026). Simplifications are allowed,
**honest labeling is not negotiable** (repo `CLAUDE.md`). Asking for a **DESIGN-PASS** (or CONDITIONAL
with named conditions) before the core crate lands on `main`.

## 1. Scope of this REQ

The XRPL-agnostic Rust core of Confidant (`SPEC.md`): commitment tree, nullifier set, proving and
verifying of shield / private transfer / unshield, and the auditor reveal. **Out of scope here:** the
XRPL side (Payments, memo anchoring, custody of the pool account) and the HTTP/UI layer, both owned by
the repo owner (XRPL/front-end lead).

## 2. The reused circuit (SPEC's "critical dependency")

- Crate `orchard = 0.15.5` (zcash/orchard), features `circuit`, `multicore`, `std`. The Zcash Orchard
  **Action circuit** (Halo 2 / Pasta) is reused unchanged; proving and verifying keys are **built from
  the circuit at process start** (`ProvingKey::build`, no serialized key artifact to trust).
- Bundle version `BundleVersion::orchard_v2()` → circuit `OrchardCircuitVersion::FixedPostNu6_2`
  (the Orchard pool as deployed on Zcash since NU6.2), V2 note plaintexts, `OrchardDomain` note
  encryption. Flags: spends + outputs + cross-address enabled (`default_flags()`); the shield step uses
  `Flags::SPENDS_DISABLED` (output-only bundle, dummy spends).
- `BundleType::DEFAULT` (transactional, padded to ≥ 2 actions). Every op is one Orchard **bundle**.

What the circuit + bundle enforce per action (the SPEC's "transfer/unshield proof"):
membership of the spent commitment under the bundle anchor (Sinsemilla Merkle tree, depth 32);
knowledge of the spending key (`rk` = randomized `ak`, spend-auth signature over the sighash);
correct nullifier derivation (`nf = DeriveNullifier_nk(rho, psi, cm)`); correct output note
commitment; value commitments `cv_net` such that the **binding signature** ties Σ`cv_net` to the
public `value_balance`. Hidden: values, senders, recipients of internal hops.

## 3. Pool state (backend, in-memory + JSON snapshot; gitignored)

- `tree`: `shardtree::ShardTree<MemoryShardStore<MerkleHashOrchard,u32>, 32, 16>` — all `cmx` of
  every accepted bundle are appended **including dummy outputs**, as Zcash does, so the tree is
  exactly the bundle log's leaves. A checkpoint per accepted bundle; the set of checkpoint roots is the
  **anchor history**. A bundle is accepted only against an anchor in that history.
- `nullifiers`: set of every `nf` of every accepted bundle (dummy spends included — they are random
  and harmless). Duplicate `nf` ⇒ refuse (double-spend).
- `bundles`: append-only log of accepted bundles' **public effects** — anchor, flags, `value_balance`,
  per action `(nf, rk, cmx, cv_net, encrypted_note)`, proof bytes, spend-auth + binding signatures,
  op tag, external context. This is what anyone re-verifies against the anchored root.
- `keystore` (operator-held, see §6): `owner_id → SpendingKey` (random 32 bytes at registration).

## 4. Operations

Common tail for every op: `verify_proof(vk)`; verify every `rk` signature and the binding signature
over `sighash`; anchor ∈ history; every `nf` ∉ `nullifiers`; `value_balance` equals the expected
external amount for the op; then **atomically** insert nullifiers, append `cmx` leaves, checkpoint,
append to `bundles`. Any failure ⇒ nothing changes.

| Op | Builder | `value_balance` | External context in sighash |
|---|---|---|---|
| **shield**(owner, amount, depositTxHash) | 1 output to `owner`'s address, spends disabled | `−amount` | `depositTxHash` |
| **transfer**(owner, [(recipient_owner, amount)]) | greedy note selection from owner's unspent notes; spends with witnesses at the current anchor; outputs; change to owner's **internal**-scope address | `0` | none |
| **unshield**(owner, amount, destination) | spends + change; no external output | `+amount` | `destination ‖ amount` |

**Sighash** (the message the spend-auth and binding signatures sign):
`BLAKE2b-256(personal = "Confidant_SigHash")( bundle.commitment(TxVersion::V5) ‖ op_tag(1) ‖ external_ctx )`.
`bundle.commitment` already commits to anchor, flags, value balance and every action's public parts;
the suffix binds a shield to the XRPL deposit and an unshield to its XRPL payout, so a published
unshield proof cannot be re-pointed to a different destination/amount.

Who verifies the XRPL leg: the FE/API layer (owner's side) submits the deposit Payment and passes
`amount` + `depositTxHash` to the core; the core trusts that pairing (operator model, §6). Unshield:
the core returns `payout {amount, destination}` and the FE builds the pool→destination Payment.

## 5. Anchoring and re-verification

`GET /pool/state` → `root` (current checkpoint root, 32 bytes hex), `anchoredRoot` (last root the FE
wrote into a Payment memo), `leafCount`, `nullifierCount`. The bundle log (§3) is exportable so a
third party can replay: re-verify every proof/signature, rebuild the tree, and match the anchored root.
No on-ledger verification (SPEC non-goal; the amendment sketch is the roadmap).

## 6. Keys and trust model (the honest label)

- **Operator sees everything.** The backend holds every owner's `SpendingKey`, selects notes, proves and
  signs. This is the SPEC's declared trust model for the demo ("distributable / TEE later" is a
  roadmap footnote, never a demo claim). `senderAuth` (who may spend as `owner`) is an API-layer
  concern on the owner's side; the core takes an `owner_id` it trusts.
- **Auditor key = the holder's `FullViewingKey`** (96 bytes), handed to the auditor at onboarding.
  `reveal(fvk)` trial-decrypts every recorded action with `ivk(External)` and `ivk(Internal)`,
  derives each decrypted note's nullifier with `fvk.nk`, and reports unspent notes + balance. One key
  reveals exactly one holder — their incoming notes, their change, and which were spent — and
  nothing about anyone else. Claim wording: "selective disclosure of one holder's full history and
  balance", not "balance only" (an FVK sees spends too; an IVK alone cannot tell spent from unspent).
- Rejected for time: a global auditor key with per-note dual encryption. Listed as roadmap.

## 7. Privacy claims to be labeled

Hidden on ledger: sender, receiver, amount of every internal hop (nothing hits XRPL but a root memo).
Public and inherent: shield/unshield amounts and external XRPL addresses; the bundle log shows the
**number of actions per op** (padded ≥ 2) and timing. Real-world privacy = anonymity set (pool
pre-populated from many testnet accounts) + fixed denominations + delay. No "untraceable / trustless".

## 8. Negative suite I commit to (unit tests in the core crate)

1. Double-spend: the same note in two bundles ⇒ second refused, state unchanged.
2. Wrong anchor: a valid bundle built against an anchor not in history ⇒ refused.
3. Tampered `value_balance` ⇒ binding signature fails ⇒ refused.
4. Signature over a different sighash (e.g. unshield re-pointed to another destination) ⇒ refused.
5. Proof verified under a different circuit version's key ⇒ refused.
6. Transfer exceeding the owner's unspent balance ⇒ refused before proving.
7. Replay of an accepted bundle ⇒ refused (nullifiers).
8. `reveal` with an unrelated FVK ⇒ empty; with the holder's FVK ⇒ exactly their notes, spent ones
   excluded from the balance.
9. Shield without a positive amount / with zero outputs ⇒ refused.

## 9. Questions for the audit session

- **Q-POOL-1** Circuit/bundle version: `orchard_v2` (FixedPostNu6_2) as above, or `ironwood_v3`
  (PostNu6_3, V3 notes, the one trust-bridge proves in-enclave)? I prefer v2 ("the Orchard circuit
  on Zcash mainnet"); both permit cross-address outputs. Objections?
- **Q-POOL-2** The sighash construction in §4 (bundle commitment ‖ op tag ‖ external context, under
  a Confidant personalization). Sound? Anything else that must be bound?
- **Q-POOL-3** Auditor key = per-holder FVK (§6). Acceptable as "selective disclosure" with the
  wording given? Or do you require the auditor to receive something weaker/stronger?
- **Q-POOL-4** Appending dummy-output `cmx` leaves and dummy-spend nullifiers to the pool state —
  any concern (set growth, linkability)?
- **Q-POOL-5** Anything in the published bundle log (§3) that undermines the §7 claims?
- **Q-POOL-6** Conditions you want carried into the IMPL REQ beyond the §8 suite.

## 10. Process

The audit session has no write access to `kupermind/confidant`. I will commit your `RESP-*` text
verbatim into `docs/audit/`, attributed to "the audit session" (repo rule: roles, never names).
Verdict vocabulary as in the AUDIT-PROTOCOL: DESIGN-PASS / IMPL-PASS / CONDITIONAL / FAIL.

---

## Dev-side amendments after the RESP (2026-10-04)

- **Personalization string.** BLAKE2b personalization is at most 16 bytes; `"Confidant_SigHash"`
  is 17. Implemented as `"ConfidantSigHash"` (16 bytes). Same domain separation, one byte shorter.
- **C5 / R1 implemented as specified:** the verify tail derives the unshield context's amount from
  the bundle's own `value_balance`; the caller's vouched payout amount is only ever compared
  against `value_balance`, never bound into the sighash itself. Shield/unshield/transfer sign checks
  on `value_balance` are enforced before any other check.
- **R2 (pool-instance id in the sighash):** not implemented for the hackathon; recorded as roadmap.
