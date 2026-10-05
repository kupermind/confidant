# `backend/` — the quorum services

The operational half of v2 steps 3–4. Three pieces; XRPL transactions are signed only in the Node
sidecars, the Rust node only reads the ledger.

| Service | Language | Holds | Does |
|---|---|---|---|
| `node/` — `confidant-node` | Rust (axum) | a `Verifier` state file (public, no keys) | `GET /state`, `GET /log?from=N`, `POST /submit/shield` (looks the deposit up on the ledger itself), `POST /submit/transfer`, `POST /submit/unshield`, `POST /endorse` (idempotent; re-reads the deposit itself), `POST /anchored`, `POST /audit/reveal`; startup `--rebuild --mirror <url>` = scanner (`account_tx` → anchor memos) + `rebuild_checked` with every shield's deposit re-validated |
| `quorum/signer.mjs` | Node, xrpl.js | ONE signer's XRPL key (`*.key.json`, 0600) | `POST /sign {txs, submission, memoHex}`: asks **its own** node `/endorse`, checks each tx's shape against that endorsement (anchor: pool → sink, 1 drop, exactly the memo; payout: destination + amount from its node's answer), only then multisigns |
| `quorum/coordinator.mjs` | Node, xrpl.js | nothing | `POST /submit/*` → own node → anchor (+ payout) Payments prepared with autofill → fan out to sidecars → ≥ k signatures → `multisign` + submit anchor then payout → `POST /anchored` to every node; proxies `GET /log`, `GET /state`, `POST /audit/reveal` |
| `quorum/setup-pool.mjs`, `start-all.mjs`, `lib.mjs` | Node | — | faucet pool + sink, N signer keys, `SignerListSet` + `asfDisableMaster`; launch N nodes + N sidecars + coordinator; shared helpers (tx shape checks, HTTP, launcher) |

```bash
cargo build --release --bin confidant-node --example cli
cd backend/quorum && npm install
SIGNERS=3 QUORUM=2 DIR=../../data/quorum npm run setup-pool        # once, testnet
NODE_BIN=../../target/release/confidant-node DIR=../../data/quorum npm run start-all
# coordinator on :7200, signer nodes :7001-7003 (+ the coordinator's node :7000), sidecars :7101-7103
```

End to end through these services on testnet: `e2e/testnet/run-services.mjs` (`docs/E2E.md` §2b).
Trust statements and failure modes: `SPEC.md` §6–7, `docs/NODE-OPERATOR.md`. Audit:
`docs/audit/REQ-quorum-services-2026-10-05.md`.
