# `backend/` — the quorum services

The operational half of v2 steps 3–4. Three pieces; XRPL transactions are signed only in the Node
sidecars, the Rust node only reads the ledger.

| Service | Language | Holds | Does |
|---|---|---|---|
| `node/` — `confidant-node` | Rust (axum) | a `Verifier` state file (public, no keys) | `GET /state`, `GET /log?from=N`, `POST /submit/shield` (looks the deposit up on the ledger itself), `POST /submit/transfer`, `POST /submit/unshield`, `POST /endorse` (idempotent for the identical op + external + bundle; re-reads the deposit itself), `POST /anchored {root, anchorTxHash}` (reads the anchor tx from the ledger, checks its memo carries the root), `POST /audit/reveal`. Startup gate: `account_info` SignerList == `--signers` with `SignerQuorum == --quorum` and the master key disabled, else exit. Optional boundary screening: `--screening-list FILE --screening-standby-ledgers N` → 403 for listed depositors/destinations and too-fresh deposits, at submit and endorse alike (`quorum/screening/`: `fetch-ofac.mjs`, `audit.mjs`). `--rebuild --mirror <url>` = scanner (`account_tx` must start at the pool's creation → anchor memos) + `rebuild_checked` with every shield's deposit re-validated |
| `quorum/signer.mjs` | Node, xrpl.js | ONE signer's XRPL key (`*.key.json`, 0600) | `POST /sign {txs, submission, memoHex}`: asks **its own** node `/endorse`; reads the pool's next Sequence and the current ledger **itself** (`XRPL_RPC`); checks each tx against that endorsement — anchor: pool → sink, 1 drop, exactly the memo; payout: destination + amount from its node's answer; both: Fee ≤ `MAX_FEE_DROPS` (5000), Sequence pinned (anchor = next, payout = next + 1, anchor first), LastLedgerSequence present and within 60 (anchor) / 600 (payout) ledgers — only then multisigns |
| `quorum/coordinator.mjs` | Node, xrpl.js | nothing | `POST /submit/*` → own node → anchor (+ payout) Payments prepared with autofill (payout LLS generous) → fan out to sidecars → ≥ k signatures → `multisign` + submit anchor, `POST /anchored {root, anchorTxHash}` to every node, then the payout; the answer carries `payoutBlob` (resubmittable by anyone until its LLS) and `payoutError` if the ledger refused it after the anchor; proxies `GET /log`, `GET /state`, `POST /audit/reveal` |
| `quorum/setup-pool.mjs`, `start-all.mjs`, `lib.mjs` | Node | — | faucet pool + sink, N signer keys, `SignerListSet` + `asfDisableMaster`; launch N nodes + N sidecars + coordinator; shared helpers (tx shape checks, HTTP, launcher) |

```bash
cargo build --release --bin confidant-node --example cli
cd backend/quorum && npm install
SIGNERS=3 QUORUM=2 DIR=../../data/quorum npm run setup-pool        # once, testnet
NODE_BIN=../../target/release/confidant-node DIR=../../data/quorum npm run start-all
# coordinator on :7200, signer nodes :7001-7003 (+ the coordinator's node :7000), sidecars :7101-7103
# (start-all passes --signers/--quorum from config.json to every node; a node whose configured quorum
#  is not what the ledger shows exits with "pool custody on the ledger is not the configured quorum")
```

End to end through these services on testnet: `e2e/testnet/run-services.mjs` (`docs/E2E.md` §2b). Against
a running coordinator (deployed or local): `npm run smoke` in `backend/quorum` with `POOL` and
`COORDINATOR_URL`. Server deployment (systemd + nginx): `backend/deploy/README.md`.
Trust statements, what a sidecar pins, payout recovery: `SPEC.md` §6, `docs/NODE-OPERATOR.md`. Audit:
`docs/audit/REQ-quorum-services-2026-10-05.md` → `RESP-quorum-services-2026-10-05.md` (DESIGN-PASS, C1–C5).
