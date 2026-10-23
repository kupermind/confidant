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
pre-event preparation on a pre-existing library (the Zcash Orchard circuit). The XRPL integration
already built is pre-event too; the judged work is what §0 decides to build on-site. Say so on stage.

---

## 0. Hackathon plan — DECIDED 2026-10-16 (plan of record for Oct 24–25; drafted 2026-10-13)

**The problem.** The event rule (organizer's page): *"Bring an existing project … and use the 36
hours to ship a new feature on XRP Ledger. Only what you build during the hackathon counts toward
judging."* Sat Oct 24, 8:30 AM → Sun Oct 25, 8:00 PM. The XRPL integration this file used to call
"the judged work" (quorum services, deposit binding, browser wallet, fees, screening) is already built
and deployed, so it is pre-event work too. Unless something new is built on-site, nothing is judged.
Rubric, pitch length and submission format are not published yet.

**Working rules until Oct 24 (in force).**
- **Feature freeze on `main`.** Pre-event work is ops, rehearsal, design and the deck only. Open items
  that are good on-site material (§4.3a refund path, payout resume, §4.5.3 one-payment disclosure) wait
  for the event.
- **Public pre-event baseline.** The submission needs a public repo. Tag the baseline (e.g.
  `pre-hackathon`) so the split between "came with" and "built this weekend" is a `git diff` anyone can
  run. Say on stage: "we arrived with a working shielded pool on testnet; this weekend we built X."
- **Designs may be written before the event; code is written at the event.** The audit session is
  asked to be on call during the 36 hours for the slice rulings.

**On-site candidates (options, to be chosen).**

| Option | What | XRPL tie | ZK depth | Fits 36 h |
|---|---|---|---|---|
| A. Credential-gated shielding | shield only from accounts holding an accepted XLS-70 Credential from a trusted issuer (issuer set possibly via a Permissioned Domain); node checks at submit and endorse, sidecar refuses, issuer page + accept step in the UI | native compliance primitives (live on mainnet) | low | yes — same shape as the screening gate |
| B. One-payment disclosure | note-opening report (§4.5.3): hand an auditor one payment, not the viewing key; optionally bound to the auditor's credentialed XRPL account | via A's credentials | medium (core + wasm) | yes |
| C. ZK proof of shielded funds | a holder proves "I control unspent notes worth ≥ X" to a lender / underwriter (lending track) without revealing notes or the total; the verdict can become an XLS-70 Credential on the borrower's account | lending track; checked against a verifier rebuilt from the anchors on XRPL ("unspent now" needs the full nullifier set, not only `account_tx`) | medium — **Orchard circuit unchanged** (see feasibility below) | **yes** — the prototype works |
| D. Full data availability on the ledger | bundles published as chunked memos (§4.5.1); any node rebuilds from XRPL alone, no mirror | XRPL as the data layer | low | yes, but not visual |
| Fillers | §4.3a refund for refused deposits; one-click payout resume from `payoutBlob` | — | — | yes |

**Option C feasibility (2026-10-13, throwaway prototype, not merged).** No new circuit is needed.
The proof is an ordinary Orchard bundle built like an unshield of X with change back to self, but
signed over a separate sighash domain (personalization `ConfidantPoF_v1_`, context = lender
challenge ‖ X) that no pool operation ever computes. A read-only check then confirms: known
anchor, nullifiers unspent, valid proof, signatures under the proof-of-funds sighash, value balance
= X. Prototype results (release, laptop): prove 1.3 s, verify 10 ms, 2 actions, ≈ 18.6 KB as JSON;
reveals X, not the total; bound to the challenge and to X; `submit_unshield` / `submit_transfer` of the
same record are refused (`InvalidSpendAuth`) and nothing is applied; a keyless verifier rebuilt from
the public log reaches the same verdict; once the proven notes are spent the old proof fails
(`DoubleSpend`). Honest limits, to state if C ships: (1) **a snapshot, not a lock** — funds can move
right after; the pool cannot encumber notes without a quorum-enforced lock (a trust-path change);
(2) **the lender sees the nullifiers**, so when those notes are later spent the lender knows that
operation was the holder's — mitigated by the wallet rotating the notes to itself right after
proving (the lender then learns only that one self-transfer is theirs); (3) **a proof can be lent
out** — it proves someone who controls ≥ X answered the challenge, not that this person is the
borrower; binding to the borrower's XRPL account signature narrows but does not close it
(the §4.5.3 audit condition applies); (4) the action count hints at how many notes were used.
On-site work: core (`pof` sighash domain, `Wallet::build_proof_of_funds`,
`Verifier::verify_proof_of_funds`, tests to the 100% gate, mutation table, audit slice), wasm export,
a lender page that verifies in the browser, credential issuance by xrpl.js.

Current lean (2026-10-14): **A + C** — "compliant privacy built on XRPL Credentials": A gates who
enters the pool with an XLS-70 Credential; C lets a holder prove funds ≥ X to a lender, who checks
it against the anchors on XRPL and can record the verdict as a Credential on the borrower's account.
B (one-payment disclosure) is the next candidate if time remains. Not considered:
RLUSD/issued tokens (deposit/payout/trust-line surface, risk), on-ledger verification (non-goal), a
hardware signer in the quorum (liveness on stage).

**C's prototype is pre-event work and is disclosed as such.** Draft PR #29 (branch
`spike/proof-of-funds`, not for merge) carries it with open questions for an external design review
(domain separation, the "≥ X" range argument, nullifier linkage, transferability, snapshot vs lock).
If C is chosen, the on-site work is the full implementation (audit slice, coverage and mutation table,
wasm export, lender page, Credential issuance); on stage the spike is named as pre-event.

**C's design review (2026-10-14): conditional DESIGN-PASS** (`docs/audit/REQ+RESP-pof-design-2026-10-14.md`,
relayed on PR #29). Sound as an event-scope *underwriting signal*: domain separation holds, the
"≥ X" argument holds, nullifier linkage is inherent (self-rotation is an acceptable labelled
mitigation), a snapshot not a lock. Conditions if C is chosen:
1. ✅ every pool apply-path re-verifies both signatures under the pool sighash before consuming a
   nullifier, plus a guard that enumerates every hash domain and forbids collisions — landed (PR #31);
2. a distinct record type, so a proof of funds is refused before any signature check;
3. break tests in both directions with a mutation per site;
4. coverage back to 100 % (every error arm);
5. the challenge carries the lender's identity, the borrower's XRPL account and a ledger index or
   expiry; the lender re-verifies at decision time;
6. the record never goes to a coordinator and never enters the public log; as few parties as
   possible see the nullifiers (preferably the quorum verifies and issues the Credential, not the
   lender);
7. labels on the lender surface: "signal, not proof of ownership" and "snapshot, not lock".

**On the ledger — proposal: [`docs/PROOF-OF-FUNDS.md`](./docs/PROOF-OF-FUNDS.md).**
Condition 6 means the lender never holds the proof, so the verdict has to live on XRPL: an XLS-70
Credential issued by the pool account k-of-N after each signer's node verified the proof itself
(Tier 1, with an `Expiration`), revoked by the quorum when the proven notes move (Tier 2, a tripwire,
not a lock). **There is no "proof of funds that locks" with the circuit unchanged** (§1a there): a
real lock is either **shielded collateral escrow** — the collateral moves into a note under a 2-of-3
FROST group {holder, lender, arbiter = quorum}, a separate post-event product, not a PoF tier — or a
**circuit-level** encumbrance (Tier 4, post-event). The deck presents proof of funds and escrow as two
constructions, not a ladder (lead's objection, audit's agreement, 2026-10-14).

**Proof-of-funds tiers at a glance** (detail in [`docs/PROOF-OF-FUNDS.md`](./docs/PROOF-OF-FUNDS.md)):

| Tier | What | On the ledger | Gives | Does not give | Plan |
|---|---|---|---|---|---|
| core slice | `Wallet::build_proof_of_funds`, `Verifier::verify_proof_of_funds`, own record type, audit conditions 2–4 | nothing | a proof a node can check read-only | anything a lender can read | event |
| 1 | nodes verify via `POST /pof/verify` (not logged, never via the coordinator); the quorum multisigns an expiring `CredentialCreate` (`ConfidantPoF`, `URI = H(X ‖ L ‖ E)`) through a new sidecar kind `credential-pof`; the borrower signs `CredentialAccept` | borrower account, type, expiry, hash — no notes, no nullifiers, no proof | a verdict the lender reads on XRPL; account binding; visible freshness | a lock; liveness beyond the quorum | event, **if A lands by Saturday evening**; otherwise the diagram |
| 2 | nodes keep the verified nullifiers in a private watch list; on spend the quorum multisigns `CredentialDelete` (sidecar kind `credential-revoke`) | the revocation | a tripwire the lender can watch | a lock; revocation timing leaks that the holder transacted (label it) | stretch, otherwise slides |
| — (escrow, not a tier) | **shielded collateral escrow**: collateral moves into a note whose spend key is a 2-of-3 FROST RedPallas group {holder, lender, arbiter = quorum}; the arbiter releases or liquidates only after reading the loan's state on the ledger | ordinary anchors only | prevention by key control, circuit unchanged — a different product (funds move, holder gives up sole custody) | proof of funds; time locks; the arbiter widens what the quorum can do (state it next to the drain line) | slides; post-event |
| 4 | a new circuit: proof of funds without revealing nullifiers; notes with spending conditions | — | the full answer | — | one line on a slide; post-event |

**Scope check (2026-10-14): all of this is more than 36 hours.** The cut, decided 2026-10-16 (e):
event = A + C's core slice + C Tier 1 if A lands by Saturday evening (A and Tier 1 share one
Credential module in node and sidecar); Tier 2 a stretch; escrow and Tier 4 on slides only, as two
separate constructions; B dropped.
UI surfaces are the FE team's: credential issue/accept, "prove funds" in the wallet, a lender page
that reads the Credential.

**Decisions — all taken by the owner on 2026-10-16 ((f) on 2026-10-14).** Nothing below is built yet;
the build happens on-site under the ship rule.

| # | Decision | Options | Decided |
|---|---|---|---|
| a | ✅ choose C or not | A only · A + C | **A + C.** A first, then C, one shared Credential module in node and sidecar (the audit's advice). |
| b | ✅ framing on every surface | — | **Confirmed.** Never "borrower B owns ≥ X", only "someone controlling ≥ X answered a challenge bound to B"; the Credential means "controlled ≥ X as of ledger L", not "controls it now". Anything else is a blocker. |
| c | ✅ where the proof is verified | node endpoint `POST /pof/verify` · verifier in the lender's browser | **Node endpoint.** Condition 6: the quorum sees the nullifiers, not the lender; the lender reads a Credential on the ledger instead of holding the record. The browser-side verifier is dropped. |
| d | ✅ X in the Credential | clear in the URI · only a hash, X given off-ledger | **Clear in the URI** for the event: a loan broker reads one field and the slide explains itself. Hash-only (threshold off the public ledger) stays an option to add later. |
| e | ✅ the scope cut above | — | **As proposed**, under the ship rule below: A + C core slice + Tier 1 only if A lands by Saturday evening and gets its own IMPL-PASS in-window; Tier 2 stretch; escrow and Tier 4 slides only; B dropped. |
| f | ✅ **track** (decided 2026-10-14) | Lending & Borrowing · Protocol Innovation · Why Not | **Lending & Borrowing.** Proof of funds is an underwriting signal for a loan broker, escrow is collateral, and both read loan state from the XRPL Lending Protocol (XLS-66) — the track asks for builds on it before it goes live. Protocol Innovation targets core infrastructure (rippled, C++), clients and tooling; nothing here touches rippled and on-ledger verification is a SPEC non-goal. Why Not is the fallback for the pool alone. |
| g | ✅ network for the demo | pool and lending both on devnet · pool on testnet, vault + broker + loan on devnet · wait for testnet | **Revised 2026-10-23: the pool stays on testnet.** The move to devnet was never started and everything proven — the quorum, HTTPS, the measured timings, the pre-populated pool — is on testnet, where Credentials and Permissioned Domains are enabled, so A and C's Tier 1 run there unchanged. The Lending Protocol (XLS-66) and Vaults are enabled only on devnet, so the broker/loan side of the demo runs on devnet with separate accounts, or on slides; the slide says so in one sentence. (Original decision of 2026-10-16: everything on devnet with a redeploy script — superseded.) |

**Ship rule, above the schedule** (audit, review on PR #27): a new **signing** path ships only with
its own firsthand IMPL-PASS inside the window; a **read** gate does not carry that risk. A (a ledger
read, fail-closed, the screening gate's shape) fits; Tier 1 (`credential-pof`) ships only if A lands
**and** it gets its own IMPL-PASS in-window, otherwise the flow diagram is the correct outcome; Tier 2
is a stretch under the same rule. The six `credential-pof` requirements (own node's verdict,
stand-alone kind, node-recomputed `URI`, node-checked freshness, mandatory unspent re-check at
signing, never persisted or logged) and the Credential's meaning ("controlled ≥ X as of L", not
"controls now") are in [`docs/PROOF-OF-FUNDS.md`](./docs/PROOF-OF-FUNDS.md) §2.

**Timeline for A + C** ((c) node endpoint, (g) pool on testnet, loan side on devnet or slides).

| When | Crypto/ZK lead | API/XRPL lead |
|---|---|---|
| Sat 9–13 | Credential module in node + sidecar; credential check at submit and endorse | issuer page, issue/accept flow |
| Sat 13–20 | tests, mutation table, audit REQ, deploy to the testnet quorum | refusal + status in the UI; deck skeleton |
| Sat 20 – Sun 2 | proof of funds in core (from the reviewed design); if A is in: `POST /pof/verify` + sidecar kind `credential-pof`; audit REQ | "prove funds" in the wallet; lender page reading the Credential; re-populate the pool with credentialed holders |
| Sun 9–14 | audit fixes; Tier 2 or fillers if ahead | deck and story (escrow + Tier 4 slides, as separate constructions) |
| Sun 14–18 | freeze; two full rehearsals on the demo laptop; fill `docs/DEMO.md` timings; fallbacks | same |

**Other items and status (2026-10-14).**

| Item | Status |
|---|---|
| HTTPS on the demo domain (`confidant.im`) | ✅ live, PR #28 merged |
| Re-run the headless-browser cycle over HTTPS; fill the `docs/DEMO.md` threaded timings | ✅ done by the crypto/ZK lead: cross-origin isolated, 16 threads, proving keys 6.0 s, shield proof 3.2 s, whole cycle with fees 81 s (PR #34, merged) |
| Move the quorum to devnet | dropped 2026-10-23 — the pool stays on testnet (decision g, revised) |
| Re-populate the pool on testnet, then the public log snapshot (PR #49), then cut the `pre-hackathon` baseline tag | ✅ 2026-10-23: pre-populated (8 holders × 5/10/25 XRP + 4 private transfers, 0 failed; pool 28 → 40 bundles, 274.29 XRP shielded; fees swept, available 0) → snapshot pinned, CID `bafybeifgshmfpvfw75wbi5755urzjxejvjwv5lwds6lys6nelajd6etzda` (40 records, root `dd1a9a87…`), published on the ledger in memo tx `8CBBA25CC347…`, and a fresh node rebuilt from the gateway copy to the live root in 15 s → tag `pre-hackathon` on the commit that merges this row |
| C spike, draft PR #29 | stays a draft, not for merge; disclosed as pre-event; its coverage CI fails on purpose |
| Apply-path guards (audit condition 1) | ✅ PR #31 |
| Which of XLS-65 / XLS-66 / XLS-80 are enabled on testnet or devnet; how a loan broker can require a Credential | ✅ checked 2026-10-14 (amendments object on each network, rippled 3.4.1). **Testnet:** Credentials, PermissionedDomains enabled; SingleAssetVault and LendingProtocol supported but **not enabled**. **Devnet:** all four enabled. XLS-66 has no on-ledger borrower gate (uncollateralized loans, off-chain underwriting; `LoanSet` is counter-signed by the broker), so the broker requires the Credential by policy before counter-signing; on-ledger Credential gating exists for depositors of a private vault (XLS-65 `tfVaultPrivate` + XLS-80 domain). XLS-70 reserve and delete rules confirmed. Detail in `docs/PROOF-OF-FUNDS.md` §2. Leads to decision (g). |
| `credential-pof` operations: anyone-deletes-expired cleanup (pool reserve); bound `POST /pof/verify` cost per node | open — in the Tier 1 design |
| Confirm Credentials and Permissioned Domains on testnet | ✅ enabled on testnet and devnet (same check, 2026-10-14) |
| A design written (no code) | open — next docs PR; the audit wants A's REQ first on-site |
| Pitch length, rubric, submission format from the organizers | open |
| Where the public repo lives | ✅ this repository (`kupermind/confidant`), flipped to public on the evening of 2026-10-23 after the baseline tag — the PR and audit history is part of the story |
| FE team: design and deck structure | open (FE team) |
| Rehearse on the demo laptop | open |

---

## 1. Where we are — one screen

| Area | Status | Where |
|---|---|---|
| **v2 trust-minimized architecture** (client-side keys/proving, state on ledger, k-of-N SignerList, permissionless relaying) | ✅ core + harness (PR #8); ✅ quorum services (PR #11); ✅ deployed on a testnet host; ⬜ user-facing layer — §5 | `SPEC.md` §4–5, §16; `docs/audit/`; `docs/API-BOUNDARY.md` |
| Reused circuit validated (M0 gate) | ✅ Zcash Orchard Action circuit unchanged, `orchard` 0.15.5 / `orchard_v2` / `FixedPostNu6_2` | `core/src/verifier.rs` (`BUNDLE_VERSION`) |
| Core: `Wallet` + `Verifier`, deposit binding, anchor memos, `rebuild_checked`, `endorse`, reveal, key-free snapshots | ✅ | `core/` |
| Tests | ✅ core: 31 tests (incl. the apply-path signature-safety + hash-domain guards, 2026-10-14), 27 + 5 guards mutation-tested, **100% line coverage, enforced in CI**; node: 10 tests against a fake ledger, 25 + fee guards mutation-tested, 95% gate (≈ 98%); sidecar guard: node:test suite (14 tests incl. `fee-sweep`), 20 guards mutation-tested | `core/tests/`, `backend/node/tests/`, `backend/quorum/test/`, CI `coverage` + `quorum` jobs |
| CI | ✅ fmt / clippy `-D warnings` / tests (release) / coverage gates (core 100%, node 95%) / sidecar unit tests / hygiene (no key-shaped files, label markers in SPEC/README/SPEC.ru, allow-listed over-claim grep) / manual testnet e2e (both harnesses) | `.github/workflows/check.yml` |
| Security review | ✅ v1 DESIGN-PASS + IMPL-PASS (+2 slice PASSes); v2 core DESIGN-PASS + IMPL-PASS; quorum services DESIGN-PASS (C1–C5, all implemented) + IMPL-PASS; deployment manifests passed a security-delta look | `docs/audit/INDEX.md` |
| Honest labels (v2 wording) | ✅ SPEC, SPEC.ru, README, crate docs | — |
| XRPL testnet e2e | ✅ CLI-driven (`run.mjs`): 2-of-3 SignerList, deposit binding, lying coordinator refused, new node rebuilt with deposit re-validation; ✅ through the services (`run-services.mjs`): coordinator flow, sidecar refusals (memo, payout, fee, sequence, LLS, order), custody gate, payout blob, new node `--rebuild`; ✅ smoke client against the deployed quorum | `e2e/testnet/`, `backend/quorum/smoke.mjs`, `docs/E2E.md` |
| Core ↔ API contract, node-operator guide, **UI ↔ backend boundary** | ✅ | `docs/CORE-API.md`, `docs/NODE-OPERATOR.md`, `docs/API-BOUNDARY.md` (+ RU) |
| **Quorum services**: Rust verifier node (`/submit/*`, `/endorse` idempotent, ledger-verified `/anchored`, `/log`, custody gate + scanner + `rebuild_checked` on start), Node signer sidecars (sign only what their own node endorsed; Fee/Sequence/LastLedgerSequence pinned from their own ledger reads), coordinator (k-of-N multisig, anchor then payout, payout blob returned) | ✅ built, audited and merged 2026-10-05 (PR #11); ✅ **deployed** the same night on a testnet host as systemd units behind nginx (`backend/deploy/`), 3 signers / quorum 2, smoke cycle green over the public address | `backend/`, `backend/deploy/README.md`, `docs/E2E.md` §2b |
| **Boundary screening** (optional per-node policy): listed depositors / destinations and too-fresh deposits refused at submit and endorse (403); OFAC XRP list fetcher; replayable audit over the public log | ✅ built, audited (CONDITIONAL → IMPL-PASS, canonical addresses) and merged 2026-10-10 (PR #20); deployed on the testnet quorum with the OFAC XRP list and a 2-ledger standby | `backend/node/src/screening.rs`, `backend/quorum/screening/`, `SPEC.md` §6, `docs/audit/RESP-screening-gate-2026-10-10.md` |
| **Protocol fee** (Railgun's economic model, server-side policy): configurable bps on shield and unshield, transfers free; shield credited net (deposit bound with `delivered = gross − fee`), payout net; per-node fee ledger replayable from the ledger, capped independently by the pool's ledger balance minus outstanding notes; k-of-N sweep to a treasury the sidecars verify against their own node | ✅ merged 2026-10-13 (PR #23; audit PASS + IMPL-PASS of the independent bound @ `f77775a`); ✅ **deployed** the same day on a fresh testnet pool with 25/25 bps from genesis (the fee policy is a genesis constant; the previous pool was retired), first real sweep verified on the ledger by all four nodes; UI shows the fee and builds the net note | `backend/node/src/fees.rs`, `SPEC.md` §6–7, `docs/API-BOUNDARY.md` §2, `docs/audit/REQ-protocol-fees-2026-10-11.md` |
| Front-end: UI over the coordinator, deposit from the user's XRPL account, explorer links | ✅ **prototype** 2026-10-10: `frontend/index.html` (static) with the wallet **in the browser** (`frontend/wasm`, core → WebAssembly in a worker; keys and proofs never leave the page) and the demo wallet service as fallback · ✅ 2026-10-13 **multithreaded proving in the browser** (`frontend/pkg-threads`, wasm-bindgen-rayon; page served with COOP/COEP; measured in headless Chrome on a 20-core laptop: proving keys 6.0 s and a 2-action proof 3.2 s on 16 threads vs 12.1 s / 16.2 s single-threaded on the same machine; whole shield → transfer → unshield → reveal cycle 75 s vs 127 s), proving key built in the background right after the wallet exists, thread count and proof time shown · ✅ `backend/quorum/prepopulate.mjs` · ✅ `docs/DEMO.md` run-sheet · ✅ **deployed 2026-10-13** (PR #24) and verified in headless Chrome against the server: real path shield → transfer → unshield → reveal with fees green (note 4.9875 XRP from a 5 XRP deposit, payout 4.975032): 131 s single-threaded over plain http, 77 s with 16 threads through a localhost tunnel (proof 16.3 s vs 3.1 s); all 25 demo-mode buttons; **caveat:** browsers honour COOP/COEP only in a secure context, so over plain `http://<ip>/` the wallet runs single-threaded (16 s per proof) — threads need HTTPS or `http://localhost` via an SSH tunnel to the host (documented in `docs/DEMO.md` and `backend/deploy/README.md`) · ✅ domain + TLS for the UI site (`confidant.im`, PR #28, live 2026-10-14) · ✅ cycle re-run over HTTPS (PR #34: 16 threads, proof 3.2 s, cycle 81 s) · ✅ 2026-10-21 **pool total in the UI** (`GET /fees.outstandingBacking`, the public backing: status bar and a row under the private balance, labelled public) · ✅ 2026-10-21 **overview page** `frontend/how-it-works.html` (served at `/how-it-works.html`, linked from the app header and footer: the pool, the quorum, the hackathon lending features, the enclave horizon as a footnote, honest limits) · ✅ 2026-10-22 **create-wallet flow on phones**: the card appears as soon as the key exists, the XRPL rows say "funding… (10–30 s)" with the fund button disabled, faucet calls are single-flight and the account is generated before the faucet is asked (a retry funds the same address), progress and errors visible in both states · ✅ 2026-10-22 step ticks in sync with the phases (a `classList.toggle` with an undefined flag was flipping them), screening standby shown as "Anchoring — the verifiers wait until the deposit is N ledgers old" instead of the node's refusal text · ✅ 2026-10-23 a line under the shielded total reconciles it with the explorer: "pool account B = backing + operator float and unswept fees" from `/fees.poolExcess` · ✅ 2026-10-23 the shielded total is a headline stat in the header (left of the status pill) and `frontend/balances.html` reconciles it with the pool account's balance live from the ledger (funding, deposits, payouts, sweeps, anchors, XRPL fees) with a FAQ · ✅ 2026-10-23 page tests in the repo and CI (`frontend/test`: pure functions incl. the ledger reconciliation against a fixture; headless-Chrome smoke of the three pages in demo mode) · ⬜ FE team: presentation and design; rehearsal timings on the demo laptop | `frontend/`, `backend/wallet/`, `backend/quorum/prepopulate.mjs`, `docs/DEMO.md` |

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
  once per hash). A second bound is independent of the fee ledger (audit, 2026-10-13): `available`
  is capped by `poolExcess` = the pool's validated balance minus the notes it backs, and a sweep that
  leaves the pool below its outstanding notes is never recorded. `setup-pool.mjs` funds the treasury (`FEE_SHIELD_BPS` / `FEE_UNSHIELD_BPS`, default
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
- **Apply-path signature safety + hash-domain guard** (2026-10-14, `core/tests/apply_path_guards.rs`;
  the audit's step (1) before any proof-of-funds work): a foreign-sighash bundle is refused at every
  state-changing entrypoint before any nullifier is consumed, a swapped binding signature likewise,
  state byte-identical; every BLAKE2b personalization and sighash context in `core/src` is enumerated
  and pinned to an allow-list. 5 mutations killed.
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
3b. **Logged by the audit with the apply-path guards (2026-10-14, PR #31; not part of step 1):**
   (i) **authority over `set_anchored_root`** — a `&mut` entrypoint outside the apply path (it never
   reaches `apply_verified`, so the signature invariant is untouched), but it mutates committed
   state. Today the core only requires the root to be one the pool has had; the node calls it in
   `POST /anchored` after reading the anchor Payment from the ledger itself and in `--rebuild` for the
   last memo's root; nothing signs or verifies against `anchoredRoot` (it is reported state).
   **Resolved firsthand by the audit the same day — a watch item, no slice:** `set_anchored_root`
   refuses any root that is not already a known anchor (the same gate `verify` uses), so it cannot
   inject a forged or new anchor, and `anchored_root` is inert (setter, `state()`/`snapshot()`,
   `restore()` round-trip only; every apply path keys off `is_known_anchor`). Reopen only if
   `anchored_root` ever becomes an input to a security decision.
   (ii) **scope of the hash-domain guard** — `core/tests/apply_path_guards.rs` globs `core/src` only,
   which is where every bundle sighash lives today; if another crate (node, sidecar, CLI) ever
   computes a bundle sighash, extend the glob there so the guard's scope tracks the code.
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
      `rebuild_checked` reassembles and hash-checks before use. The full data-availability design —
      layers, what dies with what, mirrors vs on-ledger chunks vs IPFS vs Filecoin/Arweave, holder-side
      persistence, the staged plan (stage 0 = off-host backup + a second node before the event) and the
      honest label per stage; stage 0 = a one-time demonstrative public IPFS snapshot of the log
      (`npm run log:snapshot`, proposed to the owner 2026-10-23, rehearsed locally: a node rebuilt from the
      ledger + the snapshot file reproduced the live state exactly) — is `docs/DATA-AVAILABILITY.md`.
      *Audit condition:* availability
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
   `/endorse` is idempotent). ✅ **HTTPS on the demo domain** (`confidant.im`, live 2026-10-14): DNS → the testnet host,
   Let's Encrypt certificate (webroot renewal with an nginx reload hook, dry run green), 443 open,
   `backend/deploy/nginx/confidant-ui-tls.conf` in place of the HTTP site (the HTTP site kept as a
   dated backup on the host), so every visitor's browser is a secure context and gets the thread pool
   (headless Chrome on another machine: `crossOriginIsolated === true`); steps and checks in
   `backend/deploy/README.md`, "HTTPS on a domain". ✅ Headless-Chrome cycle over
   `https://confidant.im/` (2026-10-14): cross-origin isolated, 16 threads, proving keys 6.0 s, shield
   proof 3.2 s, whole cycle with fees 81 s — recorded in `docs/DEMO.md`. ✅ **Pool pre-population**: `backend/quorum/prepopulate.mjs`
   (`npm run prepopulate`: N faucet-funded holders shield fixed denominations through the coordinator,
   some transfer; prints the honest "synthetic set" label). ✅ **Demo run-sheet**: `docs/DEMO.md`
   (what to click, say and show for SPEC §10, fallbacks, timing table) — ⬜ rehearse and fill the
   timings on the demo laptop. **Deployed 2026-10-13** (PR #24, nginx site with COOP/COEP, both
   bundles shipped) and verified end to end against the server from headless Chrome. (HTTPS on the demo domain: live, see above.)

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
