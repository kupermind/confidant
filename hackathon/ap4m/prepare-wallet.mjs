// One-time preparation of the agent's Confidant side (pool on XRPL L1 testnet), so agent.mjs has notes to spend:
//   1. two native wallet files via the CLI (agent, supplier) — keys stay in STATE_DIRECTORY, never leave this machine;
//   2. a faucet-funded XRPL account for the deposit, kept in STATE_DIRECTORY/xrpl.json;
//   3. when the pool's credential gate is on: asks the pool's demo issuer service (ISSUER_URL, default: the coordinator's
//      origin + /issuer) to write the account's entry credential, then accepts it from here; if the service is not
//      there, waits for the issuer's operator to write it (the under-the-hood page's fold or the host);
//   4. a shield of DEPOSIT_XRP through the coordinator (deposit Payment with the bundle-hash memo, then submit);
//   5. prints the SUPPLIER (the supplier wallet's Orchard address) and the agent's private balance.
//
//   COORDINATOR_URL=https://confidant.im/api CLI=../../target/release/examples/cli node prepare-wallet.mjs
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { Client, Wallet, xrpToDrops } from "xrpl";

const env = process.env;
const C = (env.COORDINATOR_URL ?? "https://confidant.im/api").replace(/\/$/, "");
const ISSUER = (env.ISSUER_URL ?? C.replace(/\/api$/, "") + "/issuer").replace(/\/$/, ""); // the demo issuer service
const CLI = env.CLI ?? "../../target/release/examples/cli";
const STATE = env.STATE_DIRECTORY ?? "./agent-state";
const DROPS = xrpToDrops(Number(env.DEPOSIT_XRP ?? "20"));
const LSF_ACCEPTED = 0x00010000;
mkdirSync(STATE, { recursive: true });
const t0 = Date.now(); const say = (m) => console.log(`[${String(Date.now() - t0).padStart(6)} ms] ${m}`);
const cli = (args) => execFileSync(CLI, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
const http = async (method, url, body) => { const r = await fetch(url, { method, headers: { "content-type": "application/json" }, body: body ? JSON.stringify(body) : undefined }); const j = await r.json().catch(() => ({})); if (!r.ok) { const e = new Error(j.error ?? `HTTP ${r.status}`); e.status = r.status; throw e; } return j; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// 1. Native wallets (idempotent).
const AGENT = join(STATE, "agent.wallet.json"), SUPPLIER = join(STATE, "supplier.wallet.json"), LOG = join(STATE, "log.json");
for (const f of [AGENT, SUPPLIER]) if (!existsSync(f)) { cli(["wallet", "--file", f, "init"]); say(`created ${f}`); }
const info = (f) => JSON.parse(cli(["wallet", "--file", f, "info"]));
const supplierAddr = info(SUPPLIER).address;
say(`supplier Orchard address: ${supplierAddr}`);

// 2. The XRPL account that deposits (faucet; kept so a rerun reuses it).
const cfg = await http("GET", `${C}/config`);
const client = new Client(cfg.xrplWs); await client.connect();
try {
  const XF = join(STATE, "xrpl.json");
  let x = existsSync(XF) ? JSON.parse(readFileSync(XF, "utf8")) : null;
  if (!x) { const { wallet } = await client.fundWallet(); x = { seed: wallet.seed, address: wallet.address }; writeFileSync(XF, JSON.stringify(x, null, 1), { mode: 0o600 }); say(`faucet account ${x.address} (kept in ${XF})`); }
  else say(`XRPL account ${x.address} (from ${XF})`);
  const w = Wallet.fromSeed(x.seed);

  // 3. The credential gate: wait for the issuer, then accept from here.
  if (cfg.credential?.issuer) {
    say(`gate is on: issuer ${cfg.credential.issuer}, type ${cfg.credential.typeText ?? cfg.credential.type}`);
    // The demo issuer service writes the credential to any account that asks (idempotent, rate-limited).
    let asked = null;
    try {
      const r = await fetch(`${ISSUER}/issue`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ subject: x.address }), signal: AbortSignal.timeout(60_000) });
      const j = await r.json().catch(() => ({}));
      if (r.ok) { asked = j; say(`issuer service: ${j.status}${j.tx ? ` — CredentialCreate https://testnet.xrpl.org/transactions/${j.tx}` : " (already on the ledger)"}`); }
      else say(`issuer service answered ${r.status} (${j.error ?? ""}) — waiting for the issuer's operator instead`);
    } catch (e) { say(`issuer service not reachable at ${ISSUER} (${e.message}) — waiting for the issuer's operator to write ${x.address}'s credential (the under-the-hood page's fold, or the host)`); }
    let hit = null;
    for (let i = 0; i < (asked ? 24 : 120) && !hit; i++) {
      const r = await client.request({ command: "account_objects", account: x.address, type: "credential", ledger_index: "validated" });
      hit = (r.result.account_objects ?? []).find((o) => o.Issuer === cfg.credential.issuer && String(o.CredentialType).toUpperCase() === String(cfg.credential.type).toUpperCase());
      if (!hit) await sleep(5000);
    }
    if (!hit) throw new Error(asked ? "the issuer service answered but the credential did not appear on the ledger within 2 minutes" : "no credential appeared within 10 minutes");
    if (!(hit.Flags & LSF_ACCEPTED)) {
      const r = await client.submitAndWait(w.sign(await client.autofill({ TransactionType: "CredentialAccept", Account: x.address, Issuer: cfg.credential.issuer, CredentialType: cfg.credential.type })).tx_blob);
      if (r.result.meta?.TransactionResult !== "tesSUCCESS") throw new Error(`CredentialAccept: ${r.result.meta?.TransactionResult}`);
      say("credential accepted");
    } else say("credential already accepted");
  }

  // 4. Shield DEPOSIT_XRP into the agent's wallet (net of the shield fee).
  const feeOf = (drops, bps) => Math.floor((Number(drops) * (bps ?? 0)) / 10000);
  const net = Number(DROPS) - feeOf(DROPS, cfg.fees?.shieldBps);
  const { records } = await http("GET", `${C}/log?from=0`); writeFileSync(LOG, JSON.stringify(records));
  const built = JSON.parse(cli(["wallet", "--file", AGENT, "build-shield", String(net), join(STATE, "shield.json"), LOG]));
  const record = JSON.parse(readFileSync(join(STATE, "shield.json"), "utf8"));
  const tx = await client.autofill({ TransactionType: "Payment", Account: x.address, Destination: cfg.pool, Amount: DROPS, Memos: [{ Memo: { MemoType: Buffer.from("confidant/shield").toString("hex").toUpperCase(), MemoData: built.bundleHash.toUpperCase() } }] });
  const signed = w.sign(tx); const dep = await client.submitAndWait(signed.tx_blob);
  if (dep.result.meta?.TransactionResult !== "tesSUCCESS") throw new Error(`deposit: ${dep.result.meta?.TransactionResult}`);
  say(`deposit ${cfg.explorer}/transactions/${signed.hash}`);
  let acc;
  for (let t = 0; t < 45; t++) { try { acc = await http("POST", `${C}/submit/shield`, { record, depositTxHash: signed.hash }); break; } catch (e) { if (e.status === 403 && /standby/.test(e.message)) { await sleep(4000); continue; } throw e; } }
  if (!acc?.anchorTx) throw new Error("shield not anchored");
  say(`shield anchored ${cfg.explorer}/transactions/${acc.anchorTx} (${acc.signers?.length ?? "?"} of ${cfg.signers} signers)`);

  // 5. Sync and report.
  const { records: all } = await http("GET", `${C}/log?from=0`); writeFileSync(LOG, JSON.stringify(all));
  cli(["wallet", "--file", AGENT, "sync", LOG]);
  const bal = JSON.parse(cli(["wallet", "--file", AGENT, "balance", LOG]));
  console.log(`\nOK — agent private balance ${JSON.stringify(bal)} drops. Put in .env:\n  SUPPLIER=${supplierAddr}\n  WALLET_FILE=${AGENT}\n  LOG_FILE=${LOG}`);
} finally { await client.disconnect(); }
