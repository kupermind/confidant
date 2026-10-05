# The boundary: `backend/` ↔ the user-facing layer

Status 2026-10-05, after PR #11. This is the hand-off document between the two builders: who owns
what, exactly what the UI talks to, what it must produce, and how the contract changes. If this page
and the code disagree, the code wins and this page gets fixed in the same PR.

## 1. Who owns what

| Side | Owns | Directories |
|---|---|---|
| **Crypto/ZK lead** | the Rust core; the **quorum services**: verifier node (`confidant-node`), signer sidecars, coordinator, pool setup and launch scripts; the CLI test tool; both e2e harnesses; the audit cycle | `core/`, `backend/`, `e2e/testnet/`, `docs/audit/`, `docs/*.md` |
| **API/XRPL lead** (repo owner) | the **user-facing layer**: UI, local wallet UX, the deposit Payment sent from the user's own XRPL account, explorer links, pool pre-population, demo script and pitch | `frontend/`, `scripts/` |

The seam is HTTP. The UI is a **client of the coordinator** (one process, holds no keys). Nothing on
the UI side holds pool keys, signer keys, verifier state or deposit-validation logic; nothing on the
backend side holds a user's spending keys or renders anything.

## 2. The contract the UI consumes — the coordinator

Base URL: `http://127.0.0.1:7200` when started by `backend/quorum/start-all.mjs` (configurable).
Everything is JSON. Errors are `{ "error": "<message>", "details"?: {…} }` with:

| Status | Meaning | Typical messages |
|---|---|---|
| 400 | malformed request or a core limit | zero amount, too many outputs, bad hex, missing fields |
| 409 | refused by the verifiers | nullifier already spent (replay / double spend), memo mismatch, unknown anchor, `only 1/3 signers endorsed (quorum 2)` |
| 422 | the ledger does not back the claim | deposit not validated / not to the pool / wrong amount / no `confidant/shield` memo; anchor tx does not carry the root |
| 502 | ledger RPC or a node unreachable | `txnNotFound <hash>` for a bogus deposit hash, `fetch failed` |

### Endpoints

| Call | Body | Answer |
|---|---|---|
| `GET /state` | — | `PoolState` |
| `GET /log?from=N` | — | `{ records: BundleRecord[], …PoolState }` — the mirror a wallet syncs from; save `records` as `log.json` |
| `POST /submit/shield` | `{ record, depositTxHash }` | `Accepted` + quorum fields |
| `POST /submit/transfer` | `{ record }` | `Accepted` + quorum fields |
| `POST /submit/unshield` | `{ record, destination }` | `Accepted` (with `payout`) + quorum fields incl. `payoutTx` / `payoutBlob` |
| `POST /audit/reveal` | `{ viewingKey }` | `RevealReport` |

Shapes (camelCase on the wire):

```jsonc
PoolState   { root, anchoredRoot | null, leafCount, nullifierCount, bundleCount, consumedDeposits }
Accepted    { bundleIndex, firstLeaf, newRoot, bundleHash, nullifiers[], commitments[], payout?: { destination, amount } }
quorum      { memoHex, anchorTx, payoutTx | null, payoutBlob | null, payoutError | null, signers: [rAddr…], refusals: [string…] }
RevealReport{ balance, notes: [{ direction: "incoming" | "outgoing", amount, leafIndex, bundleIndex, timestamp, spent, recipient? }] }
```

- `anchorTx` / `payoutTx` are XRPL transaction hashes: link them to `https://testnet.xrpl.org/transactions/<hash>`.
- A submit **blocks until the anchor (and the payout) validated on XRPL**: 5–15 s. Show progress.
- `payoutBlob` is the multisigned payout. If `payoutError` is set, the note is already spent and the user
  is unpaid; the UI (or anyone) may resubmit the blob with the XRPL `submit` command until its
  `LastLedgerSequence`, or simply re-POST the same `{record, destination}` to any coordinator
  (`/endorse` is idempotent; the sidecars re-sign). Never show "paid" before `payoutTx` is set.
- Amounts are **drops** (integers as numbers in `Accepted.payout.amount`, strings in XRPL fields).
- **The viewing key is the authorization for `/audit/reveal`** and reveals that holder's whole history
  (read-only, nothing else). The UI sends it only to the coordinator the user chose, over the URL the
  user configured, and says so before sending; it never stores or forwards it elsewhere. CORS is `*`
  because the coordinator holds nothing a foreign origin could take — the key stays the user's risk.

A complete client of this contract, including every negative case, is `e2e/testnet/run-services.mjs`.
Copy from there.

## 3. What the UI must produce — the `record`

`record` is a `BundleRecord` exactly as the wallet writes it (do not edit fields):

```jsonc
{ index, op: "shield" | "transfer" | "unshield", timestamp, external, anchor, flags, valueBalance,
  firstLeaf, actions: [{ nullifier, rk, cmx, … }], proof, bindingSignature }
```

It comes from the holder's wallet — during the hackathon the CLI (`cargo build --release --example cli`),
later a local process over `confidant_core::Wallet`. The wallet file `*.wallet.json` holds the
**spending keys** (0600, loud warning); it never leaves the user's machine and is never sent anywhere.

| Flow | Steps |
|---|---|
| **Sync** | `GET /log?from=0` → write `records` to `log.json`. Every build below takes it. |
| **Shield** | 1. `cli wallet --file u.wallet.json build-shield <drops> shield.json log.json` → prints `{ bundleHash, file }`. 2. The user's XRPL account sends a `Payment` of exactly `<drops>` to the pool account with one memo `{ MemoType: hex("confidant/shield"), MemoData: <bundleHash hex> }`; wait for validation. 3. `POST /submit/shield { record: <shield.json>, depositTxHash }`. **Build first, deposit second** — the memo must commit to the bundle; the amount must equal `delivered_amount`. |
| **Transfer** | sync → `cli wallet --file u.wallet.json build-transfer <recipientAddressHex> <drops> transfer.json log.json` → `POST /submit/transfer`. Nothing else reaches the ledger but the anchor. |
| **Unshield** | sync → `cli wallet --file u.wallet.json build-unshield <drops> <rDestination> unshield.json log.json` → `POST /submit/unshield` **with the same destination** (the signature inside the bundle binds it; another destination is refused). |
| **Reveal** | `cli wallet --file u.wallet.json info` → `viewingKey` → `POST /audit/reveal`. Shows that holder's full activity and nothing else. |
| **Balance** | `cli wallet --file u.wallet.json sync log.json` → `{ balance, synced, root }`. |

`init` creates the wallet file; `info` prints `{ address, viewingKey }` (recipient address = `address`, 43 bytes hex).

## 4. Not the UI's job, and not provided by `backend/`

- The UI never validates deposits, never computes anchors, never holds pool or signer keys, never
  decides that something is anchored: it passes `depositTxHash` and reads `anchoredRoot` / `anchorTx`.
- `backend/` does not provide user accounts, authentication, TLS, rate limiting (localhost demo),
  wallet storage or backup, the pre-population script (`scripts/`, owner), or any rendering.
- Trust statements to show the user are fixed in `SPEC.md` §4 and `README.md`; the UI must not
  claim more (CI checks the wording in the specs).

## 5. Running the backend for the UI

```bash
cargo build --release --bin confidant-node --example cli
cd backend/quorum && npm install
SIGNERS=3 QUORUM=2 DIR=../../data/quorum npm run setup-pool        # once per pool (testnet faucet)
NODE_BIN=../../target/release/confidant-node DIR=../../data/quorum npm run start-all
```

| What | Where |
|---|---|
| coordinator (the UI's base URL) | `http://127.0.0.1:7200` |
| coordinator's node / signer nodes | `:7000` / `:7001–7003` (same API minus the quorum fields; `/log` works on any of them) |
| signer sidecars | `:7101–7103` (not for the UI) |
| pool account, sink, signer addresses, quorum | `data/quorum/config.json` (gitignored) — the UI reads `pool` from here |
| signer keys | `data/quorum/signer*.key.json`, 0600, gitignored; never read by the UI |

Explorer pages worth a link: the pool account `https://testnet.xrpl.org/accounts/<pool>` (every anchor is a
1-drop Payment pool → sink with a `confidant/anchor` memo), each deposit, each `anchorTx`, each `payoutTx`.

A server deployment (systemd units per node/sidecar/coordinator, nginx with CORS on one public port) is
described in `backend/deploy/README.md`; the shared testnet instance's base URL and pool account are
handed over out of band (never committed). `backend/quorum/smoke.mjs` is a complete client of the
contract — shield, transfer, unshield, reveal against any running coordinator:

```bash
cd backend/quorum && POOL=<pool> COORDINATOR_URL=http://<host>:8787 CLI=../../target/release/examples/cli npm run smoke
```

## 6. Changing the contract

- **Adding** a response field is backward compatible: just ask (issue or PR comment), it lands the
  same day.
- **Renaming/removing** fields or **adding endpoints** goes through a PR touching `backend/`; if the
  change touches a trust path (what a node verifies, what a sidecar signs) it needs an audit slice
  ruling first; a pure API-shape change gets a one-line FYI to the auditor.
- The contract above is frozen for the hackathon unless both leads agree in a PR.

## 7. Pointers

`backend/README.md` (services at a glance), `docs/CORE-API.md` (core ↔ HTTP mapping), `docs/E2E.md` §2b
(the reference client), `docs/NODE-OPERATOR.md` (running a node, recovery), `SPEC.md` §6 (API contract and
trust statements). Russian version of this page: `docs/API-BOUNDARY.ru.md`.
