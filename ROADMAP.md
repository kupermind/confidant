# Confidant — Roadmap and status

**As of 2026-10-05, `main` after PR #8.** Hackathon: AI & Fintech XRP Ledger Hackathon, NYC,
**Oct 24–25, 2026**. Testnet-first. Design of record: [`SPEC.md`](./SPEC.md) / [`SPEC.ru.md`](./SPEC.ru.md).

Roles (names never, roles only): the **crypto/ZK lead** owns the Rust core (`core/`), the CLI test
tool and the e2e harness; the **API/XRPL lead** (repo owner) owns the HTTP layer, signer services,
everything XRPL in the front-end, the UI and the pitch.

Judging rule: *only what is built during the hackathon counts.* The core and the harness are
pre-event preparation on a pre-existing library (the Zcash Orchard circuit). The judged work is the
XRPL integration built on-site. Say so on stage.

---

## 1. Where we are — one screen

| Area | Status | Where |
|---|---|---|
| **v2 trust-minimized architecture** (client-side keys/proving, state on ledger, k-of-N SignerList, permissionless relaying) | ✅ core + harness (PR #8, 2026-10-05); testnet run green twice; ⬜ backend integration — §5 | `SPEC.md` §4–5, §16; `docs/audit/` |
| Reused circuit validated (M0 gate) | ✅ Zcash Orchard Action circuit unchanged, `orchard` 0.15.5 / `orchard_v2` / `FixedPostNu6_2` | `core/src/verifier.rs` (`BUNDLE_VERSION`) |
| Core: `Wallet` + `Verifier`, deposit binding, anchor memos, `rebuild_checked`, `endorse`, reveal, key-free snapshots | ✅ | `core/` |
| Tests | ✅ 29 tests (22 integration, e2e with two signers + rebuilding node, 6 unit); 27 guards mutation-tested; **100% line coverage, enforced in CI** | `core/tests/`, CI `coverage` job |
| CI | ✅ fmt / clippy `-D warnings` / tests (release) / coverage gate / hygiene (no key-shaped files, label markers in SPEC/README/SPEC.ru, allow-listed over-claim grep) / manual testnet e2e | `.github/workflows/check.yml` |
| Security review | ✅ v1 DESIGN-PASS + IMPL-PASS (+2 slice PASSes); v2 DESIGN-PASS + IMPL-PASS, all conditions met | `docs/audit/INDEX.md` |
| Honest labels (v2 wording) | ✅ SPEC, SPEC.ru, README, crate docs | — |
| XRPL testnet e2e | ✅ 2-of-3 SignerList, deposit memo binding, 3/3 signers endorsing, multisigned anchors + payout, lying coordinator refused, new node rebuilt from `account_tx` with deposit re-validation, lagging signer caught up | `e2e/testnet/run.mjs`, `docs/E2E.md` |
| Core ↔ API contract, node-operator guide | ✅ | `docs/CORE-API.md`, `docs/NODE-OPERATOR.md` |
| HTTP backend (`GET /log`, `POST /submit/*`, `/audit/reveal`), signer services, scanner | ⬜ not started | `backend/` (API/XRPL lead) |
| Front-end: local wallet, deposit memo, explorer links, UI; pre-population script; demo + pitch | ⬜ not started | `frontend/`, `scripts/` (API/XRPL lead) |

---

## 2. Milestones (from SPEC) — core + harness vs. API/XRPL/UI

| Milestone | Core + harness | API / XRPL / UI |
|---|---|---|
| **M0 — pre-event prep (not judged)** | ✅ circuit proves/verifies (params 2.5 s, 2-action bundle ≈ 0.9 s, verify ≈ 10 ms); scaffold; design + impl audited | ⬜ backend scaffold over `Verifier`; testnet pool account with SignerList + faucet accounts |
| **M1 — shield** | ✅ `Wallet::build_shield` → deposit memo = bundle hash → `Verifier::submit_shield(record, DepositRef)` | ⬜ validate the deposit on-ledger before calling the core; `POST /submit/shield`; `GET /log` |
| **M2 — private transfer** | ✅ `Wallet::build_transfer` → `Verifier::submit_transfer`; nothing for the ledger but the anchor | ⬜ `POST /submit/transfer` |
| **M3 — anchor** | ✅ `anchor_memo` per op, `check_memo`, `rebuild` / `rebuild_checked`; harness writes and reads memos on testnet | ⬜ multisigned memo Payment after each op; `anchored`; scanner |
| **M4 — unshield** | ✅ `Wallet::build_unshield` → `Verifier::submit_unshield` → `payout`; harness pays 2-of-3 | ⬜ `POST /submit/unshield` + multisigned payout |
| **M5 — disclosure + demo** | ✅ `Verifier::reveal(viewingKey)` | ⬜ `POST /audit/reveal`; UI drives shield → transfer → unshield → reveal on the real explorer; rehearsed 45-second script |

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

### Quality gates
- 29 tests; negative suite: double-spend, replay (before the proof), unknown anchor, tampered value
  balance, swapped binding signature, corrupted proof, re-pointed unshield, cross-op tag, other
  circuit version, bad requests before proving, old-anchor spend, deposit binding (stranger's bundle,
  wrong amount, reuse, duplicate in a log), bundle hash without metadata, memo codec, rebuild
  (wrong mirror / root / actions / external / deposit facts), wallet sync, signer endorse (wrong
  root / hash / external / op / actions, re-pointed payout, lagging signer), snapshot restore.
- 27 guards mutation-tested (remove → red); tables in `docs/audit/REQ-v2-core-impl-2026-10-05.md`.
- 100% line coverage, CI-enforced; regions ≈ 97% (unreachable `Err` arms of `?` on library calls).

### Review record (`docs/audit/`)
- v1: `REQ/RESP-pool-design-2026-10-04` (DESIGN-PASS, C1–C5), `REQ/RESP-pool-core-impl-2026-10-04`
  (IMPL-PASS; slices PR #2, PR #3).
- v2: `REQ/RESP-v2-trust-minimized-2026-10-05` (DESIGN-PASS, conditions met),
  `REQ/RESP-v2-core-impl-2026-10-05` (IMPL-PASS; backing on join in-core; release assert).

---

## 4. To do — crypto/ZK lead (core)

1. **Integration support.** Adjust types/signatures as the API layer needs them; each via PR + an
   audit slice ruling. Expect small, fast slices.
2. **Keep the docs in sync** with every core change (SPEC, SPEC.ru, CORE-API, E2E, this file).
3. **Roadmap items from the audit (not for the hackathon):** pool-instance id in the sighash
   personalization; `external_ctx` taking the unshield amount as `u64`; bounded anchor history;
   XLS-56 Batch for atomic anchor + payout; FROST for the signer keys.

## 5. To do — API/XRPL lead (backend, XRPL, UI, pitch)

In demo order; `docs/CORE-API.md` maps each endpoint to a core call, `docs/NODE-OPERATOR.md` describes
a signer, `e2e/testnet/run.mjs` has every XRPL step working.

1. **Backend scaffold** (`backend/`, axum): one `Verifier`; `GET /log?from=N`;
   `POST /submit/shield` (look the deposit up on-ledger: validated Payment, Destination = pool,
   `delivered_amount`, `confidant/shield` memo → `DepositRef`), `POST /submit/transfer`,
   `POST /submit/unshield`, `POST /audit/reveal`; `VerifierSnapshot` to a gitignored file.
2. **Signer services** (3 processes on stage, quorum 2): each a `Verifier` behind "endorse this
   `{submission, proposedMemo}`" → signature of the anchor (and payout) only on success; a coordinator
   collecting ≥ k signatures and submitting; rebuild from the ledger with `rebuild_checked` on start.
3. **XRPL plumbing** (`scripts/`, `xrpl.js`): pool account on testnet with `SignerListSet` + master
   disabled; faucet funding; the token (XRP or issued — decide).
4. **Front-end:** local wallet process (`Wallet` behind localhost) or the CLI during the hackathon;
   deposit Payment with memo = bundle hash; explorer links; shield / transfer / unshield / reveal
   buttons; the reveal JSON.
5. **Pre-populate the pool** before the demo (many testnet accounts shield fixed denominations, some
   transfer) so the anonymity set is believable.
6. **Demo script + pitch** (SPEC §10): rehearse the 45 seconds with the v2 wording (non-custodial
   notes, quorum can drain the pool account, internal graph hidden but not the endpoints, ZK first,
   TEE a footnote). Bonus only if time: Proof-of-Innocence, FROST.

## 5a. v2 — trust-minimized architecture (decided 2026-10-05, core + harness done the same day)

| Step | What | Status |
|---|---|---|
| **1. Keys and proving on the client** | `Wallet` / `Verifier` split; the API takes a ready bundle (`senderAuth` gone); deposit binding via the memo hash + consumed set | ✅ core (PR #8) · ✅ harness · ⬜ backend/FE |
| **2. State on the ledger** | anchor memo per op; proofs mirrored by hash; `rebuild` / `rebuild_checked` | ✅ core codec + rebuild · ✅ harness scanner (`account_tx`) · ⬜ backend scanner |
| **3. k-of-N verifiers (SignerList)** | SignerList quorum, master disabled; every signer `endorse`s on its own state; coordinator assembles the multisig; anchor first, payout second | ✅ core `endorse` · ✅ harness 2-of-3 on testnet · ⬜ signer services |
| **4. Permissionless relaying** | any verifier accepts; `export-log` / `rebuild`; node-operator guide | ✅ core/CLI · ✅ `docs/NODE-OPERATOR.md` · ⬜ `GET /log` endpoint |

Honest label after v2: notes non-custodial; pool account and verification federated (k-of-N); state
reconstructible from the ledger; **collusion of k signers can drain the pool account but cannot spend
anyone's notes** — the real gap vs. Tornado/Railgun, kept prominent and dated (k-of-N because today's
XRPL has no on-ledger Halo 2 verifier; an amendment adding one upgrades this). Signers never
blind-sign (`Verifier::endorse`). Upside vs. Railgun: Halo 2 has no trusted setup.

## 6. Decisions still open

- **Token:** XRP vs. an issued/MPT token (the core is token-agnostic, one token per pool).
- **Denominations:** fixed set (recommended) or free amounts.
- **Anchoring cadence:** per op (as implemented) or per batch.
- **Where the signer services run on stage** and where their public snapshots live.

## 7. Risks and the honest line

- **Federated custody.** Collusion of k signers can drain the pool account. Say it. On-ledger
  verification (the amendment) is the roadmap, never a demo claim.
- **Endpoint correlation.** The pool hides the internal graph, not that someone shielded X and later
  someone unshielded X. Pre-populate; prefer fixed denominations; add delay.
- **Empty pool at launch = no privacy** for the first operations.
- **Liveness** depends on k signers; on stage that is three processes — rehearse the restart path
  (`rebuild_checked` from the ledger).
- **Proving cost is fine** (≈ 0.9 s per 2-action bundle on the wallet side).

## 8. Working agreements

- Branch + PR for everything; `main` only via PR with green CI. Core changes get an audit slice
  ruling before merge; the ruling is committed verbatim under `docs/audit/`.
- Never commit keys, wallet files, `data/` or `.env`; CI's hygiene job checks. Never put real names in
  git.
- Keep `SPEC.md`, `SPEC.ru.md`, `README.md` and `docs/` in sync with the code in the same PR (CI
  checks the label markers).
