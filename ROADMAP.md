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
| Tests | ✅ core: 29 tests, 27 guards mutation-tested, **100% line coverage, enforced in CI**; node: 10 tests against a fake ledger, 25 + fee guards mutation-tested, 95% gate (≈ 98%); sidecar guard: node:test suite (14 tests incl. `fee-sweep`), 20 guards mutation-tested | `core/tests/`, `backend/node/tests/`, `backend/quorum/test/`, CI `coverage` + `quorum` jobs |
| CI | ✅ fmt / clippy `-D warnings` / tests (release) / coverage gates (core 100%, node 95%) / sidecar unit tests / hygiene (no key-shaped files, label markers in SPEC/README/SPEC.ru, allow-listed over-claim grep) / manual testnet e2e (both harnesses) | `.github/workflows/check.yml` |
| Security review | ✅ v1 DESIGN-PASS + IMPL-PASS (+2 slice PASSes); v2 core DESIGN-PASS + IMPL-PASS; quorum services DESIGN-PASS (C1–C5, all implemented) + IMPL-PASS; deployment manifests passed a security-delta look | `docs/audit/INDEX.md` |
| Honest labels (v2 wording) | ✅ SPEC, SPEC.ru, README, crate docs | — |
| XRPL testnet e2e | ✅ CLI-driven (`run.mjs`): 2-of-3 SignerList, deposit binding, lying coordinator refused, new node rebuilt with deposit re-validation; ✅ through the services (`run-services.mjs`): coordinator flow, sidecar refusals (memo, payout, fee, sequence, LLS, order), custody gate, payout blob, new node `--rebuild`; ✅ smoke client against the deployed quorum | `e2e/testnet/`, `backend/quorum/smoke.mjs`, `docs/E2E.md` |
| Core ↔ API contract, node-operator guide, **UI ↔ backend boundary** | ✅ | `docs/CORE-API.md`, `docs/NODE-OPERATOR.md`, `docs/API-BOUNDARY.md` (+ RU) |
| **Quorum services**: Rust verifier node (`/submit/*`, `/endorse` idempotent, ledger-verified `/anchored`, `/log`, custody gate + scanner + `rebuild_checked` on start), Node signer sidecars (sign only what their own node endorsed; Fee/Sequence/LastLedgerSequence pinned from their own ledger reads), coordinator (k-of-N multisig, anchor then payout, payout blob returned) | ✅ built, audited and merged 2026-10-05 (PR #11); ✅ **deployed** the same night on a testnet host as systemd units behind nginx (`backend/deploy/`), 3 signers / quorum 2, smoke cycle green over the public address | `backend/`, `backend/deploy/README.md`, `docs/E2E.md` §2b |
| **Boundary screening** (optional per-node policy): listed depositors / destinations and too-fresh deposits refused at submit and endorse (403); OFAC XRP list fetcher; replayable audit over the public log | ✅ built, audited (CONDITIONAL → IMPL-PASS, canonical addresses) and merged 2026-10-10 (PR #20); deployed on the testnet quorum with the OFAC XRP list and a 2-ledger standby | `backend/node/src/screening.rs`, `backend/quorum/screening/`, `SPEC.md` §6, `docs/audit/RESP-screening-gate-2026-10-10.md` |
| **Protocol fee** (Railgun's economic model, server-side policy): configurable bps on shield and unshield, transfers free; shield credited net (deposit bound with `delivered = gross − fee`), payout net; per-node fee ledger replayable from the ledger; k-of-N sweep to a treasury the sidecars verify against their own node | ✅ built 2026-10-11 (branch `protocol-fees`, audit slice requested); UI shows the fee and builds the net note | `backend/node/src/fees.rs`, `SPEC.md` §6–7, `docs/API-BOUNDARY.md` §2 |
| Front-end: UI over the coordinator, deposit from the user's XRPL account, explorer links | ✅ **prototype** 2026-10-10: `frontend/index.html` (static) with the wallet **in the browser** (`frontend/wasm`, core → WebAssembly in a worker; keys and proofs never leave the page) and the demo wallet service as fallback · ✅ 2026-10-13 **multithreaded proving in the browser** (`frontend/pkg-threads`, wasm-bindgen-rayon; page served with COOP/COEP; measured in headless Chrome on a 20-core laptop: proving keys 6.0 s and a 2-action proof 3.2 s on 16 threads vs 12.1 s / 16.2 s single-threaded on the same machine; whole shield → transfer → unshield → reveal cycle 75 s vs 127 s), proving key built in the background right after the wallet exists, thread count and proof time shown · ✅ `backend/quorum/prepopulate.mjs` · ✅ `docs/DEMO.md` run-sheet · ⬜ FE team: presentation and design; rehearsal timings on the demo laptop | `frontend/`, `backend/wallet/`, `backend/quorum/prepopulate.mjs`, `docs/DEMO.md` |

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
- **Protocol fee** (2026-10-11, Railgun/Tornado economics on the operator's terms): `--fee-shield-bps`
  / `--fee-unshield-bps` / `--fee-treasury` on every node (≤ 10 %, treasury mandatory when non-zero,
  never settable from the UI). Shield notes are credited net — the wallet builds `gross − fee`, the
  node binds the deposit with `delivered = gross − fee`; unshields pay out net and the sidecars pin
  the net amount; transfers are free. Each node keeps a fee ledger (`accrued` / `swept` /
  `available`, `GET /fees`) that `--rebuild` recomputes from the ledger; fees reach the treasury only
  by an explicit k-of-N sweep (`POST /fees/sweep` on the coordinator; sidecar kind `fee-sweep` checked
  against *its own* node's treasury and available amount; `POST /fees/swept` verified on the ledger,
  once per hash). `setup-pool.mjs` funds the treasury (`FEE_SHIELD_BPS` / `FEE_UNSHIELD_BPS`, default
  25 bps); the deploy env renderer passes `FEE_ARGS` to every node.
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
   3a. **Refund path for refused deposits:** a quorum-signed Payment back to the depositor of a
   shield the gate refused (sidecar shape rule: destination = the deposit's sender, amount = the
   delivered amount, deposit unconsumed) — a small trust-path slice; manual today.
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
      `rebuild_checked` reassembles and hash-checks before use. *Audit condition:* availability
      only — backing stays "release bound to a verified consumed deposit", never "the bytes were
      published"; the moment rebuild-without-mirror gates a value path it is a trust-path slice.
   2. **Bounded anchor window** (the audit's "bounded anchor history", now with a concrete rule).
      Record the root per validated ledger index; accept a bundle's anchor only inside
      `L − W ≤ ledger(anchor) ≤ L − K_min` (paper profile W = 100 blocks ≈ 16 h, K_min = 1; for
      XRPL's ~4 s ledgers choose W in ledgers to match the intended publication window). Bounds
      verifier memory and makes anchor age a public, uniform value instead of a fingerprint.
      *Audit condition:* `K_min` must stay backing-grade (tied to the deposit path's finality, not
      to a liveness/privacy constant); `W` may prune history but never admit an anchor below
      backing-grade burial.
   3. **Selective disclosure beyond the full viewing key.** Add a note-opening report (value,
      `rho`, `rseed`, recipient of one note; the auditor recomputes `cmx` against the log) and a
      derived report (inflows / outflows / balance at a log height with the supporting openings),
      so a holder can disclose one payment or one period instead of their whole history. The full
      viewing key stays the broad option. Core-only, small; a candidate for the demo if time allows
      ("disclose one payment, not the history"). *Audit condition:* every opening/report
      authenticates who may open or derive it, bound to the recipient's identity, not to mere
      possession of a key or session.
   4. **Certified-recipient lineage / per-note proof of innocence** (paper Appendix D; Railgun
      PPOI assessed 2026-10-10): an opt-in, encrypted, recursively-proved claim carried inside a note
      that every ancestor note descends from a screened (or authority-certified) deposit and every
      transfer along the way went to a certified address; dummy payload of the same length
      otherwise, so the chain cannot tell. Railgun's PPOI is the reference design (per-list proofs,
      1 h unshield-only standby, inheritance through blinded commitments, proof once per shield);
      its node / list-provider / aggregator software (TypeScript, GPL-3.0, run as a separate
      service) and its status model are reusable, its proof is not — a circom/Groth16 circuit over
      Railgun's BN254/Poseidon note format, while Orchard notes live on Pallas with Sinsemilla —
      so this needs our own Halo 2 circuit with the Orchard gadgets, a lineage attestation format
      and a full design + audit cycle. **The cheap form is built** (boundary screening, §1): with
      the gate on since genesis, pool-wide innocence holds by construction and is replayable from
      the public log. This expensive form is for when there is nothing else to do (owner, 2026-10-10). *Audit condition:* the certification binds the identity
      computed from evidence, never a host-named one.
   Each item becomes a REQ + firsthand IMPL-PASS when built (audit session, 2026-10-08, recorded in
   `docs/audit/RESP-quorum-services-2026-10-05.md`). Not adopted: proving ciphertext/plaintext consistency in-circuit (their ciphertext-derived note
   leaf; fixes the "sender burns a recipient's output with a garbage ciphertext" limitation we
   inherit from Orchard) — requires a different circuit, out of our reuse-Orchard-unchanged scope.
6. **From Zakura's "Transparent PIR and address rotation"** (2026,
   https://zakura.com/engineering/transparent-pir-rotating-addresses/). The "lookup leak": a wallet
   that asks an RPC server for the history of addresses A, B, C tells that server they belong to
   one user even when the chain never links them. Their fix: rotate addresses on every use and
   fetch history by private information retrieval over sharded activity, with small public
   per-range filters (≈ 15 KB per 10,000 scripts) so the wallet only queries ranges that may
   contain its activity. What it means here (recorded 2026-10-09):
   1. **Keep wallet sync lookup-free — an invariant, not a feature.** A Confidant wallet syncs by
      downloading the whole public log (`GET /log?from=N`) and trial-decrypting locally, so no node
      ever learns which notes belong to whom. Never add a "which of these are mine" endpoint. When
      the log outgrows full download, the acceptable successors are per-range public filters + PIR
      (their design) or detection-key schemes — never per-note queries. (post-hackathon)
   2. **Rotate the XRPL endpoints.** Shield and unshield addresses are the pool's public endpoints
      (§7 endpoint correlation). The UI should default to a fresh XRPL destination per unshield
      (the harnesses already do) and encourage deposits from a fresh account, so chain analysis
      cannot cluster one user's boundary events — guidance added to `docs/API-BOUNDARY.md` §3.
      (cheap; hackathon)
   3. **The transparent lookup leak moves to the user's rippled.** A UI that checks balances or
      history of its rotating XRPL addresses at a public rippled reveals to that server that they
      belong to one client. Guidance: use the user's own or a trusted rippled; PIR over
      `account_tx` does not exist on XRPL today and would mean running a sharded directory + filters
      ourselves, as in the article. (post-hackathon, only if we operate such an index)
   4. **Post-quantum note, for honesty:** their hash-based (WOTS) spending escape hatch is specific
      to Zcash transparent scripts; XRPL has no such opcode and Orchard notes are not post-quantum
      either. Out of scope.
7. **Browser performance and demo readiness (taken over from the FE team 2026-10-11, so they can
   focus on presentation and design; built 2026-10-13, holder-side, auditor informed, no slice).**
   ✅ **Multithreaded proving in the browser**: `frontend/pkg-threads` = the same crate with the
   `threads` feature (`wasm-bindgen-rayon` over `orchard/multicore`), built by
   `frontend/wasm/build-threads.sh` (nightly, atomics, shared imported memory, TLS exports — the
   shared-memory linker flags are what make the thread pool work); the worker picks it when the page
   is cross-origin isolated (nginx site sends COOP/COEP; the xrpl.js CDN carries CORP) and falls back
   to `pkg/`. Measured (headless Chrome, 20-core laptop, 16 threads): proving keys 6.0 s, 2-action
   proof 3.2 s; single-threaded on the same machine 12.1 s / 16.2 s (≈ 25 s / ≈ 20 s on a busy one). ✅ **Proving key once per tab, in the background**:
   core builds `pk`/`vk` lazily (`Params::pk()`/`vk()`), so creating or restoring a wallet is instant
   and the worker's `warm()` builds the proving key while the user funds the account; the node warms
   the verifying key at startup. ⬜ *Cross-session* key caching is not possible yet: orchard 0.15 has
   no serialization for its `ProvingKey` (private fields over halo2's `ProvingKey`), so IndexedDB
   persistence would need an orchard patch — on 16 threads the 6 s rebuild per tab is acceptable.
   ✅ **UX for waits**: thread count in the wallet card, "proving keys ready (N threads, X s)",
   "proved in X s on N threads" on every success line, fee and net amount shown before and after.
   ⬜ Resume a failed payout from `payoutBlob` with one click (today: re-submit the same unshield;
   `/endorse` is idempotent). ✅ **Pool pre-population**: `backend/quorum/prepopulate.mjs`
   (`npm run prepopulate`: N faucet-funded holders shield fixed denominations through the coordinator,
   some transfer; prints the honest "synthetic set" label). ✅ **Demo run-sheet**: `docs/DEMO.md`
   (what to click, say and show for SPEC §10, fallbacks, timing table) — ⬜ rehearse and fill the
   timings on the demo laptop.

## 5. To do — API/XRPL lead (user-facing layer, pitch)

In demo order. The contract, the flows and how to run things: `docs/API-BOUNDARY.md` (RU:
`docs/API-BOUNDARY.ru.md`); a complete working client: `backend/quorum/smoke.mjs`. The quorum is
already deployed (base URL and pool account handed over out of band).

1. **Front-end:** a local wallet process (`Wallet` behind localhost) or the CLI during the hackathon;
   the deposit Payment with memo = bundle hash from the user's XRPL account; shield / transfer /
   unshield / reveal buttons calling the coordinator; explorer links; the reveal JSON; the
   viewing-key warning (`docs/API-BOUNDARY.md` §2).
2. **Pre-populate the pool** before the demo (many testnet accounts shield fixed denominations, some
   transfer) so the anonymity set is believable; decide the denominations. *Script moved to §4.7
   (crypto/ZK lead, 2026-10-11); the FE team decides the denominations and the look.*
3. **Demo script + pitch** (SPEC §10): rehearse the 45 seconds with the v2 wording (non-custodial
   notes, quorum can drain the pool account, internal graph hidden but not the endpoints, ZK first,
   TEE a footnote, the protocol fee named as an operating fee). Bonus only if time: Proof-of-Innocence,
   FROST. *Draft script and measured timings come from §4.7; the FE team owns presentation and design.*

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
- **Screening lists are public data and can be wrong or late.** A false positive blocks a
  legitimate depositor at the gate; their XRP sits in the pool account until the quorum refunds it
  (manual today, §4.3a). Say "gatekeeping", never "proof of innocence", for the cheap gate.
- **Liveness** depends on k signers; on stage that is three sidecars + their nodes under systemd with
  `Restart=on-failure`. The restart path is exercised (state files restored, custody gate re-checked);
  a cold rebuild from the ledger (`--rebuild --mirror`) is tested in the services harness.
- **Proving cost is fine** (≈ 0.9 s per 2-action bundle on the wallet side) — natively. In the browser
  it is ≈ 3 s on a 16-thread browser and 16–20 s single-threaded (§4.7).
- **The protocol fee is an operating fee, not a privacy feature.** Rates and the treasury are public;
  the treasury is an ordinary XRPL account under the same k-of-N as the pool, so the custody caveat
  covers it too. Different fee flags on different nodes split the quorum (roots diverge), and a rate change
  breaks `--rebuild` for earlier shields — one policy per pool, fixed at genesis (fee epochs in the
  memo would lift this; roadmap).

## 8. Working agreements

- Branch + PR for everything; `main` only via PR with green CI. Core and backend trust-path changes
  get an audit slice ruling before merge; the ruling is committed verbatim under `docs/audit/`.
- Never commit keys, wallet files, `data/` or `.env`; CI's hygiene job checks. Never put real names in
  git.
- Keep `SPEC.md`, `SPEC.ru.md`, `README.md` and `docs/` in sync with the code in the same PR (CI
  checks the label markers).
