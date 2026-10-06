// Smoke test and reference client against a RUNNING coordinator (deployed behind nginx, or a local
// start-all): faucet wallet → shield (deposit with memo = bundle hash) → private transfer → unshield to
// a fresh XRPL address → auditor reveal. Prints explorer links. Nothing here holds pool or signer keys.
//
//   COORDINATOR_URL=http://host:8787 CLI=../../target/release/examples/cli node smoke.mjs
//   env: XRPL_WS (testnet default), DEPOSIT_XRP (5), DIR (../../data/smoke-<ts>, wallets 0600, gitignored)

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { Client, Wallet, xrpToDrops } from "xrpl";
import { http, memoOf, MEMO_SHIELD, resilientClient } from "./lib.mjs";

const C = (process.env.COORDINATOR_URL ?? "http://127.0.0.1:7200").replace(/\/$/, "");
const CLI = process.env.CLI ?? "../../target/release/examples/cli";
const XRPL_WS = process.env.XRPL_WS ?? "wss://s.altnet.rippletest.net:51233";
const DROPS = xrpToDrops(Number(process.env.DEPOSIT_XRP ?? "5"));
const DIR = process.env.DIR ?? `../../data/smoke-${new Date().toISOString().replace(/[:.]/g, "-")}`;
const EXPLORER = "https://testnet.xrpl.org/transactions";
mkdirSync(DIR, { recursive: true });
const t0 = Date.now();
const log = (msg, extra = "") => console.log(`[${String(Date.now() - t0).padStart(6)} ms] ${msg}`, extra);
const cli = (...args) => JSON.parse(execFileSync(CLI, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }));
const wallet = (name) => (...args) => cli("wallet", "--file", `${DIR}/${name}.wallet.json`, ...args);
const LOG = `${DIR}/log.json`;
const sync = async () => { const { records } = await http("GET", `${C}/log?from=0`); writeFileSync(LOG, JSON.stringify(records)); };
const record = (f) => JSON.parse(readFileSync(`${DIR}/${f}`, "utf8"));
const must = (cond, msg) => { if (!cond) throw new Error(`SMOKE FAILED: ${msg}`); };

const client = resilientClient(new Client(XRPL_WS));
await client.connect();
try {
  const state0 = await http("GET", `${C}/state`);
  log(`coordinator ${C}: ${state0.bundleCount} bundles, root ${state0.root.slice(0, 16)}…`);
  const { wallet: funder } = await client.fundWallet();
  const alice = wallet("alice"), bob = wallet("bob");
  alice("init"); const bobInfo = bob("init");
  await sync();

  // 1. shield: build first, then the deposit Payment with memo = bundle hash, then submit the tx hash.
  const built = alice("build-shield", DROPS, `${DIR}/shield.json`, LOG);
  const prepared = await client.autofill({ TransactionType: "Payment", Account: funder.address, Destination: (await poolOf()), Amount: DROPS, Memos: [memoOf(MEMO_SHIELD, built.bundleHash)] });
  const signed = funder.sign(prepared);
  const dep = await client.submitAndWait(signed.tx_blob);
  must(dep.result.meta?.TransactionResult === "tesSUCCESS", `deposit failed: ${dep.result.meta?.TransactionResult}`);
  log(`deposit ${EXPLORER}/${signed.hash}`);
  const s1 = await http("POST", `${C}/submit/shield`, { record: record("shield.json"), depositTxHash: signed.hash });
  must(s1.bundleIndex === state0.bundleCount && s1.anchorTx, `shield not anchored: ${JSON.stringify(s1)}`);
  log(`shield accepted by ${s1.signers.length} signers, anchor ${EXPLORER}/${s1.anchorTx}`);

  // 2. private transfer alice → bob: only the anchor reaches the ledger.
  await sync();
  alice("build-transfer", bobInfo.address, DROPS, `${DIR}/transfer.json`, LOG);
  const s2 = await http("POST", `${C}/submit/transfer`, { record: record("transfer.json") });
  must(s2.anchorTx, "transfer not anchored");
  log(`transfer anchored ${EXPLORER}/${s2.anchorTx}`);

  // 3. unshield bob → a fresh XRPL address: anchor, then the 2-of-3 payout.
  await sync();
  const fresh = Wallet.generate();
  bob("build-unshield", DROPS, fresh.address, `${DIR}/unshield.json`, LOG);
  const s3 = await http("POST", `${C}/submit/unshield`, { record: record("unshield.json"), destination: fresh.address });
  must(s3.anchorTx && s3.payoutTx && !s3.payoutError, `unshield: ${JSON.stringify({ anchorTx: s3.anchorTx, payoutTx: s3.payoutTx, payoutError: s3.payoutError })}`);
  const bal = (await client.request({ command: "account_info", account: fresh.address, ledger_index: "validated" })).result.account_data.Balance;
  must(bal === DROPS, `fresh address balance ${bal} ≠ ${DROPS}`);
  log(`unshield anchored ${EXPLORER}/${s3.anchorTx}, paid ${EXPLORER}/${s3.payoutTx} (${bal} drops at ${fresh.address})`);

  // 4. reveal bob with his viewing key.
  const reveal = await http("POST", `${C}/audit/reveal`, { viewingKey: bobInfo.viewingKey });
  must(reveal.balance === 0 && reveal.notes.length === 1 && reveal.notes[0].spent, `reveal: ${JSON.stringify(reveal)}`);
  const state1 = await http("GET", `${C}/state`);
  log(`reveal ok; pool now ${state1.bundleCount} bundles, anchored root ${state1.anchoredRoot?.slice(0, 16)}…`);
  console.log(`\nOK — wallets and bundles under ${DIR}`);
} finally {
  await client.disconnect();
}

async function poolOf() {
  // The pool account is public knowledge; the coordinator exposes it only through its anchors, so
  // the client takes it from the environment (config.json → POOL) — set POOL=r… .
  const pool = process.env.POOL;
  must(pool, "set POOL=<pool account> (from the quorum's config.json)");
  return pool;
}
