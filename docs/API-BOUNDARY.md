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
| 409 | refused by the verifiers | nullifier already spent (replay / double spend), memo mismatch, unknown anchor, `only 1/3 signers endorsed (quorum 2)`; fee sweep above the available fees / already recorded |
| 422 | the ledger does not back the claim | deposit not validated / not to the pool / wrong amount / no `confidant/shield` memo; anchor tx does not carry the root |
| 403 | refused by a node's screening policy (optional, per node) | `account r… is on the screening list` (final); `deposit … younger than the standby … retry at ledger N` (retry later) |
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
| `GET /config` | — | `{ network, explorer, xrplWs, pool, sink, quorum, signers, fees: { shieldBps, unshieldBps, treasury }, credential: { issuer, type, typeText } \| null, nodes: [url…] }` — public facts the UI needs; no secrets. `nodes` are the URLs a browser reaches each verifier node at (default `/node/<i>` on the UI origin), for the proof-of-funds check |
| `GET /fees` | — | `FeeReport` of the coordinator's node |
| `GET /policy` (node) | — | `{ credential: { issuer, type, typeText } \| null, screening: { active, standbyLedgers } }` — the deposit gates the node enforces; the coordinator copies `credential` into `GET /config` |
| `POST /fees/sweep` | `{ amount? }` (drops; default: everything available) | `{ amount, tx, treasury, recorded, refusals }` — operator call, k-of-N Payment pool → treasury |
| `POST /pof/verify` (node, reached as `/node/<i>/pof/verify` — never through the coordinator) | `{ record, amount, challenge: { borrower, lender, ledgerIndex, expiry }, credential?: bool }` | `{ ok, amount, challenge, validatedLedger, root, bundleHash, meaning, credential: { subject, credentialType, credentialTypeText, expiration, uri, uriText } \| null, credentialRefused: string \| null }` — proof of funds checked read-only on that node's own state; never logged or persisted. With `credential: true` the node also derives the XLS-70 Credential it would let a sidecar sign, **after its own bounds** (borrower eligible under the pool's gate when on; no proof-of-funds credential from this pool yet; pool's outstanding ones under the cap of 8) — otherwise `null` with the reason. 400 challenge refused · 409 the proof does not verify · 413 body > 256 KiB · 429 node busy · 502 no validated ledger / ledger unreadable |
| `POST /pof/credential` | `{ record, amount, challenge }` | `{ tx, issuer, subject, credentialType, credentialTypeText, expiration, uri, amount, challenge, validatedLedger, signers, refusals }` — the quorum records a verified proof as an expiring `CredentialCreate` from the pool account to the borrower; every sidecar re-verifies on its own node at signing time. 409 the node withholds the credential (not eligible / already holds one / cap) or fewer than k signers verified · 413 body > 1 MiB · 502 ledger |

Shapes (camelCase on the wire):

```jsonc
PoolState   { root, anchoredRoot | null, leafCount, nullifierCount, bundleCount, consumedDeposits }
Accepted    { bundleIndex, firstLeaf, newRoot, bundleHash, nullifiers[], commitments[], fee, payout?: { destination, amount } }
FeeReport   { shieldBps, unshieldBps, treasury | null, accrued, swept, available, poolExcess | null, outstandingBacking }   // available = min(accrued − swept, poolExcess); poolExcess = pool balance − outstanding notes, from the ledger
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
- **Protocol fee (set on the server, shown by the UI).** `GET /config.fees` gives the rates in basis
  points. **Shield:** the deposit is the gross amount the user typed; the UI must build the note for
  `gross − floor(gross · shieldBps / 10000)` and deposit `gross` — a note built for any other amount is
  refused with 422 (`delivered == value_balance`). **Unshield:** the note burns the amount the user
  typed; `payout.amount` in the answer is the net amount the destination receives
  (`gross − floor(gross · unshieldBps / 10000)`); show both. **Transfers are free.** `Accepted.fee` is
  the fee the quorum took, in drops. Fees are never configured, shown as editable, or paid separately
  in the UI; the treasury is public and the whole fee ledger is verifiable from the ledger.
- **Total shielded (shown by the UI, public).** `GET /fees.outstandingBacking` is the value of every note
  the pool backs, in drops (shields in minus unshields out, from the public value balances). It is
  public by construction — every shield and unshield is a Payment on the ledger — so the UI shows it
  as "Total shielded — N XRP (backed, public)" next to the holder's private balance, never the other
  way round. It is the note-backing total, not the pool account's whole XRPL balance (which also holds
  operator funding, unswept fees and refused deposits awaiting refund), and the tooltip says so; only
  how the total is split among holders, and who paid whom inside, is private.
- **Pool account balance vs. shielded total.** The UI shows the total as a headline stat in the header (left of the
  status pill) with a second line from the same `GET /fees` answer, and `frontend/balances.html` reconciles both
  figures live from the ledger with a FAQ (memo, stray XRP, fees, float, anchors). The line:
  "pool account B XRP = `outstandingBacking` shielded + `poolExcess` operator float and unswept fees"
  (B = the two added; `poolExcess` is the node's own read of balance − backing, `null` when the ledger is
  unreadable). The excess is the operator's funding at genesis, protocol fees not yet swept to the treasury,
  and any refused deposit awaiting refund — never note value.
- **Shield recovery.** Once the deposit Payment is validated the UI keeps the record and the deposit hash locally
  until the anchor is confirmed; after a lost answer or a reload it offers "Resume anchoring", which resubmits the
  same record to `/submit/shield`. The node is idempotent for the bundle that is its last record; a 409 (nullifier
  already spent) on the resubmit means the shield was applied earlier and the UI treats it as done.
- **The viewing key is the authorization for `/audit/reveal`** and reveals that holder's whole history
  (read-only, nothing else). The UI sends it only to the coordinator the user chose, over the URL the
  user configured, and says so before sending; it never stores or forwards it elsewhere. CORS is `*`
  because the coordinator holds nothing a foreign origin could take — the key stays the user's risk.

A complete client of this contract, including every negative case, is `e2e/testnet/run-services.mjs`.
Copy from there.

### Credential gate (hackathon feature A) — the contract between the node and the holder side

*Proposed 2026-10-24 (event day); implemented the same day on both sides (`backend/node/src/credential.rs`,
`frontend/issuer.html`, the app's Compliance row). The node reports its gate through `GET /policy`, which the
coordinator copies into `/config.credential`.*

- **Config.** `GET /config` gains `credential: { issuer: "r…", type: "<hex>", typeText: "ConfidantKYC",
  expiresAfter?: <seconds> } | null`. `null` (or absent) means the gate is off and the UI shows nothing.
  The UI reads the issuer and the type from here and from nowhere else.
- **Rule.** A shield is accepted only if the deposit's `Account` holds an XLS-70 Credential on the ledger
  with `Issuer == credential.issuer`, `CredentialType == credential.type`, the **accepted** flag set
  (`lsfAccepted`) and no past `Expiration`, read by each node from the ledger itself (`ledger_entry`
  `credential` or `account_objects` of the depositor) at submit and again at endorse. The sidecar signs
  nothing its own node did not accept (unchanged rule).
- **Refusal.** HTTP **403** with `{ error: "no accepted credential from <issuer> of type <typeText> for
  <account>" }` — the same status family as the screening gate; the UI tells them apart by the word
  `credential`. An issued-but-not-accepted Credential is refused too (the subject must accept it: that
  signature is what binds the account).
- **Holder side (UI).** The wallet card shows a *Compliance* row: `verified ✓` / `verified — press Confirm`
  / `not verified — how to get verified`; *Confirm* signs `CredentialAccept { Account: holder, Issuer,
  CredentialType }` with the holder's XRPL account in the browser (xrpl.js); a refused shield links to
  the issuer page. Until the node side ships `credential` in `/config`, the issuer page hands the holder over with
  `/?accept=<issuer>:<typeHex>`, which the app keeps in storage and treats as the gate's config (demo path only).
  The demo issuer is a static page (`frontend/issuer.html`): a faucet-funded XRPL
  account playing the authorized compliance provider (a KYC/AML firm in production), which signs `CredentialCreate { Account: issuer, Subject: holder,
  CredentialType, Expiration? }` for any address pasted in and prints its own address and the type for
  the node operators' config. Honest label: the issuer is a demo persona; the gate is real on the ledger.
- **Browser library.** XLS-70 transaction types are absent from xrpl.js 4.3.0, which the app loaded; the
  holder page and the issuer page use **xrpl.js 5.3.0** (the same version as `backend/quorum`).

### Proof of funds (hackathon feature C, Tier 1 — the read side)

*Built 2026-10-24 (event day) on the core slice (`core`: `OpTag::ProofOfFunds`, domain `ConfidantPoF_v1_`). Node side:
`backend/node/src/pof.rs` and `POST /pof/verify`; holder side: the wasm export `buildProofOfFunds(amount, challenge)` and
the worker op `pof({ amount, challenge })` (one name on every surface: worker op, wallet-service route, node endpoint). The `credential-pof` signing path (the quorum writing an expiring
Credential on the borrower's account) is a separate slice under the ship rule in `docs/PROOF-OF-FUNDS.md` §6.*

- **Challenge.** The lender fills `{ borrower, lender, ledgerIndex, expiry }`: `borrower` is the borrower's classic XRPL
  address (X-addresses refused, so both sides sign the same bytes); `lender` an opaque id — a name, an account, a request
  id — of 1–64 printable characters without `|`; `ledgerIndex` the validated ledger index the lender saw when issuing the
  challenge; `expiry` a ledger index after it and at most 21 600 ledgers (≈ 24 h) later. Its **canonical bytes** — what
  the holder's wallet signs over and what every node recomputes from the fields — are the UTF-8 string
  `confidant/pof/v1|<borrower>|<amount>|<ledgerIndex>|<expiry>|<lender>`. The UI builds exactly this string and passes it
  to `pof({ amount, challenge })`; `amount` is in drops.
- **Verify.** The lender — or the holder on the lender's behalf — POSTs `{ record, amount, challenge }` to **each node**
  at a URL from `/config.nodes` (`/node/<i>/pof/verify` on the UI origin). The coordinator is not in the path and never
  sees the record. The node validates the fields, judges freshness against the validated ledger index **it reads itself**
  (`ledgerIndex` within 200 ledgers of it, `expiry` after it), recomputes the bytes and runs the read-only check on its
  own state. Answer: `{ ok: true, amount, challenge, validatedLedger, root, bundleHash, meaning }` — `root` is that node's
  state the proof was checked against, `meaning` the honest label to show verbatim. Errors: **400** `proof of funds:
  challenge refused: …` (a field or the freshness window), **409** the proof does not verify against this node's state
  (another amount, another challenge, a pool record, notes spent since), **413** body over 256 KiB, **429** the node is
  busy (four verifications at a time — retry), **502** the node cannot read the validated ledger.
- **Never logged, never persisted.** The record carries the holder's nullifiers; the node keeps no copy and writes nothing
  about the request, not even the bundle hash. The UI must not send it to the coordinator and must not keep it beyond the
  session; the holder's wallet should rotate the proven notes to itself afterwards (the nodes can otherwise link the later
  spend — `docs/PROOF-OF-FUNDS.md` §1).
- **Honest label (lender surface, verbatim from `meaning`):** "someone controlling at least `amount` in this pool answered
  this challenge as of the validated ledger: an underwriting signal, a snapshot, not a lock, and not proof that the borrower
  owns the notes." k verdicts from N nodes are k independent reads of the same ledger-reconstructible state; the lender
  decides how many it wants. Nothing reaches the ledger in this slice.
- **Holder side (UI, built 2026-10-24).** A fourth tab, **Prove funds**: amount, borrower (prefilled with the holder's
  XRPL account), lender id, validity (1 / 6 / 24 h → 900 ledgers per hour). The app reads the validated ledger index
  itself, builds the challenge and the canonical string, proves in the browser (`pof`), POSTs
  `{ record, amount, challenge }` to **every** URL in `/config.nodes` and shows a per-node verdict table (✓ / the
  refusal, the ledger judged at, that node's root), the honest label, and the **proof package**
  `{ amount, challenge, record, bundleHash }` to copy or download for the lender. It then offers to **rotate the notes**
  (a transfer to the holder's own address, pre-filled on the Transfer tab) so the nodes cannot link the next spend to the
  proof. The package is never sent to the coordinator and is not stored.
- **Lender page (`frontend/lender.html`, built 2026-10-24).** Step 1 drafts the request to the borrower (lender id,
  minimum amount, validity). Step 2 takes the pasted or uploaded package, checks its shape, shows what it claims
  (amount, borrower with an explorer link, lender id — flagged when it is not the lender's own, the ledger window and
  how long it is still valid), POSTs to every node in `/config.nodes`, and prints the verdict table with "k of N nodes
  verified" and the nodes' `meaning` verbatim; a 400 (stale / expired) and a 409 (does not verify) are explained. A
  "told / not told / not a lock" card closes the page. `?demo=1` fakes four nodes (one busy, 429) and loads a demo
  package.
- **On the ledger (signing path, built 2026-10-24 — ships only with its own IMPL-PASS).** When k of N nodes verified, the
  Prove funds tab offers *record it on the ledger as a Credential*: `POST /pof/credential` with the same body. The answer
  names the `CredentialCreate` transaction and the fields; the app then offers **Confirm with my signature** —
  `CredentialAccept { Account: borrower, Issuer: pool, CredentialType }` signed in the browser by the holder's XRPL account
  (the account named in the challenge; the app refuses to confirm for another account). Both land in Activity. The
  Credential's `Expiration` is `close_time(L) + 4 s × (E − L)`, its `URI` is the text
  `confidant/pof/v1|X|L|E|<digest16>` — every node derives both, so sidecars sign only what their node computed. The
  lender page's step 3 reads the borrower's credentials from the ledger (`account_objects type credential`, issuer = the
  pool, type `ConfidantPoF`), reports accepted / issued / expired with the expiry time, decodes the URI and says whether it
  matches the package it verified (the nodes' `credential.uri` from the verdicts). Honest label on both surfaces: the
  credential says "controlled ≥ X as of ledger L", not "controls it now".

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
| **Shield** | 1. `cli wallet --file u.wallet.json build-shield <drops> shield.json log.json` → prints `{ bundleHash, file }`. 2. The user's XRPL account sends a `Payment` of exactly `<drops>` to the pool account with one memo `{ MemoType: hex("confidant/shield"), MemoData: <bundleHash hex> }`; wait for validation. 3. `POST /submit/shield { record: <shield.json>, depositTxHash }`. **Build first, deposit second** — the memo must commit to the bundle; the amount must equal `delivered_amount`. A 403 mentioning the standby means "too fresh": retry after the named ledger (the demo quorum uses 2 ledgers ≈ 8 s); a 403 naming the screening list is final — the deposit stays in the pool account until the operators refund it. |
| **Transfer** | sync → `cli wallet --file u.wallet.json build-transfer <recipientAddressHex> <drops> transfer.json log.json` → `POST /submit/transfer`. Nothing else reaches the ledger but the anchor. |
| **Unshield** | sync → `cli wallet --file u.wallet.json build-unshield <drops> <rDestination> unshield.json log.json` → `POST /submit/unshield` **with the same destination** (the signature inside the bundle binds it; another destination is refused). Default to a **fresh XRPL destination** for every unshield and suggest depositing from a fresh account: the pool's entry and exit addresses are public, and reuse lets chain analysis cluster one user's boundary events (ROADMAP §4.6). |
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
handed over out of band (never committed). Once the TLS site is up (`backend/deploy/README.md`, "HTTPS
on a domain") the public base URL is `https://confidant.im/api`; the browser UI uses its own origin's `/api/`. `backend/quorum/smoke.mjs` is a complete client of the
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
- 2026-10-11: protocol fee added (`Accepted.fee`, net `payout.amount`, `GET /config.fees`,
  `GET /fees`, `POST /fees/sweep`); the UI builds the shield note net of the fee (§2).
- 2026-10-21: `GET /fees.outstandingBacking` (the public pool total); the UI shows it in the status
  bar and under the private balance (§2).

## 6a. The prototype (2026-10-10)

`frontend/index.html` is a working first version for the FE team to start from: one static page, no
build step. The wallet is **in the browser**: `frontend/wasm` (confidant-core behind wasm-bindgen,
built with `wasm-pack build frontend/wasm --target web --out-dir ../pkg`) runs in
`frontend/wallet-worker.js`; the page keeps the `WalletSecret` JSON in `localStorage`, syncs from
`GET /api/log`, and the worker builds `{record, bundleHash}` for `POST /api/submit/*`. Two builds of
the same crate: `pkg-threads/` proves on a Web-Worker thread pool and is picked when the page is
cross-origin isolated (the nginx site sends COOP/COEP), `pkg/` is the single-threaded fallback. The
proving key is built once per worker, in the background right after the wallet exists (`warm`), so
the first operation does not wait for it; a build's answer carries `provingMs` and `threads`, which the
page shows ("proved in X s on N threads").

**Browser proving — what to budget for, and what not to change** (measured 2026-10-23 on the live site,
Chrome, 20-core laptop, cross-origin isolated, `performance.measureUserAgentSpecificMemory`): the wallet
worker holds ≈ 70 MB once the proving key is built and ≈ 135 MB after the first proof — WebAssembly
linear memory does not shrink, so that is the session's working set; the threaded build declares a
2 GiB maximum for its shared memory (`frontend/wasm/build-threads.sh`), far above need. Times: proving
keys 6.0 s and a 2-action proof 3.1 s on 16 threads; ≈ 12 s and ≈ 16 s single-threaded. Three facts keep
this cheap, and the UI must not change them: (1) the circuit is the Zcash Orchard Action circuit reused
unchanged — its size (`k`) is fixed by Orchard, there is nothing to tune; (2) **no non-native curve
arithmetic runs inside the proof** — spend authorization is RedPallas on Pallas, native to the circuit;
every XRPL signature (secp256k1 / ed25519) is made by `xrpl.js` outside the proof, and the deposit is
bound by the memo (the bundle hash) that every node checks against the ledger — so lookup-table curve
libraries (PSE / Axiom style) are not needed and must not be added; (3) proving always runs in
`wallet-worker.js`, never on the main thread. Threads need a cross-origin-isolated page: the COOP/COEP
headers *and* a secure context (HTTPS, or `http://localhost`); over plain `http://<ip>/` browsers ignore
the headers and the wallet silently runs single-threaded (the wallet card says "N threads"). The default
allocator is used; `wee_alloc` is unmaintained and not worth its known leaks. Pre-population for a demo: `npm run prepopulate` in
`backend/quorum` (fixed denominations, faucet-funded holders, a few private hops — a synthetic set, say
so); the stage run-sheet is `docs/DEMO.md`. `xrpl.js` in
the browser does the faucet and the deposit. Fallback when WebAssembly is unavailable: the demo
wallet service (`backend/wallet`: `POST /wallets`, `GET /wallets/{id}`, `POST
/wallets/{id}/{shield|transfer|unshield}`), which holds the demo wallets' spending keys on the demo
host — label it so. **Testnet demo only, and it must not carry over to real value:** a custodial wallet service is never the path for a mainnet or real-XRP pool; the browser (WASM) wallet is the production path and the service does not graduate with it. `?demo=1` renders the page on simulated data. nginx site:
`backend/deploy/nginx/confidant-ui.conf` (port 80). Smoke of the exact UI path without a browser:
`npm run smoke-ui` in `backend/quorum`.

## 7. Pointers

`backend/README.md` (services at a glance), `docs/CORE-API.md` (core ↔ HTTP mapping), `docs/E2E.md` §2b
(the reference client), `docs/NODE-OPERATOR.md` (running a node, recovery), `SPEC.md` §6 (API contract and
trust statements). Russian version of this page: `docs/API-BOUNDARY.ru.md`.
