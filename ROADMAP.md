# Confidant — Roadmap and status

**As of 2026-10-05, `main` @ `9c09890`.** Hackathon: AI & Fintech XRP Ledger Hackathon, NYC,
**Oct 24–25, 2026**. Testnet-first. Scope is fixed in [`SPEC.md`](./SPEC.md); this file tracks
what is done against it and what is left, per role.

Roles (names never, roles only): the **crypto/ZK lead** owns the Rust core (`core/`); the
**API/XRPL lead** (repo owner) owns the HTTP layer, everything XRPL, the UI and the pitch.

Judging rule to keep in mind: *only what is built during the hackathon counts.* The core below is
pre-event preparation on top of a pre-existing library (the Zcash Orchard circuit). The judged work
is the XRPL integration built on-site. Be honest about that split on stage.

---

## 1. Where we are — one screen

| Area | Status | Where |
|---|---|---|
| **v2 trust-minimized architecture** (client-side keys/proving, state on ledger, k-of-N SignerList, relaying) | 🟡 core + harness done 2026-10-05 (PR #8, testnet run green); backend integration left — §5a | `SPEC.md` §v2, `docs/audit/`, `docs/E2E.md` |
| Reused circuit validated (SPEC's "critical dependency", M0 gate) | ✅ done — Zcash Orchard Action circuit via `orchard` 0.15.5, `orchard_v2` / `FixedPostNu6_2`, unchanged | `core/src/pool.rs` (`BUNDLE_VERSION`) |
| Rust core: tree, nullifiers, shield / transfer / unshield, verify tail, auditor reveal, public bundle log, snapshot/restore | ✅ done | `core/` (`confidant-core`) |
| Tests | ✅ 29 tests (22 integration, 7 unit); negative suite from the audit REQ; every guard mutation-tested; **100% line coverage, enforced in CI**; core-level **e2e of the demo script** | `core/tests/`, CI `coverage` job |
| End-to-end | ✅ core level (`core/tests/e2e_demo.rs`) · ✅ **XRPL testnet level** (`e2e/testnet/run.mjs`: real deposit, memo anchors, payout to a fresh address, auditor reveal, log replay vs. anchored roots) · ⬜ through the HTTP API once `backend/` exists | `docs/E2E.md` |
| CI | ✅ fmt / clippy `-D warnings` / tests (release) / coverage gate / hygiene (no key-shaped files, honest-label markers present, allow-listed over-claim grep) | `.github/workflows/check.yml` |
| Security review | ✅ DESIGN-PASS + IMPL-PASS from the audit session, two slice PASSes for follow-ups; conditions applied | `docs/audit/` (`INDEX.md`) |
| Honest labels (custodial model, endpoint correlation, auditor-key scope, empty pool) | ✅ in SPEC, README, crate docs | `SPEC.md` §Privacy, `README.md` |
| Core ↔ API contract | ✅ documented | `docs/CORE-API.md` |
| HTTP backend (axum, 5 endpoints) | ⬜ not started | `backend/` (API/XRPL lead) |
| XRPL: pool account, faucet funding, deposit validation, memo anchoring, payout Payments, pre-population | ⬜ not started | `scripts/`, FE (API/XRPL lead) |
| Front-end + demo script + pitch | ⬜ not started | `frontend/` (API/XRPL lead) |

---

## 2. Milestones (from SPEC) — core side vs. XRPL/API side

| Milestone | Core (`confidant-core`) | API / XRPL / UI |
|---|---|---|
| **M0 — pre-event prep (not judged)** | ✅ circuits prove/verify (keys 2.5 s, 2-action bundle ≈ 0.9 s, verify ≈ 10 ms); repo scaffold; design + impl audited | ⬜ testnet pool account + N faucet-funded accounts; backend scaffold with the 5 endpoints over the core (stubs or real) |
| **M1 — shield** | ✅ `Pool::shield(owner, amount, deposit_tx_hash)` → commitment, leaf index, new root | ⬜ validate the deposit Payment on XRPL (amount, destination = pool, tx hash) **before** calling the core — this is the custodial backing step; `POST /shield`; `GET /pool/state` |
| **M2 — private transfer** | ✅ `Pool::transfer` → one proven Orchard bundle, nullifiers recorded, root advances; nothing for the ledger | ⬜ `POST /transfer` with sender authentication (`senderAuth` → `owner`) |
| **M3 — anchor** | ✅ `state().root`, `set_anchored_root`, exportable bundle log, third-party replay (`examples/demo.rs`) | ⬜ write the root into a Payment memo each batch; confirm back with `set_anchored_root`; explorer link |
| **M4 — unshield** | ✅ `Pool::unshield` → proof bound to destination + amount, `payout {amount, destination}` | ⬜ build + submit the pool → destination Payment (single regular key), `POST /unshield` |
| **M5 — disclosure + demo** | ✅ `Pool::reveal(viewing_key)` → one holder's full activity + balance | ⬜ `POST /audit/reveal`; minimal UI driving shield → transfer → unshield → reveal next to the real testnet explorer; rehearsed 45-second script |

---

## 3. Done — detail

### Core (`core/`, crate `confidant-core`)
- **Circuit:** Zcash Orchard Action circuit reused unchanged; proving/verifying keys built from the
  circuit at process start (no key artifact to trust). Every op is one Orchard bundle, padded to
  ≥ 2 actions.
- **State:** Sinsemilla commitment tree (depth 32, `shardtree`) with the full anchor history; global
  nullifier set; append-only public bundle log (no plaintext, no keys), rebuildable into live
  bundles and re-verifiable; operator keystore (one random spending key per owner); tracked notes.
- **Verify tail** (pure, then atomic apply): value-balance sign per op → vouched amount ==
  value balance → anchor ∈ history → nullifiers (replay/double-spend) → proof → spend-auth
  signatures → binding signature. Any failure leaves the pool unchanged.
- **Sighash** `BLAKE2b-256("ConfidantSigHash")(bundle commitment ‖ op tag ‖ external ctx)`
  binds a shield to its XRPL deposit and an unshield to its destination and its own value
  balance (payout amount single-sourced).
- **Auditor reveal:** the holder's Orchard full viewing key reveals their receipts, change, spent
  status and outgoing recipients/amounts — nothing about transactions they were not party to.
- **Persistence:** JSON `Snapshot` (contains spending keys — gitignored patterns) restored by
  replaying the log through the same verify tail.
- **Serde types** for the five SPEC endpoints (camelCase), ready to return as-is.

### Quality gates
- 29 tests; `e2e_demo.rs` plays the whole SPEC demo script (pre-populated pool, shield, anchor,
  private hop, backend restart, unshield, auditor reveal of exactly one holder, third-party replay
  of the public log); the negative suite covers double-spend, replay, unknown anchor, tampered value
  balance, swapped binding signature, corrupted proof, re-pointed unshield, cross-op tag, other
  circuit version, insufficient funds, old-anchor spend, snapshot restore (nullifiers survive,
  witnesses survive, corruption refused), reveal scope, every malformed record field.
- Every guard mutation-tested (remove the guard → its test goes red) — tables in
  `docs/audit/REQ-pool-core-impl-2026-10-04.md`.
- 100% line coverage, CI-enforced; regions at ~97.6% (the rest are `Err` arms of `?` on library
  calls that cannot fail under the pool's invariants).

### Review record (`docs/audit/`)
- `REQ/RESP-pool-design-2026-10-04` — DESIGN-PASS, conditions C1–C5 (all applied).
- `REQ/RESP-pool-core-impl-2026-10-04` — IMPL-PASS @ `aba71fe`; slice PASS for the
  nullifiers-before-proof reorder (PR #2) with a recorded caveat; slice PASS for the coverage /
  `verify` tightening work (PR #3).

---

## 4. To do — crypto/ZK lead (core)

Priority order; each item that touches `core/` goes through a PR + an audit slice ruling.

1. **Integration support.** Adjust types/signatures as the API layer needs them (owner ids,
   address encoding, receipt fields). Expect small, fast slices.
2. **Snapshot path helper** (audit note N): `Snapshot::save(path)` / `load(path)` that only
   accepts the gitignored names (`*.pool-state.json`, `/data/`), so a caller cannot write keys
   next to the code.
3. **Pre-population support.** Nothing new in the core (`shield` per account already works);
   `core/examples/cli.rs` + `e2e/testnet/run.mjs` are the template for a pre-population script.
4. **Fixed denominations (optional, demo privacy).** If the API lead wants it, a `Denominations`
   policy check in the core (`shield`/`unshield` amounts ∈ allowed set). Otherwise API-layer policy.
5. **Roadmap items from the audit (not for the hackathon):** pool-instance id in the sighash
   personalization (R2); `external_ctx` taking the unshield amount as `u64` at the call site;
   bounded anchor history; sender authentication inside the core.

## 5. To do — API/XRPL lead (backend, XRPL, UI, pitch)

In demo order; `docs/CORE-API.md` maps each endpoint to a core call.

1. **Backend scaffold** (`backend/`, axum): one `Pool` behind a mutex; the five endpoints;
   `senderAuth` → `owner` mapping; owner registration (`register_owner`, hand the `viewingKey` to
   the "auditor"); error → HTTP status mapping; snapshot to a gitignored file on each accepted op.
2. **XRPL plumbing** (`scripts/`, `xrpl.js`): pool account on testnet with a regular key; faucet
   funding for the pool and N demo accounts; the token to use (XRP or an issued token — decide).
3. **Shield path:** submit the deposit Payment → verify it on-ledger (amount, destination, hash) →
   `POST /shield`. The backing of the whole pool is this check; keep it strict.
4. **Anchor path:** after each accepted op (or each batch), read `GET /pool/state` and write
   `root` into a memo Payment from the pool account; confirm with `set_anchored_root`.
5. **Unshield path:** `POST /unshield` → build the pool → `destination` Payment for
   `payout.amount` → submit → show on the explorer.
6. **Pre-populate the pool** before the demo (many testnet accounts shield, some transfer) so the
   anonymity set is believable and unshields are not 1:1 linkable on stage.
7. **Minimal UI**: shield / transfer / unshield / reveal buttons, explorer links, the reveal JSON.
   Use the real testnet explorer, not a mock.
8. **API-level e2e** (`docs/E2E.md` §3): point `e2e/testnet/run.mjs` at the endpoints instead of
   the CLI; the XRPL side (deposit validation, memo anchoring, payout) is already written there and
   can be lifted into the backend.
9. **Demo script + pitch** (SPEC §Demo script): rehearse the 45 seconds; keep the honest labels in
   the words (custodial today, hides the internal graph, not the endpoints; ZK first, TEE only as
   a roadmap footnote). Bonus only if time: SignerList multisig, Proof-of-Innocence.

## 5a. v2 — trust-minimized architecture (decided 2026-10-05, in progress)

Decision: leave the single trusted operator behind and reach decentralization comparable to
Railgun/Tornado on today's XRPL (not better: without an amendment nobody on-ledger verifies a
proof, so the ceiling is a federated k-of-N). Design: `SPEC.md` §"v2" / `SPEC.ru.md` §16; audit:
`docs/audit/REQ-v2-trust-minimized-2026-10-05.md`.

| Step | What changes | Owner | Status |
|---|---|---|---|
| **1. Keys and proving on the client** | `Pool` splits into `Wallet` (keys, own notes, build/prove/sign; a local process, no WASM) and `Verifier` (tree, nullifiers, log, verify/apply). The API takes a ready bundle; `senderAuth` disappears (spend-auth signatures authorize). **Deposit binding:** the deposit Payment's memo carries the shield bundle's hash; a verifier accepts a shield only if the memo hash matches and the deposit is unconsumed | crypto lead (core), API/XRPL lead (deposit memo in FE, localhost wallet) | ✅ core (PR #8) · ✅ harness · ⬜ backend/FE |
| **2. State on the ledger** | one anchor memo per op: `root ‖ bundle hash ‖ external ‖ nullifiers ‖ commitments` (≤ 1 KB); proofs mirrored off-ledger by hash; anyone rebuilds the verifier state from the pool account's transactions + a mirror | crypto lead (codec, rebuild), API/XRPL lead (scanner) | ✅ core codec + `rebuild` · ✅ harness scanner (`account_tx`) · ⬜ backend scanner |
| **3. k-of-N verifiers (SignerList)** | the pool account gets a SignerList quorum, master key disabled; every signer runs its own `Verifier`, re-checks the proof and the deposit on-ledger, and signs payout/anchor only then (`Verifier::endorse`); a coordinator assembles the multisig. Anchor first, payout second (XLS-56 Batch not used) | API/XRPL lead (harness shows 3 signers) | ✅ core `endorse` · ✅ harness 2-of-3 on testnet (2026-10-05) · ⬜ signer services |
| **4. Permissionless relaying** | any client submits to any verifier; `GET /log` serves the mirror; node-operator guide | both | ✅ core/CLI (any verifier accepts; `export-log`/`rebuild`) · ✅ `docs/NODE-OPERATOR.md` · ⬜ `GET /log` endpoint |

Honest label after v2: notes are non-custodial (plaintext never leaves the client); the pool account
and proof verification sit with a k-of-N quorum of independent verifiers; state is reconstructible
from the ledger; **collusion of k signers can drain the pool account but cannot spend anyone's
notes** — the real gap vs. Tornado/Railgun, kept prominent. Dated and upgradeable: k-of-N because
today's XRPL has no on-ledger Halo 2 verifier; an amendment adding one upgrades this to on-ledger
verification. Signers never blind-sign: each applies the transition to its own state and signs only
the root it computed (`Verifier::endorse`). Upside vs. Railgun: Halo 2 has no trusted setup.
Audit: DESIGN-PASS (conditional) 2026-10-05 — `docs/audit/RESP-v2-trust-minimized-2026-10-05.md`.

## 6. Decisions still open

- **Token:** XRP vs. an issued/MPT token for the pool. The core is token-agnostic (one token per
  pool).
- **Denominations:** fixed set (recommended for the privacy story) or free amounts.
- **Anchoring cadence:** per op (simplest, more Payments) or per batch.
- **Who runs the backend on stage** (laptop vs. a small VM) and where the snapshot lives.

## 7. Risks and the honest line

- **Custodial build.** The operator holds every spending key, can spend any holder's funds, and
  vouches that each shield is backed by a real deposit. Say it. Removing it (distributed operator,
  TEE, in-enclave deposit proofs) is the production roadmap, never a demo claim.
- **Endpoint correlation.** The pool hides the internal graph, not that someone shielded X and
  later someone unshielded X. Pre-populate; prefer fixed denominations; add delay in the demo.
- **Empty pool at launch = no privacy** for the first operations. Pre-populate before the demo.
- **Proving cost is fine** (≈ 0.9 s per 2-action bundle, +0.45 s per extra action, ~100 MiB).
- **No on-ledger verification** (SPEC non-goal). The amendment sketch in SPEC is the story for that.

## 8. Working agreements

- Branch + PR for everything; `main` only via PR with green CI. Core changes get an audit slice
  ruling before merge; the ruling is committed verbatim under `docs/audit/`.
- Never commit keys, snapshots or `.env`; CI's hygiene job checks. Never put real names in git.
- Keep `SPEC.md`/`README.md` labels in sync with what the code enforces (CI checks the markers).
