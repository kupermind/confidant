# Running a Confidant verifier or signer

Anyone can run a verifier: it holds no keys and its state is public. A **signer** is a verifier
whose XRPL key is in the pool account's SignerList; it additionally signs anchor and payout
transactions — only for roots it computed itself.

## What you trust, and what you check yourself

- You trust the soundness of the Zcash Orchard circuit (Halo 2, no trusted setup) and the XRPL
  ledger: the k-signed anchor memos on the pool account are the real, ordered history.
- You verify **every proof and every signature yourself** when you rebuild or accept a bundle, so
  note validity (no forged note, no double-spend) never rests on other operators.
- You **may** re-validate every deposit on-ledger (pool backing). If you defer that to the quorum
  you add no risk beyond the conceded one: collusion of k signers can drain the pool account; it
  cannot spend anyone's notes.

## Join from the ledger (step 2 + 4 of v2)

1. Read the pool account's transactions (`account_tx`, oldest first) and collect the memos of type
   `confidant/anchor` in ledger order.
2. Fetch the bundle bytes from any mirror (`GET /log` of any verifier) — they are addressed by the
   memo's bundle hash, so the mirror cannot substitute anything.
3. For every shield memo, look the deposit up yourself: `tx <depositHash>` must be a validated
   Payment to the pool account; record `delivered_amount` and the `confidant/shield` memo. Put the
   facts in `deposits.json` (`{ "<txHash>": { "txHash", "delivered", "memoHash" } }`).
4. `cli verifier --state data/me.verifier-state.json rebuild memos.json log.json deposits.json`
   (`Verifier::rebuild_checked`): each bundle is hash-checked, proof-verified and applied; each
   shield must be backed by its deposit (`delivered == −valueBalance`, memo == bundle hash, named
   deposit present); your root must equal the last memo's root. Any mismatch — bad mirror, bad
   anchor, unbacked shield — refuses the join. Omitting `deposits.json` (`Verifier::rebuild`)
   re-verifies the cryptography only and trusts the k-signed anchors on backing; a signer must
   not do that.

## Serve

- Accept submissions from any client: `submit-shield` (after you validated the deposit on-ledger
  yourself), `submit-transfer`, `submit-unshield`. Reject = nothing changes.
- Serve your log (`export-log`) so wallets can sync and other nodes can rebuild.

## Sign (step 3 of v2) — never blind

For each proposal from a coordinator you receive `{submission, proposedMemo}`:

1. For a shield, validate the deposit on-ledger yourself.
2. `cli verifier endorse submission.json <memoHex>` — this checks the memo describes exactly the
   submission, runs the full verify tail on **your** state, computes the root the transition
   produces on **your** tree, and applies only if it equals the proposed root. Non-zero exit ⇒ do
   not sign; your state is unchanged.
3. Only on success, multisign the anchor Payment (memo `confidant/anchor` = `memoHex`) and, for an
   unshield, the payout Payment pool → `payout.destination` for exactly `payout.amount`.
4. After the anchor validates, `anchored <root>`.

If you fall behind (missed a proposal), `endorse` refuses; catch up with `rebuild` from the ledger
and the mirror. The reference implementation of a coordinator and three signers is
`e2e/testnet/run.mjs`.


## Keep a second copy of the log

Every node is a full mirror of the bundle bytes (`verifier-state.json`); the ledger holds only their
hashes. A pool whose nodes all run on one host has one copy. Before anything real rides on a pool:
back up `data/quorum` (states *and* signer keys) off the host, and run at least one node on another
machine, joined with `--rebuild --mirror`. A public copy of the log takes one command:
`COORDINATOR_URL=https://<host>/api npm run log:snapshot` (in `backend/quorum`) writes the log in the
shape `--mirror` reads and pins it (a pinning service via `PIN_RPC_URL`+`PIN_TOKEN` or `PINATA_JWT`, else a
local `ipfs` daemon); `--memo` with `OPERATOR_SEED` also records the CID on the ledger (`confidant/snapshot`
memo to the sink). Any node can then rebuild with `--mirror https://<gateway>/ipfs/<CID>`. What survives which failure, and the roadmap for on-ledger and
public mirrors: `docs/DATA-AVAILABILITY.md`.

## Running it as services (`backend/`)

- **Verifier node** (Rust): `confidant-node --listen 127.0.0.1:7001 --state data/n1.verifier-state.json --xrpl-rpc https://s.altnet.rippletest.net:51234 --pool <pool> --signers r1,r2,r3 --quorum 2`; add `--rebuild --mirror http://<any node>/log` to join from the ledger. It holds no keys. It **refuses to start** when the pool's on-ledger SignerList is not exactly `--signers` with `SignerQuorum == --quorum` and the master key disabled; when rebuilding, also when the `account_tx` scan does not start at the pool account's creation (partial-history server), when a memo's bundle is missing from the mirror or hashes differently, when a shield's deposit is missing or short, or when any root differs from its memo.
- **Public log snapshot and its verification:** `npm run log:snapshot` (in `backend/quorum`) writes one JSON of the
  log, pins it (token from the environment) and can publish the CID on the ledger (`--memo`);
  `SNAPSHOT_URL=<gateway URL> NODE_BIN=<confidant-node> [COORDINATOR_URL=…] npm run snapshot:verify` rebuilds a
  throwaway node from the ledger plus that object on a spare port and compares it with the snapshot and the live
  pool — the "what if your server dies" rehearsal as one command (`docs/DATA-AVAILABILITY.md`).
- **Protocol fee** (server-side policy; the same on every node of a quorum): `--fee-shield-bps <S> --fee-unshield-bps <U> --fee-treasury <rT…>` (each ≤ 1000; a treasury is mandatory when either rate is non-zero — the node refuses to start otherwise). Shields are credited net of the shield fee (the wallet builds the net note; the deposit stays gross and is bound with `delivered = gross − fee`), unshields pay out net, transfers are free. The node accounts every fee in `<state>.fees.json` (`GET /fees`: `accrued`, `swept`, `available`) and recomputes it on `--rebuild` from the ledger. **Sweeping to the treasury:** `curl -X POST http://<coordinator>/fees/sweep -d '{}'` moves everything available as a k-of-N Payment pool → treasury; each sidecar signs it only as kind `fee-sweep` — destination = the treasury, no memos, amount ≤ what *its own* node reports available — and each node records the hash only after reading the validated Payment from the ledger itself (`POST /fees/swept {txHash}`; a hash counts once). Independently of its fee ledger, a node caps `available` by `poolExcess` — the pool account's validated balance minus the value of the notes it backs, both read from public data — reports nothing sweepable when the ledger cannot be read, and refuses to record a sweep after which the pool holds less than its outstanding notes. The policy is a **constant of the pool from its genesis**: nodes with different fee flags compute different roots and refuse each other's memos, and a later rate change would make `--rebuild` bind every earlier shield with the wrong net amount and refuse. New rates = a new pool (a fee epoch in the anchor memo is roadmap).
- **Credential gate** (optional; XLS-70, hackathon feature A): `--credential-issuer <r…> --credential-type <hex>
  [--credential-type-text <label>]`. A shield is accepted only if the deposit's sender holds a Credential on the
  ledger from that issuer, of that type, **accepted** by the subject (`CredentialAccept`) and not expired; the node
  reads the entry itself (`ledger_entry` `credential`, validated ledger) at submit and again at endorse, never
  from the client, and fails closed when the ledger cannot be read. Refusals are HTTP 403 naming issuer, type and
  account. `GET /policy` reports the gate (and the screening state); the coordinator copies it into
  `GET /config.credential` for the UI. Same policy on every node of a quorum (`render-env.mjs`:
  `CREDENTIAL_ISSUER`, `CREDENTIAL_TYPE`, `CREDENTIAL_TYPE_TEXT` → `CREDENTIAL_ARGS`). Honest label: gatekeeping on
  a trusted issuer's say-so at the public boundary; it says nothing about the holder's notes.
- **Boundary screening** (optional): add `--screening-list <file>` (one XRPL address per line, `#` comments, or a JSON array) and `--screening-standby-ledgers <n>` to a node. It then refuses — at `/submit/*` and `/endorse`, HTTP 403, fail-closed — shields from listed depositors, shields whose deposit is younger than `n` validated ledgers, and unshields to listed destinations. List entries and destinations are canonicalized to the classic `r…` address (X-addresses decoded, tags dropped), so any spelling of a listed account is caught; a node refuses to start on an unreadable list file and warns at startup about entries that are not XRPL addresses (they block nobody). Build a list from OFAC: `cd backend/quorum && npm run screening:fetch-ofac -- --out ../../data/screening/ofac-xrp.txt [--extra my-list.txt]`. Re-audit any pool from its public log: `npm run screening:audit -- --list <file> --log http://<node>/log`. A refused deposit stays in the pool account; refund it with a quorum-signed Payment to the depositor (manual procedure today). Production standby: ~900 ledgers (1 h); the demo uses 2.
- **Signer sidecar** (Node, `backend/quorum/signer.mjs`): `PORT NODE_URL POOL SINK KEY_FILE XRPL_RPC [MAX_FEE_DROPS]` — holds your XRPL key, signs only what *your* node endorsed and only transactions whose shape matches that endorsement, with Fee ≤ the ceiling, Sequence equal to the pool's next sequence as the sidecar itself reads it (anchor, then payout) and a LastLedgerSequence present and within the window. A `fee-sweep` is signed only against *your* node's `GET /fees` (treasury and available amount), never against the coordinator's numbers.
- **Coordinator** (Node, `backend/quorum/coordinator.mjs`): `PORT NODE_URL NODES SIGNERS QUORUM POOL SINK XRPL_WS` — the public entry point; holds no keys.
- `backend/quorum/setup-pool.mjs` installs the SignerList and disables the master key; `start-all.mjs` launches everything from `data/quorum/config.json`. For a server: `backend/deploy/` (systemd unit templates, nginx site, env renderer, README).

What a sidecar refuses, by construction: an anchor whose memo differs from the one its node endorsed, an anchor not paid pool → sink for 1 drop, a payout whose destination or amount differs from its node's `payout` (the net amount), a fee sweep to anything but the treasury or above the fees its own node has available, any transaction from another account, a Fee above the ceiling, a Sequence that is not the pool's next (or a payout not directly after its anchor), a missing or too-distant LastLedgerSequence, anything when its node refuses. The coordinator's claims never reach a signature — not even "this root is anchored": a node marks a root anchored only after reading the anchor transaction from the ledger itself.

**If a payout fails after its anchor landed** (the note is spent, the user unpaid): the coordinator's answer carries `payoutBlob`, the multisigned payout; resubmit it (`submit` with `tx_blob`) from any machine until its LastLedgerSequence. Nothing else can be paid in its place. **If the coordinator itself is gone:** start another one (it holds nothing) and submit the same `{record, destination}` again — every node's `/endorse` is idempotent for that exact submission, the payout is prepared at the pool's current next sequence, the sidecars re-sign, and it lands.
