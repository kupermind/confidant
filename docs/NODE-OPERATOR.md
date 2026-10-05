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
