// Smoke test of the UI's path: the demo wallet service + the coordinator + the browser's XRPL steps
// (faucet, deposit with memo), exactly what frontend/index.html does — without a browser.
//
//   WALLET_URL=http://host:8787/wallet-or-:7950 COORDINATOR_URL=http://host:8787/api node smoke-ui.mjs
//   (local: WALLET_URL=http://127.0.0.1:7250 COORDINATOR_URL=http://127.0.0.1:7200)
import { Client, Wallet, xrpToDrops } from "xrpl";
import { http, memoOf, MEMO_SHIELD, resilientClient } from "./lib.mjs";

const C = (process.env.COORDINATOR_URL ?? "http://127.0.0.1:7200").replace(/\/$/, "");
const W = (process.env.WALLET_URL ?? "http://127.0.0.1:7250").replace(/\/$/, "");
const DROPS = xrpToDrops(Number(process.env.DEPOSIT_XRP ?? "5"));
const t0 = Date.now();
const log = (m, x = "") => console.log(`[${String(Date.now() - t0).padStart(6)} ms] ${m}`, x);
const must = (c, m) => { if (!c) throw new Error(`SMOKE-UI FAILED: ${m}`); };

const cfg = await http("GET", `${C}/config`);
must(cfg.pool && cfg.xrplWs, "coordinator /config must expose pool and xrplWs");
const client = resilientClient(new Client(cfg.xrplWs)); await client.connect();
try {
  const alice = await http("POST", `${W}/wallets`), bob = await http("POST", `${W}/wallets`);
  must(alice.id && alice.address.length === 86, "wallet created");
  const { wallet: funder } = await client.fundWallet();
  log(`wallets ${alice.id.slice(0, 6)}… ${bob.id.slice(0, 6)}…, XRPL funder ${funder.address}`);

  const built = await http("POST", `${W}/wallets/${alice.id}/shield`, { amount: Number(DROPS) });
  const tx = await client.autofill({ TransactionType: "Payment", Account: funder.address, Destination: cfg.pool, Amount: DROPS, Memos: [memoOf(MEMO_SHIELD, built.bundleHash)] });
  const signed = funder.sign(tx); const dep = await client.submitAndWait(signed.tx_blob);
  must(dep.result.meta?.TransactionResult === "tesSUCCESS", "deposit");
  let s1 = null;
  for (let t = 0; t < 45 && !s1; t++) { try { s1 = await http("POST", `${C}/submit/shield`, { record: built.record, depositTxHash: signed.hash }); } catch (e) { if (!(e.status === 403 && /standby/.test(e.message))) throw e; await new Promise((r) => setTimeout(r, 2000)); } }
  must(s1?.anchorTx, "shield anchored");
  log(`shield anchored ${cfg.explorer}/transactions/${s1.anchorTx}`);
  must((await http("GET", `${W}/wallets/${alice.id}`)).balance === Number(DROPS), "alice balance after shield");

  const t1 = await http("POST", `${W}/wallets/${alice.id}/transfer`, { recipient: bob.address, amount: Number(DROPS) });
  const s2 = await http("POST", `${C}/submit/transfer`, { record: t1.record });
  must(s2.anchorTx, "transfer anchored"); log(`transfer anchored ${cfg.explorer}/transactions/${s2.anchorTx}`);
  must((await http("GET", `${W}/wallets/${bob.id}`)).balance === Number(DROPS), "bob received");

  const fresh = Wallet.generate();
  const u = await http("POST", `${W}/wallets/${bob.id}/unshield`, { amount: Number(DROPS), destination: fresh.address });
  const s3 = await http("POST", `${C}/submit/unshield`, { record: u.record, destination: fresh.address });
  must(s3.anchorTx && s3.payoutTx && !s3.payoutError, `unshield ${JSON.stringify({ a: s3.anchorTx, p: s3.payoutTx, e: s3.payoutError })}`);
  const bal = (await client.request({ command: "account_info", account: fresh.address, ledger_index: "validated" })).result.account_data.Balance;
  must(bal === DROPS, `fresh address got ${bal}`);
  log(`unshield paid ${cfg.explorer}/transactions/${s3.payoutTx}`);

  const r = await http("POST", `${C}/audit/reveal`, { viewingKey: bob.viewingKey });
  must(r.balance === 0 && r.notes.length === 1 && r.notes[0].spent, "reveal");
  log("reveal ok");
  console.log("\nOK — the UI path works end to end");
} finally { await client.disconnect(); }
