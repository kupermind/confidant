# Confidant × AP4M — "pay for privacy" relay (Mastercard track, hackathon only)

A scripted **agent** pays **RLUSD on XRPL EVM Testnet** (chain 1449000) through Mastercard's AP4M for the **relay** of a
zero-knowledge transfer that Confidant's k-of-N quorum then anchors **on XRPL L1 testnet**. Two ledgers, two receipts:
the metered payment is public, the settlement of the transfer is private.

**What is and is not touched.** The relay is a fail-closed paywall *in front of* the coordinator's existing
`POST /submit/transfer`. Core, verifier nodes, signer sidecars and the coordinator's trust path are unchanged; the relay
holds no Confidant keys and never signs on XRPL L1 — so no audit slice (ROADMAP §0 ship rule: a read gate, not a signing
path). The relay fee is a **metering fee**, separate from the pool's protocol fee, and not a privacy feature.

**Hackathon-only SDK.** The AP4M kit is licensed "for hackathon use only" and is **not vendored**: `npm install` fetches
`@ap4m/sdk` from the hackathon service and `setup.sh` downloads the two helper files next to the scripts (gitignored).

## Layout

| File | Role |
|---|---|
| `relay.mjs` | x402 Merchant (the kit's `createX402Merchant`, unmodified, sells a static "relay ticket" at `GET /resource`) **+** `POST /relay/transfer { record, channelId, claimId }`: verifies the claim **on-chain** (a `VoucherRedeemed` for that channel and claim at the PaymentLedger, amount ≥ price) and **unused** (spent-set in the state directory), forwards the bundle to the coordinator, returns the anchor. Missing / unknown / reused claim → `402`. The `VoucherRedeemed` scan looks back 200 000 blocks from the chain head (≈ the event's lifetime); an older claim reads as "not found". The spent-set lives on disk per relay process — one relay instance per deployment. |
| `agent.mjs` | Builds a private transfer with the native Confidant wallet (CLI), meets the `402`, applies the **payment policy** (price ≤ `MAX_PRICE_RLUSD`, channel cap remaining), pays with `buyResource` on its Channel, retries with the `claimId`, calls `settle`, prints both explorer links. The operation name is derived from the bundle hash: a rerun resumes, never pays twice. |
| `setup.sh` | `npm install` + download of `ap4m-hackathon-sdk.cjs` and `ap4m-x402.mjs` from the service. |
| `prepare-wallet.mjs` | One-time pool side for the agent: two native wallet files (agent, supplier — keys stay in the state directory), a faucet XRPL account, waits for the gate's issuer to write its credential and accepts it, shields `DEPOSIT_XRP` through the live coordinator, prints `SUPPLIER` / `WALLET_FILE` / `LOG_FILE` for `.env`. |
| `env.example` | Every variable; copy to `.env` (gitignored) and fill from the portal. **Never commit keys.** |

## Run

```bash
cd hackathon/ap4m && ./setup.sh && cp env.example .env   # fill AGENT_PRIVATE_KEY, MERCHANT_*, TEAM_NAME, SUPPLIER …
set -a; . ./.env; set +a
npm run prepare-wallet   # once: the agent's shielded notes on XRPL L1 (needs the gate's issuer to write one credential)
node relay.mjs            # :4308 ticket merchant, :4309 relay → COORDINATOR_URL
node agent.mjs            # one run = one 402 → pay → relay → settle, both receipts printed
```

Prerequisites: Node ≥ 22.13; the team onboarded (below); `target/release/examples/cli` built; a Confidant wallet file
with shielded notes (`WALLET_FILE`, created with `cli wallet --file … init` and funded by a shield — the e2e harness
shows how).

## Onboarding (what the workshop deck and the portal say)

1. **Register = the organizer's form.** Its "XRPL EVM Wallet Address for Funding" is the team's **funding wallet**: an
   EVM EOA (`0x…`, XRPL EVM Testnet, chain `1449000`) that **signs in** to the team portal ("funding or Agent wallet"),
   **funds the Agent's budget** and **receives what is left after the event**. The guide allows the Agent wallet to be
   the funding wallet, so one EOA can do both; the Merchant payout wallet must differ from it.
2. **Approval auto-provisions and funds 20 RLUSD** (deck: "approve → auto-provision + 20 RLUSD funded"); no booth visit
   is needed for the budget. Gas: XRP from `https://faucet.testevm.ripple.com`.
3. **Sign in** with the funding wallet at `hackathon.ap4m.build/participant`; on *My setup* add the **Agent** wallet (if
   different) and the **Merchant payout** wallet (own EOA — "keep the key, it's how you access your winnings" — or a
   hackathon pool wallet). Note `TEAM_NAME`, `MERCHANT_ID`, the payout address.
4. **Open a channel** (the agent does it with `CHANNEL_CAP`, or pick an open one as `CHANNEL_ID`), then pay.

Walk-in sign-up (no form answered) needs a connected browser wallet that signs the sign-up message; staff approve.

## Demo (a 5-minute slot; the flow itself in under 3 — the deck's "what a strong demo proves": control, payment, proof)

1. **Control.** "Two verified parties: the agent is registered in AP4M's registry (KYA), the merchant is our relay; the
   channel has a funded cap." Terminal: wallet synced, bundle built. "This agent holds shielded funds on the XRP Ledger
   and needs to pay a supplier without publishing it. Privacy is a metered service: it pays per relay, in RLUSD."
2. **Payment.** The `402` and the policy line: "it reads the price, checks its channel, decides to pay — no human."
   Voucher signed; the relay releases the resource at once.
3. **Proof.** EVM explorer: `VoucherRedeemed` at the PaymentLedger; then XRPL testnet explorer: the pool account's new
   anchor Payment — no sender, no receiver, no amount. "Two ledgers, two receipts."
4. **Settlement.** `settle` → RLUSD to the merchant payout (`ClaimPaid` + `SettlementRecorded`), the receipt URL.
5. **Over-cap attempt rejected.** With `OVER_CAP_DEMO=1` the agent tries a second relay against what is left of the cap
   (demo channel `CHANNEL_CAP=0.15`, price `0.1`): the policy refuses — "0.05 RLUSD left < 0.1, not paying" — and the
   ledger would refuse the voucher anyway (`settled ≤ redeemed ≤ cap`). Say: "the limit is funded and enforced by the
   contract, the agent only decides inside it."

Judging (deck): real-world utility 25 %, trust & control 25 %, technical execution 25 %, path to production 15 %,
demo & story 10 %. The web guide's rubric adds: autonomy (the agent reads the 402 and decides), use-case clarity (the
payment is load-bearing), on-chain proof (`VoucherRedeemed` at the PaymentLedger), RLUSD settlement (the payout wallet's
balance changed), demo quality (live, no recordings, the flow in 3 minutes), creativity.

**Path to production (one paragraph for the judges).** The relay is a metered gateway in front of an API that already
runs on testnet with a k-of-N quorum; the same x402 exchange works against AP4M's production rails when they exist. The
pool side keeps its own path (`ROADMAP.md`): RLUSD *inside* the pool (deposit/payout for issued tokens), the signing
path's subject-consent binding, and on-ledger verification when XRPL offers it. The relay fee would become the
operators' revenue line, separate from the pool's protocol fee.

Fallbacks: AP4M side down → the pool demo (`docs/DEMO.md`) plus the rehearsal receipts, said plainly; EVM faucet dry →
the pre-funded rehearsal wallets; channel exhausted → the second channel opened in advance.

## Honest labels

Hackathon SDK on a testnet with testnet RLUSD. The pool account is federated (k-of-N): collusion of k signers can drain the
pool account, never spend a note; shield/unshield endpoints are public; an empty pool has no privacy. RLUSD **inside** the
pool is roadmap — the core is token-agnostic, the deposit/payout surface for issued tokens is not built; never say "RLUSD
support". On the AP4M side the Agent key stays with the team; the RLUSD budget sits in AP4M's escrow vault and the AP4M
service executes settlement — their design, said plainly if asked.
