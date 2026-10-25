// Confidant × AP4M agent — pays for the relay of a private transfer, autonomously (hackathon track only).
//
//   1. sync the Confidant wallet (native CLI), build a transfer bundle to SUPPLIER (an Orchard address)
//   2. POST /relay/transfer { record }                        → 402 { pay: { priceRlusd, … } }
//   3. policy: price ≤ MAX_PRICE_RLUSD and the channel's remaining cap ≥ price → pay; otherwise stop and say why
//   4. buyResource() on the open Channel — the kit's x402 purchase; the relay redeems the Voucher on-chain
//   5. POST /relay/transfer { record, channelId, claimId }    → 200 { anchorTx, payment }
//   6. settle() — wait for the RLUSD payout to the merchant and print the receipt
//   7. print both receipts: EVM explorer (VoucherRedeemed, settlement) and XRPL testnet (the anchor memo)
//   8. OVER_CAP_DEMO=1: try a second relay against what is left of the channel cap — the policy refuses when the
//      remaining cap is below the price (the ledger enforces settled ≤ redeemed ≤ cap and would refuse the voucher too)
//
//   Operation names are derived from the bundle hash, so a rerun resumes the same purchase instead of paying twice.
//
//   Env: AGENT_PRIVATE_KEY, TEAM_NAME, AP4M_API_URL (default https://hackathon.ap4m.build), RELAY_URL
//   (default http://127.0.0.1:4309), RESOURCE_URL (default http://127.0.0.1:4308/resource), CHANNEL_ID (or CHANNEL_CAP
//   to open one), MAX_PRICE_RLUSD (default 0.25), CLI (path to confidant's cli), WALLET_FILE, LOG_FILE (log.json synced
//   from the coordinator), SUPPLIER (Orchard address hex), AMOUNT (drops), COORDINATOR_URL, STATE_DIRECTORY, AP4M_RPC_URL
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ethers } from "ethers";
import hackathonSdk from "./ap4m-hackathon-sdk.cjs";
import { buyResource, loadX402Context } from "./ap4m-x402.mjs";

const { signIn, requestChannel, settle, getChannelState } = hackathonSdk;
const env = process.env;
const AP4M_API_URL = (env.AP4M_API_URL ?? "https://hackathon.ap4m.build").replace(/\/$/, "");
const RELAY_URL = (env.RELAY_URL ?? "http://127.0.0.1:4309").replace(/\/$/, "");
const RESOURCE_URL = env.RESOURCE_URL ?? "http://127.0.0.1:4308/resource";
const COORDINATOR_URL = (env.COORDINATOR_URL ?? "http://127.0.0.1:7900").replace(/\/$/, "");
const MAX_PRICE = ethers.parseUnits(env.MAX_PRICE_RLUSD ?? "0.25", 18);
const STATE = env.STATE_DIRECTORY ?? "./agent-state";
const CLI = env.CLI ?? "../../target/release/examples/cli";
const WALLET_FILE = env.WALLET_FILE ?? join(STATE, "agent.wallet.json");
const LOG_FILE = env.LOG_FILE ?? join(STATE, "log.json");
for (const k of ["AGENT_PRIVATE_KEY", "TEAM_NAME", "SUPPLIER", "AMOUNT"]) if (!env[k]) { console.error(`agent: need ${k}`); process.exit(2); }
mkdirSync(STATE, { recursive: true });
const t0 = Date.now(); const say = (m) => console.log(`[${String(Date.now() - t0).padStart(6)} ms] ${m}`);
const cli = (args) => execFileSync(CLI, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
const post = async (url, body, tries = 1) => {
  for (let i = 1; ; i++) {
    try { const r = await fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body), signal: AbortSignal.timeout(170_000) }); const j = await r.json().catch(() => ({})); return { status: r.status, body: j }; }
    catch (e) { if (i >= tries) throw e; say(`relay unreachable (${e.cause?.code ?? e.message}) — retrying in 3 s`); await new Promise((ok) => setTimeout(ok, 3000)); }
  }
};
// A run that paid but did not get relayed resumes here: the same bundle and the same purchase (the kit returns the
// earlier claim for the same operation name, nothing is paid twice) are presented to the relay again.
const PENDING = join(STATE, "pending.json");
const pending = existsSync(PENDING) ? JSON.parse(readFileSync(PENDING, "utf8")) : null;

// 1. The private transfer, built locally by the native wallet (keys never leave this machine) — or the pending one.
let built;
if (pending) { built = { bundleHash: pending.bundleHash }; say(`resuming the pending run: bundle ${pending.bundleHash.slice(0, 12)}… (paid: ${pending.claimId ? "yes" : "not yet"})`); }
else {
  const { records } = await (await fetch(`${COORDINATOR_URL}/log?from=0`)).json();
  writeFileSync(LOG_FILE, JSON.stringify(records));
  built = JSON.parse(cli(["wallet", "--file", WALLET_FILE, "build-transfer", env.SUPPLIER, env.AMOUNT, join(STATE, "transfer.json"), LOG_FILE]));
  writeFileSync(PENDING, JSON.stringify({ bundleHash: built.bundleHash, operation: `relay-${built.bundleHash.slice(0, 24)}` }, null, 1));
  say(`bundle built: ${built.bundleHash.slice(0, 12)}… — a private transfer of ${Number(env.AMOUNT) / 1e6} XRP inside the pool`);
}
const record = JSON.parse(readFileSync(join(STATE, "transfer.json"), "utf8"));

// 2. Ask the relay without paying: expect a 402 with the price.
let r = await post(`${RELAY_URL}/relay/transfer`, { record });
if (r.status !== 402) { console.error(`expected 402 from the relay, got ${r.status} ${JSON.stringify(r.body).slice(0, 200)}`); process.exit(1); }
const price = ethers.parseUnits(String(r.body.pay.priceRlusd), 18);
say(`402 received: the relay costs ${r.body.pay.priceRlusd} RLUSD (chain ${r.body.pay.chainId}, ledger ${r.body.pay.paymentLedger.slice(0, 10)}…)`);

// 3. The payment policy — the decision the rubric asks for, made here and logged.
const config = await (await fetch(`${AP4M_API_URL}/public/config`)).json();
const provider = new ethers.JsonRpcProvider(env.AP4M_RPC_URL ?? config.network.rpcUrl);
const agent = new ethers.Wallet(env.AGENT_PRIVATE_KEY, provider);
const context = await loadX402Context(AP4M_API_URL);
const { token } = await signIn(AP4M_API_URL, env.TEAM_NAME, agent);
let channelId = pending?.channelId || env.CHANNEL_ID;
if (!channelId) {
  const ch = await requestChannel(AP4M_API_URL, token, agent, { capRlusd: env.CHANNEL_CAP ?? "2" }); channelId = ch.channel_id;
  // Remember the channel at once: a resume must reuse it (the platform caps the number of open channels per team).
  writeFileSync(PENDING, JSON.stringify({ ...(pending ?? { bundleHash: built.bundleHash, operation: `relay-${built.bundleHash.slice(0, 24)}` }), channelId }, null, 1));
  say(`opened channel ${channelId.slice(0, 10)}… with a ${env.CHANNEL_CAP ?? "2"} RLUSD cap`);
}
const state = await getChannelState(provider, config.contracts.paymentLedger, channelId);
const remaining = BigInt(state.cap ?? state.limit ?? 0n) - BigInt(state.redeemed ?? 0n);
if (price > MAX_PRICE) { say(`policy: price ${ethers.formatUnits(price, 18)} > max ${ethers.formatUnits(MAX_PRICE, 18)} RLUSD — NOT paying`); process.exit(3); }
if (remaining > 0n && remaining < price) { say(`policy: channel has ${ethers.formatUnits(remaining, 18)} RLUSD left < price — NOT paying`); process.exit(3); }
say(`policy: price ${ethers.formatUnits(price, 18)} ≤ max ${ethers.formatUnits(MAX_PRICE, 18)} RLUSD, channel redeemed so far ${ethers.formatUnits(state.redeemed ?? 0n, 18)} — paying`);

// 4. Pay over x402: one purchase per bundle (the operation name is the bundle hash).
// The kit's merchant answers 503 while its chain head is stale and asks to "resume the same operation": the signed
// payment is retained on disk, so a retry re-presents it — never a second payment.
let purchase;
for (let i = 1; ; i++) {
  try { purchase = await buyResource({ context, provider, wallet: agent, stateDirectory: STATE, resourceUrl: RESOURCE_URL, channelId, maximumAmount: MAX_PRICE.toString(), operation: pending?.operation ?? `relay-${built.bundleHash.slice(0, 24)}` }); break; }
  catch (e) { if (i < 5 && /resume the same operation|retry the same operation/.test(e.message)) { say(`merchant not ready (${String(e.message).slice(0, 60)}…) — resuming in 5 s`); await new Promise((ok) => setTimeout(ok, 5000)); continue; } throw e; }
}
writeFileSync(PENDING, JSON.stringify({ ...(pending ?? { bundleHash: built.bundleHash, operation: `relay-${built.bundleHash.slice(0, 24)}` }), channelId, claimId: purchase.claimId }, null, 1));
say(`paid: claim ${purchase.claimId.slice(0, 12)}…, VoucherRedeemed ${config.network.explorerUrl}/tx/${purchase.transactionHash}`);
await fetch(`${AP4M_API_URL}/public/participant/record-redemption`, { method: "POST", headers: { "content-type": "application/json", authorization: `Bearer ${token}` }, body: JSON.stringify({ tx_hash: purchase.transactionHash }) }).catch(() => {});

// 5. Now the relay forwards the bundle to the quorum.
for (let i = 1; ; i++) {
  r = await post(`${RELAY_URL}/relay/transfer`, { record, channelId, claimId: purchase.claimId }, 6);
  if (r.status === 200) break;
  // A 5xx with the claim kept is the relay's own upstream (the public RPC or the quorum) having a moment: retry.
  if (r.status >= 500 && r.body?.claimKept && i < 6) { say(`relay ${r.status} (${String(r.body.error).slice(0, 80)}…) — claim kept, retrying in 5 s`); await new Promise((ok) => setTimeout(ok, 5000)); continue; }
  console.error(`relay refused after payment: ${r.status} ${JSON.stringify(r.body).slice(0, 300)} — the claim is kept; rerun to retry`); process.exit(1);
}
try { unlinkSync(PENDING); } catch {}
say(`relayed: anchored on XRPL L1 — https://testnet.xrpl.org/transactions/${r.body.anchorTx} (no sender, receiver or amount in the memo)`);

// 6. Settlement: RLUSD reaches the merchant payout; the receipt is verifiable by anyone.
const settled = await settle(AP4M_API_URL, token, channelId);
say(`settled: payout ${settled.payout_tx ? `${config.network.explorerUrl}/tx/${settled.payout_tx}` : "pending"}; receipt ${settled.receipt_url ? AP4M_API_URL + settled.receipt_url : "pending"}`);

// 8. The funded limit, shown: a second relay only if the cap still covers it.
if (env.OVER_CAP_DEMO === "1") {
  const after = await getChannelState(provider, config.contracts.paymentLedger, channelId);
  const left = BigInt(after.cap ?? 0n) - BigInt(after.redeemed ?? 0n);
  if (left < price) say(`second relay: policy refuses — ${ethers.formatUnits(left, 18)} RLUSD left on the channel < price ${ethers.formatUnits(price, 18)}; the ledger enforces settled ≤ redeemed ≤ cap and would refuse the voucher too. NOT paying.`);
  else say(`second relay: ${ethers.formatUnits(left, 18)} RLUSD left ≥ price — the policy would pay (set CHANNEL_CAP=0.15 to show the over-cap refusal)`);
}
console.log(`\nOK — two ledgers, two receipts: the metered payment is public on XRPL EVM, the settlement of the transfer is private on XRPL L1.`);
