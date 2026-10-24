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
| `relay.mjs` | x402 Merchant (the kit's `createX402Merchant`, unmodified, sells a static "relay ticket" at `GET /resource`) **+** `POST /relay/transfer { record, channelId, claimId }`: verifies the claim **on-chain** (a `VoucherRedeemed` for that channel and claim at the PaymentLedger, amount ≥ price) and **unused** (spent-set in the state directory), forwards the bundle to the coordinator, returns the anchor. Missing / unknown / reused claim → `402`. |
| `agent.mjs` | Builds a private transfer with the native Confidant wallet (CLI), meets the `402`, applies the **payment policy** (price ≤ `MAX_PRICE_RLUSD`, channel cap remaining), pays with `buyResource` on its Channel, retries with the `claimId`, calls `settle`, prints both explorer links. The operation name is derived from the bundle hash: a rerun resumes, never pays twice. |
| `setup.sh` | `npm install` + download of `ap4m-hackathon-sdk.cjs` and `ap4m-x402.mjs` from the service. |
| `env.example` | Every variable; copy to `.env` (gitignored) and fill from the portal. **Never commit keys.** |

## Run

```bash
cd hackathon/ap4m && ./setup.sh && cp env.example .env   # fill AGENT_PRIVATE_KEY, MERCHANT_*, TEAM_NAME, SUPPLIER …
set -a; . ./.env; set +a
node relay.mjs            # :4308 ticket merchant, :4309 relay → COORDINATOR_URL
node agent.mjs            # one run = one 402 → pay → relay → settle, both receipts printed
```

Prerequisites: Node ≥ 22.13; the team onboarded on the portal (Agent wallet, Merchant payout wallet, budget funded, XRP
for gas from the EVM faucet); `target/release/examples/cli` built; a Confidant wallet file with shielded notes
(`WALLET_FILE`, created with `cli wallet --file … init` and funded by a shield — the e2e harness shows how).

## Demo (3 minutes, live)

1. Terminal: wallet synced, bundle built. "This agent holds shielded funds on the XRP Ledger and needs to pay a supplier
   without publishing it. Privacy is a metered service: it pays per relay, in RLUSD, through Mastercard's AP4M."
2. The `402` and the policy line: "it reads the price, checks its channel, decides to pay — no human."
3. EVM explorer: `VoucherRedeemed` at the PaymentLedger.
4. XRPL testnet explorer: the pool account's new anchor Payment — no sender, no receiver, no amount.
5. Portal / EVM explorer: RLUSD settled to the merchant payout; the verifiable receipt.

Fallbacks: AP4M side down → the pool demo (`docs/DEMO.md`) plus the rehearsal receipts, said plainly; EVM faucet dry →
the pre-funded rehearsal wallets; channel exhausted → the second channel opened in advance.

## Honest labels

Hackathon SDK on a testnet with testnet RLUSD. The pool account is federated (k-of-N): collusion of k signers can drain the
pool account, never spend a note; shield/unshield endpoints are public; an empty pool has no privacy. RLUSD **inside** the
pool is roadmap — the core is token-agnostic, the deposit/payout surface for issued tokens is not built; never say "RLUSD
support". On the AP4M side the Agent key stays with the team; the RLUSD budget sits in AP4M's escrow vault and the AP4M
service executes settlement — their design, said plainly if asked.
