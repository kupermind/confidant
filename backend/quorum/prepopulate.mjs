// Pre-populates a RUNNING pool before a demo so the first real operations do not stand alone:
// N holder wallets (keys under DIR, 0600, gitignored) each shield a fixed denomination through the
// coordinator from a faucet-funded XRPL account, then some of them pass their note on in a private
// transfer. Honest label: this is a SYNTHETIC anonymity set funded by the operator from the testnet
// faucet — it makes the demo pool look lived-in; it is not evidence of real users.
//
//   COORDINATOR_URL=http://host:8787 CLI=../../target/release/examples/cli HOLDERS=8 DENOMS=5,10,25 TRANSFERS=4 node prepopulate.mjs
//   env: XRPL_WS (testnet default), DIR (../../data/prepop-<ts>), POOL (optional; read from /config)

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { Client, xrpToDrops } from "xrpl";
import { http, memoOf, MEMO_SHIELD, resilientClient } from "./lib.mjs";

const C = (process.env.COORDINATOR_URL ?? "http://127.0.0.1:7200").replace(/\/$/, "");
const CLI = process.env.CLI ?? "../../target/release/examples/cli";
const XRPL_WS = process.env.XRPL_WS ?? "wss://s.altnet.rippletest.net:51233";
const HOLDERS = Number(process.env.HOLDERS ?? 8);
const DENOMS = (process.env.DENOMS ?? "5,10,25").split(",").map((x) => xrpToDrops(Number(x.trim())));
const TRANSFERS = Number(process.env.TRANSFERS ?? Math.floor(HOLDERS / 2));
const DIR = process.env.DIR ?? `../../data/prepop-${new Date().toISOString().replace(/[:.]/g, "-")}`;
const EXPLORER = "https://testnet.xrpl.org/transactions";
mkdirSync(DIR, { recursive: true });
const t0 = Date.now();
const log = (msg, extra = "") => console.log(`[${String(Date.now() - t0).padStart(6)} ms] ${msg}`, extra);
const cli = (...args) => JSON.parse(execFileSync(CLI, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }));
const wallet = (name) => (...args) => cli("wallet", "--file", `${DIR}/${name}.wallet.json`, ...args);
const LOG = `${DIR}/log.json`;
const sync = async () => { const { records } = await http("GET", `${C}/log?from=0`); writeFileSync(LOG, JSON.stringify(records)); };
const record = (f) => JSON.parse(readFileSync(`${DIR}/${f}`, "utf8"));
const feeOf = (drops, bps) => Math.floor((Number(drops) * (bps ?? 0)) / 10000);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const client = resilientClient(new Client(XRPL_WS));
await client.connect();
const done = { shields: [], transfers: [], failed: [] };
try {
  const cfg = await http("GET", `${C}/config`);
  const POOL = process.env.POOL ?? cfg.pool;
  if (!POOL) throw new Error("no pool account: set POOL=r… or use a coordinator whose /config exposes it");
  const state0 = await http("GET", `${C}/state`);
  log(`coordinator ${C}: pool ${POOL}, ${state0.bundleCount} bundles, fees ${cfg.fees?.shieldBps ?? 0}/${cfg.fees?.unshieldBps ?? 0} bps`);
  const holders = [];
  for (let i = 0; i < HOLDERS; i++) {
    const name = `holder${String(i + 1).padStart(2, "0")}`;
    const gross = DENOMS[i % DENOMS.length];
    const net = String(Number(gross) - feeOf(gross, cfg.fees?.shieldBps));
    try {
      // A fresh faucet account per holder: the pool's public endpoints should not cluster.
      let funder = null;
      for (let a = 0; a < 5 && !funder; a++) {
        try { funder = (await client.fundWallet()).wallet; } catch (e) { log(`faucet busy (${e.message?.slice(0, 60)}), retrying in ${5 * (a + 1)} s`); await sleep(5000 * (a + 1)); }
      }
      if (!funder) throw new Error("faucet unavailable");
      const w = wallet(name); const info = w("init");
      await sync();
      const built = w("build-shield", net, `${DIR}/${name}.shield.json`, LOG);
      const prepared = await client.autofill({ TransactionType: "Payment", Account: funder.address, Destination: POOL, Amount: gross, Memos: [memoOf(MEMO_SHIELD, built.bundleHash)] });
      const signed = funder.sign(prepared);
      const dep = await client.submitAndWait(signed.tx_blob);
      if (dep.result.meta?.TransactionResult !== "tesSUCCESS") throw new Error(`deposit ${dep.result.meta?.TransactionResult}`);
      let acc = null;
      for (let t = 0; t < 45 && !acc; t++) {
        try { acc = await http("POST", `${C}/submit/shield`, { record: record(`${name}.shield.json`), depositTxHash: signed.hash }); }
        catch (e) { if (!(e.status === 403 && /standby/.test(e.message))) throw e; await sleep(2000); }
      }
      if (!acc) throw new Error("standby never cleared");
      holders.push({ name, address: info.address, net });
      done.shields.push({ name, gross, net, deposit: signed.hash, anchor: acc.anchorTx });
      log(`${name}: shielded ${Number(gross) / 1e6} XRP (note ${Number(net) / 1e6}) — deposit ${EXPLORER}/${signed.hash}, anchor ${EXPLORER}/${acc.anchorTx}`);
    } catch (e) { done.failed.push({ name, step: "shield", error: e.message }); log(`${name}: FAILED ${e.message}`); }
  }
  // Private hops: holder i hands its whole note to holder i+1 (only the anchor reaches the ledger).
  for (let i = 0; i < Math.min(TRANSFERS, holders.length - 1); i++) {
    const from = holders[i], to = holders[i + 1];
    try {
      await sync();
      wallet(from.name)("build-transfer", to.address, from.net, `${DIR}/${from.name}.transfer.json`, LOG);
      const acc = await http("POST", `${C}/submit/transfer`, { record: record(`${from.name}.transfer.json`) });
      done.transfers.push({ from: from.name, to: to.name, anchor: acc.anchorTx });
      log(`${from.name} → ${to.name}: private transfer anchored ${EXPLORER}/${acc.anchorTx}`);
    } catch (e) { done.failed.push({ name: from.name, step: "transfer", error: e.message }); log(`${from.name}: transfer FAILED ${e.message}`); }
  }
  const state1 = await http("GET", `${C}/state`);
  writeFileSync(`${DIR}/summary.json`, JSON.stringify({ coordinator: C, pool: POOL, ...done, bundlesBefore: state0.bundleCount, bundlesAfter: state1.bundleCount }, null, 2));
  console.log(`\n${done.shields.length} shields + ${done.transfers.length} transfers (${done.failed.length} failed); pool ${state0.bundleCount} → ${state1.bundleCount} bundles. Holder keys under ${DIR} (0600, gitignored).`);
  console.log("Honest label: a synthetic anonymity set funded from the testnet faucet by the operator — say so on stage.");
  if (done.failed.length) process.exitCode = 1;
} finally {
  await client.disconnect();
}
