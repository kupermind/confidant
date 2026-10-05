// End-to-end on XRPL testnet: the SPEC demo script with real Payments.
//
//   Alice (XRPL) --deposit Payment--> pool account        → core: shield
//   pool --memo Payment (root)--> anchor sink              → core: anchored
//   core: private transfer alice → bob (nothing on ledger)
//   pool --memo Payment (root)--> anchor sink              → core: anchored
//   core: unshield bob → payout                            → pool --Payment--> fresh address
//   core: auditor reveal (bob's viewing key)
//   third party: export-log + verify-log, roots match the memos on ledger
//
// The core is driven through `core/examples/cli.rs` (a test tool standing in for the backend).
// Everything XRPL happens here with xrpl.js, exactly where SPEC puts it. Testnet only.
//
// Usage:  npm install && CLI=../../target/release/examples/cli npm run e2e
// Env:    CLI (path to the built cli example), XRPL_WS (default testnet), DEPOSIT_XRP (default 25)

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { Client, Wallet, xrpToDrops, convertStringToHex, convertHexToString } from "xrpl";

const XRPL_WS = process.env.XRPL_WS ?? "wss://s.altnet.rippletest.net:51233";
const CLI = process.env.CLI ?? "../../target/release/examples/cli";
const DEPOSIT_XRP = Number(process.env.DEPOSIT_XRP ?? "25");
const DEPOSIT_DROPS = xrpToDrops(DEPOSIT_XRP);
const EXPLORER = "https://testnet.xrpl.org";
const MEMO_TYPE = "confidant/root";
const RUN = new Date().toISOString().replace(/[:.]/g, "-");
const STATE = `../../data/e2e-${RUN}.pool-state.json`;
const LOG = `../../data/e2e-${RUN}.log.json`;
const REPORT = `../../data/e2e-${RUN}.report.json`;

const report = { run: RUN, network: XRPL_WS, steps: [], links: [] };
const t0 = Date.now();
function step(name, data = {}) {
  const row = { name, t_ms: Date.now() - t0, ...data };
  report.steps.push(row);
  console.log(`[${String(row.t_ms).padStart(6)} ms] ${name}`, Object.keys(data).length ? data : "");
}
function link(label, hash) {
  const url = `${EXPLORER}/transactions/${hash}`;
  report.links.push({ label, url });
  console.log(`    ${label}: ${url}`);
}
function assert(cond, msg) {
  if (!cond) {
    report.failed = msg;
    writeFileSync(REPORT, JSON.stringify(report, null, 2));
    throw new Error(`ASSERTION FAILED: ${msg}`);
  }
}
function core(...args) {
  const out = execFileSync(CLI, ["--state", STATE, ...args], { encoding: "utf8" });
  return JSON.parse(out);
}

async function pay(client, from, tx) {
  const prepared = await client.autofill({ TransactionType: "Payment", Account: from.address, ...tx });
  const signed = from.sign(prepared);
  const res = await client.submitAndWait(signed.tx_blob);
  const code = res.result.meta?.TransactionResult;
  assert(code === "tesSUCCESS", `Payment ${signed.hash} failed: ${code}`);
  assert(res.result.validated === true, `Payment ${signed.hash} not validated`);
  return { hash: signed.hash, result: res.result };
}

/** What the backend must do before minting: find the deposit on-ledger and check it. */
async function validateDeposit(client, hash, poolAddress, expectedDrops) {
  const tx = await client.request({ command: "tx", transaction: hash });
  const r = tx.result;
  const json = r.tx_json ?? r;
  assert(r.validated === true, "deposit not validated");
  assert(json.TransactionType === "Payment", "deposit is not a Payment");
  assert(json.Destination === poolAddress, "deposit destination is not the pool");
  const delivered = r.meta?.delivered_amount;
  assert(delivered === expectedDrops, `deposit delivered ${delivered}, expected ${expectedDrops}`);
  return true;
}

async function balanceDrops(client, address) {
  const r = await client.request({ command: "account_info", account: address, ledger_index: "validated" });
  return r.result.account_data.Balance;
}

async function memoRoot(client, hash) {
  const tx = await client.request({ command: "tx", transaction: hash });
  const json = tx.result.tx_json ?? tx.result;
  const memo = json.Memos?.[0]?.Memo;
  assert(memo && convertHexToString(memo.MemoType) === MEMO_TYPE, "anchor memo missing");
  return memo.MemoData.toLowerCase();
}

async function main() {
  mkdirSync("../../data", { recursive: true });
  const client = new Client(XRPL_WS);
  await client.connect();
  step("connected", { network: XRPL_WS });

  // --- accounts: faucet funds the pool, Alice and an anchor sink; Bob's exit address is fresh.
  const { wallet: pool } = await client.fundWallet();
  const { wallet: alice } = await client.fundWallet();
  const { wallet: sink } = await client.fundWallet();
  const bobFresh = Wallet.generate();
  step("faucet-funded pool, alice, anchor sink; generated bob's fresh address", {
    pool: pool.address, alice: alice.address, sink: sink.address, bobFresh: bobFresh.address,
  });

  // --- core: owners (the "auditor" receives Bob's viewing key at onboarding)
  const aliceInfo = core("register", "alice");
  const bobInfo = core("register", "bob");
  const auditorKeyForBob = bobInfo.viewingKey;
  step("core: registered alice, bob", { bobViewingKeyLen: auditorKeyForBob.length });

  // --- 1. shield: Alice deposits on XRPL, backend validates, core mints
  const deposit = await pay(client, alice, { Destination: pool.address, Amount: DEPOSIT_DROPS });
  link("deposit alice → pool", deposit.hash);
  await validateDeposit(client, deposit.hash, pool.address, DEPOSIT_DROPS);
  const shield = core("shield", "alice", DEPOSIT_DROPS, deposit.hash);
  step("shield", { leafIndex: shield.leafIndex, newRoot: shield.newRoot });

  // --- anchor the root: pool writes it into a memo Payment
  const anchorTxs = [];
  async function anchor(label) {
    const { root } = core("state");
    const a = await pay(client, pool, {
      Destination: sink.address, Amount: "1",
      Memos: [{ Memo: { MemoType: convertStringToHex(MEMO_TYPE), MemoData: root.toUpperCase() } }],
    });
    anchorTxs.push({ root, hash: a.hash });
    const st = core("anchored", root);
    assert(st.anchoredRoot === root, "core did not record the anchored root");
    link(`anchor ${label} (root ${root.slice(0, 12)}…)`, a.hash);
    return root;
  }
  await anchor("after shield");

  // --- 2. private transfer: nothing on the ledger
  const poolSeqBefore = (await client.request({ command: "account_info", account: pool.address })).result.account_data.Sequence;
  const transfer = core("transfer", "alice", "bob", DEPOSIT_DROPS);
  step("private transfer alice → bob", { nullifiers: transfer.nullifiers.length, newRoot: transfer.newRoot });
  const poolSeqAfter = (await client.request({ command: "account_info", account: pool.address })).result.account_data.Sequence;
  assert(poolSeqBefore === poolSeqAfter, "the pool account sent a transaction during the private hop");
  await anchor("after transfer");

  // --- 3. unshield: core authorizes, pool pays the fresh address
  const unshield = core("unshield", "bob", DEPOSIT_DROPS, bobFresh.address);
  assert(unshield.payout.destination === bobFresh.address, "payout destination mismatch");
  assert(unshield.payout.amount === Number(DEPOSIT_DROPS), "payout amount mismatch");
  const payout = await pay(client, pool, { Destination: unshield.payout.destination, Amount: String(unshield.payout.amount) });
  link("payout pool → bob's fresh address", payout.hash);
  const bobBalance = await balanceDrops(client, bobFresh.address);
  assert(bobBalance === String(unshield.payout.amount), `fresh address balance ${bobBalance}`);
  step("unshield + payout", { destination: bobFresh.address, drops: bobBalance });
  const finalRoot = await anchor("after unshield");

  // --- 4. selective disclosure
  const reveal = core("reveal", auditorKeyForBob);
  assert(reveal.balance === 0, "bob's balance after exit should be 0");
  assert(reveal.notes.length === 1 && reveal.notes[0].amount === Number(DEPOSIT_DROPS) && reveal.notes[0].spent === true,
    "auditor should see exactly bob's one (spent) note");
  step("auditor reveal (bob)", reveal);
  const aliceReveal = core("reveal", aliceInfo.viewingKey);
  assert(aliceReveal.notes.some((n) => n.direction === "outgoing" && n.recipient === bobInfo.address),
    "alice's key should show the outgoing payment to bob");

  // --- 5. third party: public log vs. the roots anchored on ledger
  core("export-log", LOG);
  const verified = core("verify-log", LOG);
  assert(verified.root === finalRoot, "replayed root != last anchored root");
  for (const a of anchorTxs) {
    const onLedger = await memoRoot(client, a.hash);
    assert(onLedger === a.root, `memo root ${onLedger} != ${a.root}`);
  }
  step("third-party replay of the public log matches the anchored roots", {
    bundles: verified.bundleCount, leaves: verified.leafCount, anchors: anchorTxs.length,
  });

  const state = core("state");
  report.final = { state, explorerAccounts: {
    pool: `${EXPLORER}/accounts/${pool.address}`, alice: `${EXPLORER}/accounts/${alice.address}`,
    bobFresh: `${EXPLORER}/accounts/${bobFresh.address}`,
  } };
  writeFileSync(REPORT, JSON.stringify(report, null, 2));
  console.log(`\nOK — report: ${REPORT}`);
  await client.disconnect();
}

main().catch((e) => { console.error(e); process.exit(1); });
