# Confidant — Roadmap and status

**As of 2026-10-05 (night), `main` after PR #12.** Hackathon: AI & Fintech XRP Ledger Hackathon, NYC,
**Oct 24–25, 2026**. Testnet-first. Design of record: [`SPEC.md`](./SPEC.md) / [`SPEC.ru.md`](./SPEC.ru.md).

Roles (names never, roles only): the **crypto/ZK lead** owns the Rust core (`core/`), the quorum
services (`backend/`: verifier node, signer sidecars, coordinator, setup/launch scripts), the CLI test
tool and the e2e harnesses; the **API/XRPL lead** (repo owner) owns the user-facing layer: local
wallet UX, the deposit Payment from the user's account, the UI over the coordinator, pre-population,
the demo and the pitch. (Reassigned 2026-10-05 with the owner's go: the quorum machinery is the
security-critical part and a direct extension of `endorse`/`rebuild_checked`.) The seam between the
two — the coordinator's HTTP contract, what the UI must produce, how to run the backend — is
`docs/API-BOUNDARY.md` (RU: `docs/API-BOUNDARY.ru.md`).

Judging rule: *only what is built during the hackathon counts.* The core and the harness are
pre-event preparation on a pre-existing library (the Zcash Orchard circuit). The judged work is the
XRPL integration built on-site. Say so on stage.

---

## 1. Where we are — one screen

| Area | Status | Where |
|---|---|---|
| **v2 trust-minimized architecture** (client-side keys/proving, state on ledger, k-of-N SignerList, permissionless relaying) | ✅ core + harness (PR #8); ✅ quorum services (PR #11); ✅ deployed on a testnet host; ⬜ user-facing layer — §5 | `SPEC.md` §4–5, §16; `docs/audit/`; `docs/API-BOUNDARY.md` |
| Reused circuit validated (M0 gate) | ✅ Zcash Orchard Action circuit unchanged, `orchard` 0.15.5 / `orchard_v2` / `FixedPostNu6_2` | `core/src/verifier.rs` (`BUNDLE_VERSION`) |
| Core: `Wallet` + `Verifier`, deposit binding, anchor memos, `rebuild_checked`, `endorse`, reveal, key-free snapshots | ✅ | `core/` |
| Tests | ✅ core: 29 tests, 27 guards mutation-tested, **100% line coverage, enforced in CI**; node: 8 tests against a fake ledger, 25 guards mutation-tested, 95% gate (≈ 98.7%); sidecar guard: node:test suite, 20 guards mutation-tested | `core/tests/`, `backend/node/tests/`, `backend/quorum/test/`, CI `coverage` + `quorum` jobs |
| CI | ✅ fmt / clippy `-D warnings` / tests (release) / coverage gates (core 100%, node 95%) / sidecar unit tests / hygiene (no key-shaped files, label markers in SPEC/README/SPEC.ru, allow-listed over-claim grep) / manual testnet e2e (both harnesses) | `.github/workflows/check.yml` |
| Security review | ✅ v1 DESIGN-PASS + IMPL-PASS (+2 slice PASSes); v2 core DESIGN-PASS + IMPL-PASS; quorum services DESIGN-PASS (C1–C5, all implemented) + IMPL-PASS; deployment manifests passed a security-delta look | `docs/audit/INDEX.md` |
| Honest labels (v2 wording) | ✅ SPEC, SPEC.ru, README, crate docs | — |
| XRPL testnet e2e | ✅ CLI-driven (`run.mjs`): 2-of-3 SignerList, deposit binding, lying coordinator refused, new node rebuilt with deposit re-validation; ✅ through the services (`run-services.mjs`): coordinator flow, sidecar refusals (memo, payout, fee, sequence, LLS, order), custody gate, payout blob, new node `--rebuild`; ✅ smoke client against the deployed quorum | `e2e/testnet/`, `backend/quorum/smoke.mjs`, `docs/E2E.md` |
| Core ↔ API contract, node-operator guide, **UI ↔ backend boundary** | ✅ | `docs/CORE-API.md`, `docs/NODE-OPERATOR.md`, `docs/API-BOUNDARY.md` (+ RU) |
| **Quorum services**: Rust verifier node (`/submit/*`, `/endorse` idempotent, ledger-verified `/anchored`, `/log`, custody gate + scanner + `rebuild_checked` on start), Node signer sidecars (sign only what their own node endorsed; Fee/Sequence/LastLedgerSequence pinned from their own ledger reads), coordinator (k-of-N multisig, anchor then payout, payout blob returned) | ✅ built, audited and merged 2026-10-05 (PR #11); ✅ **deployed** the same night on a testnet host as systemd units behind nginx (`backend/deploy/`), 3 signers / quorum 2, smoke cycle green over the public address | `backend/`, `backend/deploy/README.md`, `docs/E2E.md` §2b |
| Front-end: local wallet UX, deposit memo from the user's account, UI over the coordinator, explorer links; pre-population script; demo + pitch | ⬜ not started — the contract and a working reference client are ready (`docs/API-BOUNDARY.md`, `backend/quorum/smoke.mjs`) | `frontend/`, `scripts/` (API/XRPL lead) |

---

## 2. Milestones (from SPEC) — core + harness vs. API/XRPL/UI

| Milestone | Core + harness | API / XRPL / UI |
|---|---|---|
| **M0 — pre-event prep (not judged)** | ✅ circuit proves/verifies (params 2.5 s, 2-action bundle ≈ 0.9 s, verify ≈ 10 ms); scaffold; design + impl audited | ✅ verifier node over `Verifier`; ✅ testnet pool with 2-of-3 SignerList, master disabled, deployed with its quorum |
| **M1 — shield** | ✅ `Wallet::build_shield` → deposit memo = bundle hash → `Verifier::submit_shield(record, DepositRef)` | ✅ nodes validate the deposit on-ledger themselves; `POST /submit/shield`; `GET /log` · ⬜ UI: the deposit Payment from the user's account |
| **M2 — private transfer** | ✅ `Wallet::build_transfer` → `Verifier::submit_transfer`; nothing for the ledger but the anchor | ✅ `POST /submit/transfer` · ⬜ UI |
| **M3 — anchor** | ✅ `anchor_memo` per op, `check_memo`, `rebuild` / `rebuild_checked`; harness writes and reads memos on testnet | ✅ multisigned memo Payment after each op (coordinator + sidecars); ledger-verified `/anchored`; scanner on node start |
| **M4 — unshield** | ✅ `Wallet::build_unshield` → `Verifier::submit_unshield` → `payout`; harness pays 2-of-3 | ✅ `POST /submit/unshield` + multisigned payout, payout blob returned · ⬜ UI |
| **M5 — disclosure + demo** | ✅ `Verifier::reveal(viewingKey)` | ✅ `POST /audit/reveal` · ⬜ UI drives shield → transfer → unshield → reveal on the real explorer; rehearsed 45-second script |

---

## 3. Done — detail

### Core (`core/`, crate `confidant-core`)
- **Circuit:** Zcash Orchard Action circuit reused unchanged; keys built from the circuit at process
  start. Every op is one Orchard bundle, padded to ≥ 2 actions.
- **`Wallet`:** spending key; own notes found by trial-decrypting the public log; replica tree for
  witnesses; `build_shield / build_transfer / build_unshield` → `Prepared { record, hash }`.
- **`Verifier`:** tree (shardtree + cloneable `Frontier` mirror), nullifier set, consumed-deposit set,
  public bundle log, anchored root. Verify tail: value-balance sign per op → vouched amount ==
  value balance → anchor ∈ history → nullifiers → proof → spend-auth sigs → binding sig; shields
  additionally need a named, unconsumed deposit; `apply_verified` is the only mutation.
- **Deposit binding:** the deposit memo carries the bundle hash (`BLAKE2b-256("ConfidantBundle")`);
  `delivered == −valueBalance`, memo == hash, one shield per deposit.
- **Anchor memo** per op (≤ 1 KB): version ‖ op ‖ root ‖ bundle hash ‖ external ‖ n × (nullifier ‖
  cmx); unknown version/op, truncation and trailing bytes hard-reject.
- **`endorse`:** a signer checks the coordinator's memo against the submission and its own state,
  predicts the root on its own tree, applies only on agreement — never signs a handed root.
- **`rebuild` / `rebuild_checked`:** join from memos + mirror; the checked form enforces backing for
  every shield from the XRPL side's deposit facts.
- **Reveal:** the holder's full viewing key → receipts, change, spent status, outgoing recipients.
- **Persistence:** `WalletSecret` (spending key, 0600, gitignored), `VerifierSnapshot` (public).

### Quorum services (`backend/`, PR #11, deployed)
- **Verifier node** (`confidant-node`, Rust/axum): HTTP over `Verifier`; reads the ledger by JSON-RPC
  only, holds no keys. Looks every shield's deposit up itself (submit and endorse), verifies
  `/anchored` against the anchor transaction on the ledger, refuses to start unless the pool's
  on-ledger SignerList/quorum/master-key state is the configured one, rebuilds from `account_tx`
  (scan must start at the pool's creation) + a mirror with `rebuild_checked`.
- **Signer sidecars** (Node, xrpl.js): the only key holders; sign an anchor/payout only after their
  own node endorsed and the transaction's shape matches — Fee ≤ ceiling, Sequence = the pool's next
  as read by the sidecar itself, LastLedgerSequence present and bounded.
- **Coordinator** (Node): holds nothing; ≥ k signatures, anchor first, then payout; returns the
  multisigned payout blob so a failed payout is recoverable by anyone (or by a replacement coordinator).
- **Deployment** (`backend/deploy/`): systemd units per node/sidecar/coordinator under a dedicated
  user, nginx with CORS on one public port, env renderer from `config.json`; smoke client
  `backend/quorum/smoke.mjs`. Running on a testnet host since 2026-10-05 (details handed over out of band).

### Quality gates
- 29 core tests; negative suite: double-spend, replay (before the proof), unknown anchor, tampered value
  balance, swapped binding signature, corrupted proof, re-pointed unshield, cross-op tag, other
  circuit version, bad requests before proving, old-anchor spend, deposit binding (stranger's bundle,
  wrong amount, reuse, duplicate in a log), bundle hash without metadata, memo codec, rebuild
  (wrong mirror / root / actions / external / deposit facts), wallet sync, signer endorse (wrong
  root / hash / external / op / actions, re-pointed payout, lagging signer), snapshot restore.
- 27 core guards mutation-tested (remove → red); tables in `docs/audit/REQ-v2-core-impl-2026-10-05.md`.
- 100% line coverage of the core, CI-enforced; regions ≈ 97% (unreachable `Err` arms of `?` on library calls).
- Node: 8 tests against a fake ledger, 25 guards mutation-tested, 95% line gate (≈ 98.7%); sidecar
  guard: node:test suite in CI, 20 guards mutation-tested; services e2e on testnet green
  (`docs/audit/REQ-quorum-services-2026-10-05.md` §Supplement).

### Review record (`docs/audit/`)
- v1: `REQ/RESP-pool-design-2026-10-04` (DESIGN-PASS, C1–C5), `REQ/RESP-pool-core-impl-2026-10-04`
  (IMPL-PASS; slices PR #2, PR #3).
- v2: `REQ/RESP-v2-trust-minimized-2026-10-05` (DESIGN-PASS, conditions met),
  `REQ/RESP-v2-core-impl-2026-10-05` (IMPL-PASS; backing on join in-core; release assert).
- Quorum services: `REQ/RESP-quorum-services-2026-10-05` (DESIGN-PASS with C1–C5, all implemented;
  IMPL-PASS; deployment-manifests pass).

---

## 4. To do — crypto/ZK lead (core + quorum services)

1. **Integration support.** Adjust endpoints/response fields as the UI needs them (additive changes
   same day; trust-path changes via an audit slice) — `docs/API-BOUNDARY.md` §6.
2. **Run the deployed quorum through the hackathon:** update procedure (sync the tree, replace the
   binaries, `systemctl restart confidant.target`), watch the journal, rehearse the restart path on
   stage; help pre-populate the pool when the owner's script exists.
3. **Keep the docs in sync** with every change (SPEC, SPEC.ru, CORE-API, E2E, API-BOUNDARY, this file).
4. **Roadmap items from the audit (not for the hackathon):** pool-instance id in the sighash
   personalization; `external_ctx` taking the unshield amount as `u64`; bounded anchor history;
   XLS-56 Batch for atomic anchor + payout; FROST for the signer keys.
5. **Roadmap items from the "Shielded Bitcoin" paper** ([alloc] init, 2026-09,
   https://www.allocinit.xyz/uploads/shielded-bitcoin.pdf — the same shape as our v2: the ledger
   only orders envelopes, indexers replay them, custody lives only at the entry/exit boundary).
   **Principle (decided 2026-10-08): no migration of the proving system.** The Orchard circuit stays
   reused unchanged — audited, in production in Zcash since 2022, Halo 2 with no trusted setup — and
   every item below lives in the memo / verifier / wallet layers on top of it (item 4 is an add-on
   proof, not a replacement). Not for the hackathon unless marked; in order of value:
   1. **Full data availability on the ledger.** Today `rebuild` takes the anchor memo from the
      ledger and the bundle bytes (proof included) from a mirror, checked by hash. XRPL fees are
      ~10 drops per transaction, so the whole bundle (a few KB) can be published as ≤ 1 KB memo
      chunks across several Payments for a fraction of a cent, making any node rebuildable from
      the ledger alone with no mirror (the paper's assumption A2). Design: `confidant/bundle` memo
      type with `bundle hash ‖ chunk index ‖ count ‖ bytes`, anchored by the existing bundle hash;
      `rebuild_checked` reassembles and hash-checks before use.
   2. **Bounded anchor window** (the audit's "bounded anchor history", now with a concrete rule).
      Record the root per validated ledger index; accept a bundle's anchor only inside
      `L − W ≤ ledger(anchor) ≤ L − K_min` (paper profile W = 100 blocks ≈ 16 h, K_min = 1; for
      XRPL's ~4 s ledgers choose W in ledgers to match the intended publication window). Bounds
      verifier memory and makes anchor age a public, uniform value instead of a fingerprint.
   3. **Selective disclosure beyond the full viewing key.** Add a note-opening report (value,
      `rho`, `rseed`, recipient of one note; the auditor recomputes `cmx` against the log) and a
      derived report (inflows / outflows / balance at a log height with the supporting openings),
      so a holder can disclose one payment or one period instead of their whole history. The full
      viewing key stays the broad option. Core-only, small; a candidate for the demo if time allows
      ("disclose one payment, not the history").
   4. **Certified-recipient lineage** (paper Appendix D): an opt-in, encrypted, recursively-proved
      claim carried inside a note that every ancestor note descends from an authority-certified
      deposit and every transfer along the way went to a certified address; dummy payload of the
      same length otherwise, so the chain cannot tell. A stronger form of the Proof-of-Innocence
      item for the compliant-pool story; needs a lineage circuit and an attestation format — a
      separate design + audit cycle.
   Not adopted: proving ciphertext/plaintext consistency in-circuit (their ciphertext-derived note
   leaf; fixes the "sender burns a recipient's output with a garbage ciphertext" limitation we
   inherit from Orchard) — requires a different circuit, out of our reuse-Orchard-unchanged scope.

## 5. To do — API/XRPL lead (user-facing layer, pitch)

In demo order. The contract, the flows and how to run things: `docs/API-BOUNDARY.md` (RU:
`docs/API-BOUNDARY.ru.md`); a complete working client: `backend/quorum/smoke.mjs`. The quorum is
already deployed (base URL and pool account handed over out of band).

1. **Front-end:** a local wallet process (`Wallet` behind localhost) or the CLI during the hackathon;
   the deposit Payment with memo = bundle hash from the user's XRPL account; shield / transfer /
   unshield / reveal buttons calling the coordinator; explorer links; the reveal JSON; the
   viewing-key warning (`docs/API-BOUNDARY.md` §2).
2. **Pre-populate the pool** before the demo (many testnet accounts shield fixed denominations, some
   transfer) so the anonymity set is believable; decide the denominations.
3. **Demo script + pitch** (SPEC §10): rehearse the 45 seconds with the v2 wording (non-custodial
   notes, quorum can drain the pool account, internal graph hidden but not the endpoints, ZK first,
   TEE a footnote). Bonus only if time: Proof-of-Innocence, FROST.

## 5a. v2 — trust-minimized architecture (decided 2026-10-05, core + harness done the same day)

| Step | What | Status |
|---|---|---|
| **1. Keys and proving on the client** | `Wallet` / `Verifier` split; the API takes a ready bundle (`senderAuth` gone); deposit binding via the memo hash + consumed set | ✅ core (PR #8) · ✅ harness · ✅ node `/submit/*` · ⬜ FE wallet UX |
| **2. State on the ledger** | anchor memo per op; proofs mirrored by hash; `rebuild` / `rebuild_checked` | ✅ core codec + rebuild · ✅ node scanner (`--rebuild --mirror`) |
| **3. k-of-N verifiers (SignerList)** | SignerList quorum, master disabled; every signer `endorse`s on its own state; coordinator assembles the multisig; anchor first, payout second | ✅ core `endorse` · ✅ node `/endorse` + signer sidecars + coordinator (`backend/`) · ✅ 2-of-3 on testnet · ✅ deployed |
| **4. Permissionless relaying** | any verifier accepts; `GET /log` on every node; node-operator guide | ✅ core/CLI · ✅ node `GET /log` · ✅ `docs/NODE-OPERATOR.md` |

Honest label after v2: notes non-custodial; pool account and verification federated (k-of-N); state
reconstructible from the ledger; **collusion of k signers can drain the pool account but cannot spend
anyone's notes** — the real gap vs. Tornado/Railgun, kept prominent and dated (k-of-N because today's
XRPL has no on-ledger Halo 2 verifier; an amendment adding one upgrades this). Signers never
blind-sign (`Verifier::endorse`). Upside vs. Railgun: Halo 2 has no trusted setup.

## 6. Decisions still open

- **Denominations:** fixed set (recommended) or free amounts — the owner's call with pre-population.
- Decided 2026-10-05: **token = XRP** for the hackathon (the core stays token-agnostic); **anchoring
  per op**; **the quorum runs on one testnet host** as systemd units behind nginx, 3 signers / quorum
  2, state files on that host, every node able to rebuild from the ledger.

## 7. Risks and the honest line

- **Federated custody.** Collusion of k signers can drain the pool account. Say it. On-ledger
  verification (the amendment) is the roadmap, never a demo claim.
- **Endpoint correlation.** The pool hides the internal graph, not that someone shielded X and later
  someone unshielded X. Pre-populate; prefer fixed denominations; add delay.
- **Empty pool at launch = no privacy** for the first operations.
- **Liveness** depends on k signers; on stage that is three sidecars + their nodes under systemd with
  `Restart=on-failure`. The restart path is exercised (state files restored, custody gate re-checked);
  a cold rebuild from the ledger (`--rebuild --mirror`) is tested in the services harness.
- **Proving cost is fine** (≈ 0.9 s per 2-action bundle on the wallet side).

## 8. Working agreements

- Branch + PR for everything; `main` only via PR with green CI. Core and backend trust-path changes
  get an audit slice ruling before merge; the ruling is committed verbatim under `docs/audit/`.
- Never commit keys, wallet files, `data/` or `.env`; CI's hygiene job checks. Never put real names in
  git.
- Keep `SPEC.md`, `SPEC.ru.md`, `README.md` and `docs/` in sync with the code in the same PR (CI
  checks the label markers).
