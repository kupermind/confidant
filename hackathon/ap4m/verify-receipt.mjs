// node verify-receipt.mjs <receipt.json>
//
// Re-checks a relay receipt from PUBLIC data only — no key, no log, no trust in the relay:
//   1. the merchant's signature recovers to the payout address the statement names;
//   2. the agent's EVM signature recovers to the channel's agent, and its XRPL signature to the account named;
//   3. on the XRPL EVM: the channel's parties are those two addresses, and a VoucherRedeemed event exists for
//      (channel, claim) at the PaymentLedger — the agent paid;
//   4. on XRPL L1: the anchor transaction was sent by the pool account and its `confidant/anchor` memo names the
//      bundle hash — the quorum settled exactly that bundle (no sender, receiver or amount in it);
//   5. on XRPL L1: the agent's account carries the pool's proof-of-funds credential with the URI the statement
//      records — the agent had proved "controlled at least X XRP in the pool as of ledger L" (not "controls it now").
// Exit 0 when every check passes, 1 otherwise. Env: AP4M_API_URL, AP4M_RPC_URL, XRPL_WS (optional).
import { readFileSync } from "node:fs";
import { ethers } from "ethers";
import { Client } from "xrpl";
import { deriveAddress, verify as verifyXrpl } from "ripple-keypairs";
import { PARTICIPANT_LEDGER_INTERFACE, readPublicLogs } from "@ap4m/sdk";
import { loadX402Context } from "./ap4m-x402.mjs";
import { readParticipantPaymentChannel } from "@ap4m/sdk";
import { statementText, statementHex, bundleHashFromMemo, MEMO_ANCHOR_HEX, findPofCredential, hexToText, parsePofUri, LSF_ACCEPTED, rippleNow } from "./receipt.mjs";

const env = process.env;
const AP4M_API_URL = (env.AP4M_API_URL ?? "https://hackathon.ap4m.build").replace(/\/$/, "");
const file = process.argv[2];
if (!file) { console.error("usage: node verify-receipt.mjs <receipt.json>"); process.exit(2); }
const r = JSON.parse(readFileSync(file, "utf8")); const s = r.statement; const text = statementText(s);
let fails = 0;
const ok = (name, cond, detail = "") => { console.log(`${cond ? "✓" : "✗"} ${name}${detail ? ` — ${detail}` : ""}`); if (!cond) fails++; };
console.log(`receipt ${s.kind} for bundle ${s.bundleHash.slice(0, 12)}… · claim ${s.claimId.slice(0, 12)}… · issued ${s.issuedAt}\n`);

// 1–2. Signatures over the canonical statement.
const merchant = ethers.verifyMessage(text, r.merchantSignature);
ok("merchant signature recovers to the payout address in the statement", merchant.toLowerCase() === String(s.merchantPayout).toLowerCase(), merchant);
const agentEvm = ethers.verifyMessage(text, r.agentSignature);
let xrplSig = false; try { xrplSig = deriveAddress(r.agentXrpl.publicKey) === s.agentAccount && verifyXrpl(statementHex(s), r.agentXrpl.signature, r.agentXrpl.publicKey); } catch {}
ok("agent XRPL signature by the account the statement names", xrplSig, s.agentAccount);

// 3. XRPL EVM: the channel's parties and the redeemed voucher. The public RPC drops a read now and then: retried.
const config = await (await fetch(`${AP4M_API_URL}/public/config`)).json();
const provider = new ethers.JsonRpcProvider(env.AP4M_RPC_URL ?? config.network.rpcUrl);
const rpc = async (fn) => { let last; for (let i = 1; i <= 4; i++) { try { return await fn(); } catch (e) { last = e; await new Promise((r) => setTimeout(r, 1500 * i)); } } throw last; };
try {
  const context = await loadX402Context(AP4M_API_URL);
  const chan = await rpc(() => readParticipantPaymentChannel(provider, context.participantRequest, s.channelId));
  ok("channel's agent is the agent's EVM signer", String(chan.agent).toLowerCase() === agentEvm.toLowerCase(), agentEvm);
  ok("channel's payout is the merchant signer", String(chan.payout).toLowerCase() === merchant.toLowerCase(), chan.payout);
  const event = PARTICIPANT_LEDGER_INTERFACE.getEvent("VoucherRedeemed");
  const latest = await rpc(() => provider.getBlockNumber());
  const LOOKBACK = Number(env.LOOKBACK_BLOCKS ?? 50_000); // ≈ 2 days of blocks; the relay uses the same window
  const logs = await rpc(() => readPublicLogs(provider, { address: s.paymentLedger, topics: [event.topicHash, s.channelId, s.claimId], fromBlock: Math.max(0, latest - LOOKBACK), toBlock: latest }));
  ok("VoucherRedeemed for (channel, claim) at the PaymentLedger — the agent paid", logs.length >= 1, logs[0] ? `${config.network.explorerUrl}/tx/${logs[0].transactionHash}` : `none in the last ${LOOKBACK} blocks (older? raise LOOKBACK_BLOCKS)`);
} catch (e) { ok("XRPL EVM checks (channel parties, VoucherRedeemed)", false, `RPC unreadable: ${String(e.message).slice(0, 100)}`); }

// 4–5. XRPL L1: the anchor memo and the proof-of-funds credential.
const c = new Client(env.XRPL_WS ?? "wss://s.altnet.rippletest.net:51233"); await c.connect();
try {
  const tx = (await c.request({ command: "tx", transaction: s.anchorTx })).result;
  const body = tx.tx_json ?? tx;
  const memo = (body.Memos ?? []).map((m) => m.Memo).find((m) => String(m.MemoType).toUpperCase() === MEMO_ANCHOR_HEX);
  const anchored = memo ? bundleHashFromMemo(memo.MemoData) : null;
  ok("anchor transaction was sent by the pool account", body.Account === s.pool, `https://testnet.xrpl.org/transactions/${s.anchorTx}`);
  ok("its confidant/anchor memo names this bundle hash — the quorum settled exactly this bundle", anchored === s.bundleHash, anchored ?? "no anchor memo");
  const objs = (await c.request({ command: "account_objects", account: s.agentAccount, type: "credential", ledger_index: "validated" })).result.account_objects;
  const cred = findPofCredential(objs, s.pool);
  if (!cred) ok("agent's account carries the pool's proof-of-funds credential", false, "none on the ledger now — replaced or deleted since; the statement records what the relay read at the time");
  else {
    const uri = hexToText(cred.URI), u = parsePofUri(uri);
    ok("agent's account carries the pool's proof-of-funds credential, accepted by the account", !!(cred.Flags & LSF_ACCEPTED));
    ok("that credential's URI is the one the statement records", uri === s.pof.uri, uri);
    ok("it has not expired", !cred.Expiration || cred.Expiration > rippleNow());
    if (u) console.log(`  it says: controlled at least ${u.amount / 1e6} XRP in the pool as of ledger ${u.ledgerIndex} — a snapshot, not "controls it now"`);
  }
} finally { await c.disconnect(); }

console.log(fails ? `\n${fails} check(s) failed` : "\nAll checks passed: this agent proved its shielded funds, paid for this bundle, and the quorum anchored it.");
process.exit(fails ? 1 : 0);
