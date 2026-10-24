// Confidant × AP4M relay — an x402 Merchant in front of the coordinator (hackathon track only).
//
//   The thing sold is the RELAY of a shielded transfer bundle to the quorum. Without a valid, unused
//   payment claim the relay answers 402 and forwards nothing; with one it forwards the bundle to the
//   coordinator's existing POST /submit/transfer and returns the anchor the quorum produced.
//
//   Option B of the design ("claim-proof relay", the kit used unmodified):
//     1. the kit's createX402Merchant() sells a static "relay ticket" at GET /resource for PRICE_RLUSD —
//        the agent buys it with buyResource() and receives a claimId (its Voucher is redeemed on-chain by
//        this process from the submitter wallet, the kit's x402 shape);
//     2. POST /relay/transfer { record, channelId, claimId } — the relay checks the claim ON-CHAIN
//        (a VoucherRedeemed event for (channelId, claimId) at the PaymentLedger, the claim's amount ≥ the
//        price) and that it is UNUSED (spent-set in the state directory), then forwards the bundle to the
//        coordinator and marks the claim spent. Missing / unknown / reused claim → 402.
//
//   Nothing here touches core, nodes, sidecars or the coordinator's trust path: the relay is one more
//   client of the coordinator, holds no Confidant keys and never signs on XRPL L1. The relay fee is a
//   metering fee, separate from the pool's protocol fee.
//
//   Env (all from the portal / GET /public/config / ours; keys only via env or an untracked .env):
//     AP4M_API_URL (default https://hackathon.ap4m.build), MERCHANT_ID, MERCHANT_PAYOUT, MERCHANT_SUBMITTER_KEY,
//     PRICE_RLUSD (default 0.1), PORT (default 4308), RESOURCE_URL (public URL of GET /resource),
//     COORDINATOR_URL (default http://127.0.0.1:7900), STATE_DIRECTORY (default ./relay-state), AP4M_RPC_URL (optional)
import { createServer } from "node:http";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ethers } from "ethers";
import { PARTICIPANT_LEDGER_INTERFACE, readParticipantClaim, readPublicLogs } from "@ap4m/sdk";
import { createX402Merchant, loadX402Context } from "./ap4m-x402.mjs";

const env = process.env;
const AP4M_API_URL = (env.AP4M_API_URL ?? "https://hackathon.ap4m.build").replace(/\/$/, "");
const COORDINATOR_URL = (env.COORDINATOR_URL ?? "http://127.0.0.1:7900").replace(/\/$/, "");
const PRICE_RLUSD = env.PRICE_RLUSD ?? "0.1";
const PORT = Number(env.PORT ?? 4308), RELAY_PORT = Number(env.RELAY_PORT ?? PORT + 1);
const RESOURCE_URL = env.RESOURCE_URL ?? `http://127.0.0.1:${PORT}/resource`;
const STATE = env.STATE_DIRECTORY ?? "./relay-state";
for (const k of ["MERCHANT_ID", "MERCHANT_PAYOUT", "MERCHANT_SUBMITTER_KEY"]) if (!env[k]) { console.error(`relay: need ${k} (portal → My setup)`); process.exit(2); }
mkdirSync(STATE, { recursive: true });
const SPENT = join(STATE, "spent-claims.json");
const spent = new Set(existsSync(SPENT) ? JSON.parse(readFileSync(SPENT, "utf8")) : []);
const markSpent = (id) => { spent.add(id); writeFileSync(SPENT, JSON.stringify([...spent], null, 1)); };

const context = await loadX402Context(AP4M_API_URL);
const config = await (await fetch(`${AP4M_API_URL}/public/config`)).json();
const provider = new ethers.JsonRpcProvider(env.AP4M_RPC_URL ?? config.network.rpcUrl);
const price = ethers.parseUnits(PRICE_RLUSD, 18);
const explorer = config.network.explorerUrl?.replace(/\/$/, "") ?? "https://explorer.testnet.xrplevm.org";

// 1. The kit's merchant, unmodified: one static ticket per purchase.
const merchant = createX402Merchant({
  context, provider,
  submitter: new ethers.Wallet(env.MERCHANT_SUBMITTER_KEY, provider),
  stateDirectory: join(STATE, "merchant"),
  resourceUrl: RESOURCE_URL,
  merchantParticipantId: env.MERCHANT_ID,
  payout: env.MERCHANT_PAYOUT,
  amount: price.toString(),
  content: { ticket: "confidant/relay/v1", relay: "POST /relay/transfer { record, channelId, claimId }", priceRlusd: PRICE_RLUSD },
});
merchant.server.listen(PORT, "127.0.0.1", () => console.log(`relay: ticket merchant on :${PORT} (${RESOURCE_URL}) at ${PRICE_RLUSD} RLUSD`));

// 2. The relay: a paid claim buys exactly one forward to the quorum.
const send = (res, status, body) => { res.writeHead(status, { "content-type": "application/json", "access-control-allow-origin": "*" }); res.end(JSON.stringify(body)); };
const paymentRequired = (res, why) => send(res, 402, {
  error: "payment_required", why,
  pay: { resource: RESOURCE_URL, priceRlusd: PRICE_RLUSD, token: config.contracts.rlusdToken, paymentLedger: config.contracts.paymentLedger, chainId: config.network.chainId },
  then: "POST /relay/transfer { record, channelId, claimId }",
});

/** The claim must be redeemed on the PaymentLedger for this channel, cover the price, and be unused. */
async function verifyClaim(channelId, claimId) {
  if (!/^0x[0-9a-f]{64}$/.test(channelId ?? "") || !/^0x[0-9a-f]{64}$/.test(claimId ?? "")) return "channelId and claimId must be 32-byte hex";
  if (spent.has(claimId)) return "claim already used for a relay";
  const ledger = context.x402.merchant.ledger;
  const latest = await provider.getBlockNumber();
  const event = PARTICIPANT_LEDGER_INTERFACE.getEvent("VoucherRedeemed");
  const logs = await readPublicLogs(provider, { address: ledger, topics: [event.topicHash, channelId, claimId], fromBlock: Math.max(0, latest - 200_000), toBlock: latest });
  if (logs.length !== 1) return `no VoucherRedeemed for this claim at the PaymentLedger (${logs.length} found)`;
  const claim = await readParticipantClaim(provider, context.participantRequest, claimId, latest);
  if (claim.channelId !== channelId) return "claim belongs to another channel";
  if (BigInt(claim.amount) < price) return `claim pays ${ethers.formatUnits(claim.amount, 18)} RLUSD, the relay costs ${PRICE_RLUSD}`;
  return { tx: logs[0].transactionHash, amount: claim.amount };
}

createServer(async (req, res) => {
  if (req.method === "OPTIONS") { res.writeHead(204, { "access-control-allow-origin": "*", "access-control-allow-headers": "content-type", "access-control-allow-methods": "POST" }); return res.end(); }
  if (req.method === "GET" && req.url === "/relay/health") return send(res, 200, { ok: true, priceRlusd: PRICE_RLUSD, coordinator: COORDINATOR_URL, spentClaims: spent.size });
  if (req.method !== "POST" || req.url !== "/relay/transfer") return send(res, 404, { error: "no route" });
  let body; try { body = JSON.parse(await new Promise((ok, ko) => { let d = ""; req.on("data", (c) => { d += c; if (d.length > 1 << 20) ko(new Error("too large")); }); req.on("end", () => ok(d)); req.on("error", ko); })); } catch (e) { return send(res, 400, { error: `bad body: ${e.message}` }); }
  if (!body?.record) return send(res, 400, { error: "need record (a transfer bundle record)" });
  if (!body.claimId) return paymentRequired(res, "no payment claim with the request");
  let claim;
  try { claim = await verifyClaim(String(body.channelId ?? "").toLowerCase(), String(body.claimId).toLowerCase()); } catch (e) { return send(res, 502, { error: `payment ledger unreadable: ${e.message}` }); }
  if (typeof claim === "string") return paymentRequired(res, claim);
  // Paid: forward exactly one bundle to the quorum (the coordinator's /endorse idempotency covers retries).
  const r = await fetch(`${COORDINATOR_URL}/submit/transfer`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ record: body.record }) });
  const text = await r.text(); let out; try { out = JSON.parse(text); } catch { out = { error: text.slice(0, 200) }; }
  if (!r.ok) return send(res, r.status, { error: `quorum refused the bundle: ${out.error ?? r.status}`, claimKept: true });
  markSpent(String(body.claimId).toLowerCase());
  console.log(`relay: claim ${body.claimId.slice(0, 10)}… (${ethers.formatUnits(claim.amount, 18)} RLUSD) → anchored ${out.anchorTx}`);
  send(res, 200, { anchorTx: out.anchorTx, newRoot: out.newRoot, bundleIndex: out.bundleIndex, signers: out.signers, payment: { claimId: body.claimId, evmTx: claim.tx, evmExplorer: `${explorer}/tx/${claim.tx}` }, label: "the metered relay fee is public on XRPL EVM; the transfer's sender, receiver and amount stay inside the pool — only the anchor memo reaches XRPL L1" });
}).listen(RELAY_PORT, "127.0.0.1", () => console.log(`relay: POST /relay/transfer on :${RELAY_PORT} → ${COORDINATOR_URL}/submit/transfer`));
for (const sig of ["SIGINT", "SIGTERM"]) process.once(sig, () => merchant.close().finally(() => process.exit(0)));
