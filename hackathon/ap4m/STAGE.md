# Stage sheet — Confidant × AP4M "pay for privacy" (Mastercard track)

Companion to `README.md` (the flow, the live runs, the fallbacks) and `docs/DEMO.md` (the 5-minute script). Measured
facts come from the 2026-10-25 live runs: 68 s wall from the 402 to the settlement receipt, ≈ 44 s with an open
channel; price 0.1 RLUSD under a policy max of 0.25; the over-cap refusal exits with code 3 and signs nothing. Run 3
(2026-10-25, 03:57 UTC) measured the newest parts: the proof-of-funds beat ≈ 28 s before the 402 (≈ 11 s sync + proof,
≈ 10 s the quorum's `CredentialCreate`, ≈ 6 s the agent's `CredentialAccept`), 92 s wall to the settlement including a
12 s pause of the sponsor's service, the signed receipt written at the anchor, `verify-receipt.mjs` ≈ 5 s.

## 1. Stage checklist (run blind, 5 minutes before)

All commands in `hackathon/ap4m` with the `.env` loaded (`set -a; . ./.env; set +a`). `jq` is used to print single
fields so that no seed or key ever reaches the terminal; `node -p` does the same job.

1. **Relay up** (on the demo host, as a service) — expect `{"ok":true,"priceRlusd":"0.1","minPofXrp":"1","pool":"r…",
   "receiptSigner":"0x…","coordinator":"…","spentClaims":n}`; `receiptSigner` must be `MERCHANT_PAYOUT` (the relay
   refuses to start otherwise: receipts are signed by the address the 402 names as `payTo`, key `MERCHANT_PAYOUT_KEY`):

   ```bash
   curl -s "$RELAY_URL/relay/health"
   curl -s "$COORDINATOR_URL/config" | jq '{pool, quorum, signers, credential}'   # the quorum behind it
   ```

2. **Channel id set, so the 24 s channel opening is skipped.** `CHANNEL_ID` in `.env` must be the open 2-RLUSD
   channel; `cap − redeemed` must be ≥ 0.1 (one run) — read it on-chain:

   ```bash
   node -e 'const s=require("./ap4m-hackathon-sdk.cjs"),{ethers}=require("ethers");(async()=>{
     const c=await (await fetch(process.env.AP4M_API_URL+"/public/config")).json();
     const st=await s.getChannelState(new ethers.JsonRpcProvider(c.network.rpcUrl),c.contracts.paymentLedger,process.env.CHANNEL_ID);
     console.log("cap",ethers.formatUnits(st.cap,18),"redeemed",ethers.formatUnits(st.redeemed,18),"left",ethers.formatUnits(st.cap-st.redeemed,18))})()'
   ```

3. **Over-cap prop ready.** Note the id of the 0.15-cap channel that already paid once (0.05 RLUSD left). The
   beat is `OVER_CAP_DEMO=1 CHANNEL_ID=<that channel> node agent.mjs` **after** the main run: it meets the 402,
   reads the channel and stops at `policy: channel has 0.05 RLUSD left < price — NOT paying` (exit 3, nothing
   signed). It builds a bundle first, so afterwards delete the `pending.json` it leaves (it has no `claimId`,
   nothing was paid). Do not open spare channels: the platform caps open channels per team.

4. **State cleared.** `cat "$STATE_DIRECTORY/pending.json"` — if it exists **with** a `claimId`, finish that run
   first (`node agent.mjs` resumes it; deleting it would lose a paid claim); without one, delete it. The relay's
   lock only matters after a crash: stop the relay, `rm "$RELAY_STATE_DIRECTORY/merchant/merchant.lock"` (never
   `merchant.sqlite`), start it, re-run step 1. Never while a merchant process is running, never two merchants
   on one directory.

5. **Agent wallet synced and funded** — `balance` ≥ `AMOUNT` and ≥ `MIN_POF_XRP` in drops:

   ```bash
   curl -s "$COORDINATOR_URL/log?from=0" | jq -c .records > "$LOG_FILE"
   "$CLI" wallet --file "$WALLET_FILE" balance "$LOG_FILE"        # → {"balance":<drops>,"synced":<n>}
   ```

6. **The agent's XRPL account holds the accepted entry credential** (issuer and type from `/config.credential`;
   accepted = `Flags` has bit `0x00010000`). After a run the same list shows the `ConfidantPoF` credential
   (type hex `436F6E666964616E74506F46`, issuer = the pool). The quorum issues **one** proof-of-funds credential
   per borrower; a rehearsal's one is removed by the agent itself (`CredentialDelete` by the subject, one more
   XRPL transaction) before it proves again — nothing to clear by hand, but a few seconds more on stage.

   ```bash
   ACCT=$(jq -r .address "$STATE_DIRECTORY/xrpl.json")       # the address only — the file also holds the seed
   curl -s https://s.altnet.rippletest.net:51234 -d "{\"method\":\"account_objects\",\"params\":[{\"account\":\"$ACCT\",\"type\":\"credential\",\"ledger_index\":\"validated\"}]}" \
     | jq '.result.account_objects[] | {Issuer, CredentialType, Flags, Expiration, URI}'
   ```

7. **Proof of funds armed.** `POF=1` in the agent's `.env` (`POF=0` skips the proof and the relay then refuses).
   The agent proves its **whole** balance (X = step 5's `balance`, window L … L + 21 600 ledgers); `MIN_POF_XRP`
   on the relay must be ≤ that balance in XRP. A refusal is `403 proof of funds required: <why>` with
   `claimKept: true` — the claim is not lost: fix the cause, rerun, the run resumes. Time of the beat (build-pof,
   CredentialDelete, CredentialCreate by the quorum, CredentialAccept, the `attest` signature over the bundle
   hash): **≈ 28 s measured in run 3** (2026-10-25) — budget 30 s before the 402 and say what is happening while it runs.

8. **RLUSD left in the funded allocation.** The budget (20 RLUSD at approval) sits in AP4M's escrow vault; the
   portal's *My setup* (`$AP4M_API_URL/participant`, sign in with the funding wallet) shows what is left and the
   open channels. On-chain, only per channel: step 2's `left`. One stage run costs 0.1 RLUSD plus EVM gas (XRP)
   from the agent and the submitter wallets.

9. **Explorer tabs pre-opened** (the PaymentLedger on the EVM side, the pool account and the agent account on XRPL):

   ```bash
   open "https://explorer.testnet.xrplevm.org/address/$(curl -s "$AP4M_API_URL/public/config" | jq -r .contracts.paymentLedger)"
   open "https://testnet.xrpl.org/accounts/$(curl -s "$COORDINATOR_URL/config" | jq -r .pool)"
   open "https://testnet.xrpl.org/accounts/$ACCT"
   ```

10. **If the AP4M service answers 503.** The kit's merchant says `chain_head_stale` ("retry the same operation")
    while it waits for a new block; the agent waits 5 s and re-presents the same signed payment, up to five times.
    If it still exits, say: *"the payment service is waiting for a block; the signed payment is on disk — we resume
    the same operation"* and run `node agent.mjs` again: `pending.json` makes it resume the same bundle and the same
    purchase name, so nothing is paid twice. A `409 nullifier already spent` on a retry means the transfer **is**
    on the ledger (the pool's newest anchor) and only the answer was lost.

## 2. What a judge can verify, and how

| Artifact | Where it is public | What it proves | What it does not prove |
|---|---|---|---|
| Policy decision — `policy: price 0.1 ≤ max 0.25 RLUSD, channel redeemed so far … — paying`, or `… NOT paying` (exit 3) | The agent's terminal log only | That this code took that decision on those inputs, live — a trace of autonomy for the room | Nothing to a third party: a log line is not a proof; anyone can print one |
| `VoucherRedeemed(channelId, claimId, amount)` | XRPL EVM Testnet (chain 1449000), the PaymentLedger contract — `<EVM explorer>/tx/<hash>` | The agent's channel paid ≥ 0.1 RLUSD for that claim; the relay accepts only what it reads here, never the agent's word | What was bought: the event names a claim, not a transfer; nor that the relay forwarded anything (that is the anchor and the receipt) |
| `ConfidantPoF` credential on the agent's XRPL account (XLS-70) | XRPL testnet — `account_objects` of the agent account; issuer = the pool account; URI `confidant/pof/v1\|X\|L\|E\|digest` | k of N signers each re-verified, on their own node, a proof that the account controlled ≥ X drops of unspent shielded funds as of ledger L; the agent accepted it with its own signature, and signed this bundle's hash with the same account (`attest`), so the relay served the credential's holder, not a bystander | "Controlled ≥ X as of ledger L", **not** "controls it now" — a snapshot that expires at E; not which notes nor how far above X. **Demo-grade:** the proof revealed the proven notes' nullifiers to the signers' nodes, so the quorum can link their later spend |
| Anchor memo on XRPL L1 (memo type `confidant/anchor`; a 1-drop Payment pool → sink) | XRPL testnet, the pool account's history — `https://testnet.xrpl.org/transactions/<hash>`; memo = `version(1) ‖ op(1) ‖ root(32) ‖ bundleHash(32) ‖ …` | k signers of the pool's SignerList accepted a bundle with exactly that hash and that new root; anyone can rebuild the pool state from these memos | **Sender, receiver and amount of the transfer — by design, none of them.** Nor who paid for the relay; the receipt is what binds the payment to the bundle |
| Signed receipt — statement `confidant/ap4m-receipt/v1` `{claimId, channelId, bundleHash, anchorTx, agentAccount, pof, merchantPayout, …}` (canonical JSON, keys sorted) + `node verify-receipt.mjs <file>` | The JSON the agent saves at `$STATE_DIRECTORY/receipts/<bundleHash>.json`; every field points at a public ledger object; the verifier needs no key and no log | The merchant payout key (the 402's `payTo`) signed it; the agent countersigned with its channel (EVM) key and its XRPL account key; the channel's parties are those two addresses; the claim is in `VoucherRedeemed`; the anchor was sent by the pool account and its memo names the bundle hash; the credential with that URI is on the account — exit 0 only when all of it holds | It binds the relay's own forwarding, not the quorum's internals (those are attested by the anchor's k signatures, not by the receipt). A signature alone proves nothing about the facts — the ledger checks do; a receipt that fails them is a false receipt |
| RLUSD payout (`ClaimPaid`, `SettlementRecorded`, the receipt URL) | XRPL EVM Testnet — `<EVM explorer>/tx/<payout>`; the AP4M evidence page | The merchant payout address received the RLUSD for the claim — settlement happened | The metered payment is fully public (channel, merchant, amount, time); the budget sits in AP4M's escrow vault and AP4M executes the settlement — their rail, said plainly |

## 3. AP4M submission text

Paragraph for the sponsor form (897 characters by `wc -m`, limit 900):

```text
Confidant is a compliant shielded pool on the XRP Ledger. An autonomous agent pays for privacy: it builds a zero-knowledge transfer, meets the relay's 402, applies its policy (price 0.1 ≤ max 0.25 RLUSD, cap left on its channel), pays with an x402 voucher on XRPL EVM; the relay forwards the bundle to a k-of-N quorum that anchors it on XRPL L1. Two ledgers, two receipts: the metered payment is public; sender, receiver and amount are not. Before paying, the agent proves shielded funds: the quorum writes an expiring XLS-70 credential on its XRPL account; the relay underwrites from the ledger. A receipt signed by the merchant and countersigned by the agent is re-checked from public data; an over-cap attempt stops at the policy. Limits: federated quorum (k signers can drain the pool account, never spend a note); proof of funds demo-grade (nullifiers reach the signers' nodes); testnet only.
```

Evidence links:

- `<VoucherRedeemed tx>` — XRPL EVM Testnet, the PaymentLedger event for the stage run
- `<anchor tx>` — XRPL testnet, the pool account's anchor memo for the stage run
- `<payout tx>` — XRPL EVM Testnet, the RLUSD settlement for the stage run
- `<PoF credential account>` — XRPL testnet, the agent's account holding the `ConfidantPoF` credential
- The receipt file of the stage run (`receipts/<bundleHash>.json`), attached; re-check: `node verify-receipt.mjs <file>`
- Earlier run 1 (2026-10-25): VoucherRedeemed `https://explorer.testnet.xrplevm.org/tx/0xb9aaa78632e07d33c9c66b3f0a28be92ef160234c515787f2357b9eb30cad7df` · anchor `https://testnet.xrpl.org/transactions/533B62011D10B35C6B6DB92F0B4AD50888372DBBBB880D4638F3CA76652B0100` · payout `https://explorer.testnet.xrplevm.org/tx/0x3af20d2ae569bb7ab816d9c75a49df44fae5dc8bfac8d2631088cf9b01f84671`
- Earlier run 2 (2026-10-25, 0.15-cap channel): anchor `2104CB64…` · payout `0x326f3830…` (truncated in the README; full hashes in the run log)
- Repository: `hackathon/ap4m/README.md` (flow, live runs, honest labels), `docs/DEMO.md`, `docs/SUBMISSION.md`

## 4. One slide

**Title:** Confidant × AP4M — pay for privacy: an agent buys the relay of a shielded XRPL transfer

1. **Prove** — the agent proves ≥ X XRP of shielded funds; the quorum writes an expiring `ConfidantPoF` credential on its XRPL account (demo-grade)
2. **402** — `POST /relay/transfer` without a claim → `402 { price 0.1 RLUSD, PaymentLedger, chain 1449000 }`
3. **Decide** — policy: price ≤ 0.25 RLUSD and cap left on the channel → pay; otherwise stop (exit 3, nothing signed)
4. **Pay + relay** — x402 voucher redeemed on XRPL EVM (`VoucherRedeemed`); the relay checks claim and credential on the ledgers, forwards once; k of N signers anchor on XRPL L1 (memo: root, bundle hash — no sender, receiver, amount)
5. **Settle + receipt** — RLUSD payout to the merchant; receipt signed by the merchant payout key, countersigned by the agent's channel key and XRPL key, saved as JSON

Footer 1: *the metered payment is public, the settlement is private; the proof of funds is demo-grade*

Footer 2: `node verify-receipt.mjs agent-state/receipts/<bundleHash>.json` — re-checks both signatures, `VoucherRedeemed`, the anchor memo and the credential from public data only
