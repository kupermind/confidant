// End-to-end on XRPL testnet — v2 (trust-minimized): the SPEC demo script with real Payments,
// client-side proving, a 2-of-3 SignerList quorum of independent verifiers, one anchor memo per
// operation, and a new node rebuilding from the ledger.
//
//   wallet(alice)  build shield  ──hash──▶ deposit Payment alice → pool, memo confidant/shield = hash
//   coordinator    validates deposit on-ledger, accepts, proposes anchor memo
//   signers ×3     each re-validates the deposit, endorses on ITS OWN state, signs the anchor
//   pool           multisigned anchor Payment (memo confidant/anchor), master key disabled
//   wallet(alice)  build transfer → bob      (nothing on ledger but the anchor memo)
//   wallet(bob)    build unshield            signers endorse, sign anchor + payout
//   new node       account_tx → memos → rebuild from mirror → root == last memo
//   negatives      lying coordinator (wrong root) and a lagging signer are refused
//
// The core is driven through `core/examples/cli.rs` (wallet + verifier roles). Testnet only.
// Usage:  npm install && CLI=../../target/release/examples/cli npm run e2e

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync, readFileSync } from "node:fs";
import { Client, Wallet, xrpToDrops, convertStringToHex, convertHexToString, multisign } from "xrpl";
import { resilientClient } from "../../backend/quorum/lib.mjs";

const XRPL_WS = process.env.XRPL_WS ?? "wss://s.altnet.rippletest.net:51233";
const CLI = process.env.CLI ?? "../../target/release/examples/cli";
const DEPOSIT_XRP = Number(process.env.DEPOSIT_XRP ?? "25");
const DEPOSIT_DROPS = xrpToDrops(DEPOSIT_XRP);
const EXPLORER = "https://testnet.xrpl.org";
const MEMO_SHIELD = "confidant/shield";
const MEMO_ANCHOR = "confidant/anchor";
const RUN = new Date().toISOString().replace(/[:.]/g, "-");
const DIR = `../../data/e2e-${RUN}`;
const QUORUM = 2;
const SIGNERS = 3;

mkdirSync(DIR, { recursive: true });
const report = { run: RUN, network: XRPL_WS, steps: [], links: [], anchors: [] };
const t0 = Date.now();
const step = (name, data = {}) => {
  report.steps.push({ name, t_ms: Date.now() - t0, ...data });
  console.log(`[${String(Date.now() - t0).padStart(6)} ms] ${name}`, Object.keys(data).length ? data : "");
};
const link = (label, hash) => {
  report.links.push({ label, url: `${EXPLORER}/transactions/${hash}` });
  console.log(`    ${label}: ${EXPLORER}/transactions/${hash}`);
};
function assert(cond, msg) {
  if (!cond) {
    report.failed = msg;
    writeFileSync(`${DIR}/report.json`, JSON.stringify(report, null, 2));
    throw new Error(`ASSERTION FAILED: ${msg}`);
  }
}

// ---- core CLI wrappers --------------------------------------------------------------------
function cli(args, { allowFail = false } = {}) {
  try {
    return JSON.parse(execFileSync(CLI, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }));
  } catch (e) {
    if (allowFail) return { error: (e.stderr || e.message || "").toString().trim() };
    throw e;
  }
}
const walletCli = (name) => (...args) => cli(["wallet", "--file", `${DIR}/${name}.wallet.json`, ...args]);
const verifierCli = (name) => (...args) => cli(["verifier", "--state", `${DIR}/${name}.verifier-state.json`, ...args]);
const verifierTry = (name) => (...args) => cli(["verifier", "--state", `${DIR}/${name}.verifier-state.json`, ...args], { allowFail: true });

const alice = walletCli("alice");
const bob = walletCli("bob");
const coord = verifierCli("coordinator");
const signers = Array.from({ length: SIGNERS }, (_, i) => ({ name: `signer${i + 1}`, v: verifierCli(`signer${i + 1}`), tryV: verifierTry(`signer${i + 1}`) }));
const LOG = `${DIR}/log.json`;
const exportLog = () => coord("export-log", LOG);

// ---- XRPL helpers -------------------------------------------------------------------------
async function singleSign(client, from, tx) {
  const prepared = await client.autofill({ Account: from.address, ...tx });
  const signed = from.sign(prepared);
  const res = await client.submitAndWait(signed.tx_blob);
  assert(res.result.meta?.TransactionResult === "tesSUCCESS", `${tx.TransactionType} ${signed.hash} failed: ${res.result.meta?.TransactionResult}`);
  return { hash: signed.hash, result: res.result };
}

/** Multisign a pool-account transaction with the signers that endorsed it (≥ quorum). */
async function quorumSign(client, poolAddress, tx, signerWallets) {
  assert(signerWallets.length >= QUORUM, `only ${signerWallets.length} signers endorsed, quorum is ${QUORUM}`);
  const prepared = await client.autofill({ Account: poolAddress, ...tx }, signerWallets.length);
  const blobs = signerWallets.map((w) => w.sign(prepared, true).tx_blob);
  const combined = multisign(blobs);
  const res = await client.submitAndWait(combined);
  const code = res.result.meta?.TransactionResult;
  assert(code === "tesSUCCESS", `multisigned ${tx.TransactionType} failed: ${code}`);
  return { hash: res.result.hash, result: res.result };
}

/** What every verifier (coordinator and each signer) does before accepting a shield. */
async function validateDeposit(client, hash, poolAddress) {
  const r = (await client.request({ command: "tx", transaction: hash })).result;
  const json = r.tx_json ?? r;
  assert(r.validated === true, "deposit not validated");
  assert(json.TransactionType === "Payment", "deposit is not a Payment");
  assert(json.Destination === poolAddress, "deposit destination is not the pool");
  const memo = json.Memos?.[0]?.Memo;
  assert(memo && convertHexToString(memo.MemoType) === MEMO_SHIELD, "deposit memo missing");
  return { txHash: hash, delivered: Number(r.meta.delivered_amount), memoHash: memo.MemoData.toLowerCase() };
}

async function balanceDrops(client, address) {
  const r = await client.request({ command: "account_info", account: address, ledger_index: "validated" });
  return r.result.account_data.Balance;
}

// ---- one operation through the quorum ------------------------------------------------------
/**
 * Coordinator accepts on its state and proposes the anchor memo; every signer validates on its
 * OWN state (`endorse`) and only then signs. Returns the memo + the signer wallets that endorsed.
 */
async function runThroughQuorum(client, poolAddress, label, submission, coordAccept, signerWallets, extraDepositCheck) {
  const acc = coordAccept();
  const memo = coord("anchor-memo", String(acc.bundleIndex));
  writeFileSync(`${DIR}/sub-${acc.bundleIndex}.json`, JSON.stringify(submission));
  const endorsed = [];
  for (const [i, s] of signers.entries()) {
    if (extraDepositCheck) await extraDepositCheck(); // each signer re-validates the deposit itself
    const e = s.tryV("endorse", `${DIR}/sub-${acc.bundleIndex}.json`, memo.memoHex);
    assert(!e.error, `${s.name} refused an honest ${label}: ${e.error}`);
    assert(e.newRoot === acc.newRoot, `${s.name} computed a different root`);
    endorsed.push(signerWallets[i]);
  }
  return { acc, memo, endorsed };
}

async function anchor(client, poolAddress, sink, memo, endorsed, label) {
  const a = await quorumSign(client, poolAddress, {
    TransactionType: "Payment", Destination: sink.address, Amount: "1",
    Memos: [{ Memo: { MemoType: convertStringToHex(MEMO_ANCHOR), MemoData: memo.memoHex.toUpperCase() } }],
  }, endorsed.slice(0, QUORUM));
  report.anchors.push({ root: memo.root, hash: a.hash, memoHex: memo.memoHex });
  coord("anchored", memo.root);
  for (const s of signers) s.v("anchored", memo.root);
  link(`anchor ${label} (root ${memo.root.slice(0, 12)}…, ${memo.memoHex.length / 2} B)`, a.hash);
  return a;
}

async function main() {
  const client = resilientClient(new Client(XRPL_WS));
  await client.connect();
  step("connected", { network: XRPL_WS });

  // ---- accounts: faucet funds the pool, Alice and an anchor sink; signers and Bob's exit are fresh keys
  const { wallet: pool } = await client.fundWallet();
  const { wallet: aliceXrpl } = await client.fundWallet();
  const { wallet: sink } = await client.fundWallet();
  const signerWallets = Array.from({ length: SIGNERS }, () => Wallet.generate());
  const bobFresh = Wallet.generate();
  step("faucet-funded pool, alice, sink; generated 3 signer keys + bob's fresh address", {
    pool: pool.address, alice: aliceXrpl.address, sink: sink.address, bobFresh: bobFresh.address,
    signers: signerWallets.map((w) => w.address),
  });

  // ---- step 3 of v2: the pool account becomes a 2-of-3 SignerList, master key disabled
  const sls = await singleSign(client, pool, {
    TransactionType: "SignerListSet", SignerQuorum: QUORUM,
    SignerEntries: signerWallets.map((w) => ({ SignerEntry: { Account: w.address, SignerWeight: 1 } })),
  });
  link("SignerListSet 2-of-3 on the pool account", sls.hash);
  const dm = await singleSign(client, pool, { TransactionType: "AccountSet", SetFlag: 4 /* asfDisableMaster */ });
  link("AccountSet asfDisableMaster (pool master key disabled)", dm.hash);
  const info = (await client.request({ command: "account_info", account: pool.address, signer_lists: true })).result;
  assert((info.account_data.Flags & 0x00100000) !== 0, "master key still enabled");
  // rippled API v2 returns signer lists at the result's top level; v1 nested them in account_data.
  const signerList = (info.signer_lists ?? info.account_data.signer_lists ?? [])[0];
  assert(signerList?.SignerQuorum === QUORUM && signerList.SignerEntries?.length === SIGNERS, "signer list not set");
  step("pool account is now a 2-of-3 quorum with the master key disabled");

  // ---- core: wallets (keys stay in the wallet files), coordinator + 3 signer verifiers
  const aliceInfo = alice("init");
  const bobInfo = bob("init");
  const auditorKeyForBob = bobInfo.viewingKey;
  exportLog();
  step("core: alice and bob wallets created; coordinator + 3 signer verifiers empty", { bobAddress: bobInfo.address.slice(0, 16) + "…" });

  // ---- 1. shield: build first, deposit with memo = bundle hash, then accept against the deposit
  const built = alice("build-shield", DEPOSIT_DROPS, `${DIR}/shield.json`, LOG);
  const deposit = await singleSign(client, aliceXrpl, {
    TransactionType: "Payment", Destination: pool.address, Amount: DEPOSIT_DROPS,
    Memos: [{ Memo: { MemoType: convertStringToHex(MEMO_SHIELD), MemoData: built.bundleHash.toUpperCase() } }],
  });
  link("deposit alice → pool (memo = bundle hash)", deposit.hash);
  const dep = await validateDeposit(client, deposit.hash, pool.address);
  assert(dep.memoHash === built.bundleHash, "deposit memo does not carry the bundle hash");
  assert(dep.delivered === Number(DEPOSIT_DROPS), "delivered_amount mismatch");
  const shieldRecord = JSON.parse(readFileSync(`${DIR}/shield.json`, "utf8"));
  const shieldSub = { op: "shield", record: shieldRecord, deposit: { txHash: dep.txHash, delivered: dep.delivered, memoHash: dep.memoHash } };
  const s1 = await runThroughQuorum(client, pool.address, "shield", shieldSub,
    () => coord("submit-shield", `${DIR}/shield.json`, dep.txHash, String(dep.delivered), dep.memoHash),
    signerWallets, () => validateDeposit(client, deposit.hash, pool.address));
  step("shield accepted by coordinator and endorsed by 3/3 signers", { root: s1.acc.newRoot.slice(0, 16) + "…" });
  // A stranger's deposit cannot be claimed twice: the same deposit against a fresh bundle is refused.
  exportLog();
  const again = alice("build-shield", DEPOSIT_DROPS, `${DIR}/shield-again.json`, LOG);
  const dup = verifierTry("coordinator")("submit-shield", `${DIR}/shield-again.json`, dep.txHash, String(dep.delivered), again.bundleHash);
  assert(dup.error && /already backs a shield|does not commit/.test(dup.error), `duplicate deposit accepted: ${JSON.stringify(dup)}`);
  await anchor(client, pool.address, sink, s1.memo, s1.endorsed, "after shield");

  // ---- 2. private transfer: nothing on the ledger but the anchor; a lying coordinator is refused
  exportLog();
  alice("build-transfer", bobInfo.address, DEPOSIT_DROPS, `${DIR}/transfer.json`, LOG);
  const seqBefore = (await client.request({ command: "account_info", account: pool.address })).result.account_data.Sequence;
  const transferSub = { op: "transfer", record: JSON.parse(readFileSync(`${DIR}/transfer.json`, "utf8")) };
  const s2 = await runThroughQuorum(client, pool.address, "transfer", transferSub, () => coord("submit-transfer", `${DIR}/transfer.json`), signerWallets);
  const seqAfter = (await client.request({ command: "account_info", account: pool.address })).result.account_data.Sequence;
  assert(seqBefore === seqAfter, "the pool account transacted during the private hop");
  step("private transfer accepted and endorsed; nothing on the ledger yet", { root: s2.acc.newRoot.slice(0, 16) + "…" });
  // Negative: a coordinator proposing a wrong root for the NEXT op is refused by every signer.
  exportLog();
  bob("build-unshield", DEPOSIT_DROPS, bobFresh.address, `${DIR}/unshield.json`, LOG);
  const unshieldSub = { op: "unshield", record: JSON.parse(readFileSync(`${DIR}/unshield.json`, "utf8")), destination: bobFresh.address };
  writeFileSync(`${DIR}/sub-lie.json`, JSON.stringify(unshieldSub));
  const honestAcc = coord("submit-unshield", `${DIR}/unshield.json`, bobFresh.address);
  const honestMemo = coord("anchor-memo", String(honestAcc.bundleIndex));
  const lieHex = honestMemo.memoHex.slice(0, 4) + (honestMemo.memoHex[4] === "0" ? "1" : "0") + honestMemo.memoHex.slice(5); // flip a nibble of the root
  // Before signers see the transfer's anchor as final, anchor it (they already endorsed it).
  await anchor(client, pool.address, sink, s2.memo, s2.endorsed, "after transfer");
  const refusals = signers.map((s) => s.tryV("endorse", `${DIR}/sub-lie.json`, lieHex));
  assert(refusals.every((r) => r.error && /does not match|root/.test(r.error)), `a signer signed a root it did not compute: ${JSON.stringify(refusals)}`);
  step("lying coordinator (wrong root) refused by 3/3 signers; no signature produced");

  // ---- 3. unshield: honest memo endorsed, anchor + payout multisigned by the quorum
  const endorsed = [];
  for (const [i, s] of signers.entries()) {
    const e = s.tryV("endorse", `${DIR}/sub-lie.json`, honestMemo.memoHex);
    assert(!e.error, `${s.name} refused the honest unshield: ${e.error}`);
    assert(e.payout.destination === bobFresh.address && e.payout.amount === Number(DEPOSIT_DROPS), "payout mismatch");
    endorsed.push(signerWallets[i]);
  }
  await anchor(client, pool.address, sink, honestMemo, endorsed, "after unshield");
  const payout = await quorumSign(client, pool.address, { TransactionType: "Payment", Destination: bobFresh.address, Amount: String(honestAcc.payout.amount) }, endorsed.slice(0, QUORUM));
  link("payout pool → bob's fresh address (2-of-3 multisig)", payout.hash);
  assert((await balanceDrops(client, bobFresh.address)) === String(honestAcc.payout.amount), "fresh address balance mismatch");
  step("unshield endorsed by 3/3, anchor + payout multisigned 2-of-3; funds arrived", { destination: bobFresh.address });

  // ---- 4. selective disclosure on any verifier (a signer's state here)
  const reveal = signers[1].v("reveal", auditorKeyForBob);
  assert(reveal.balance === 0 && reveal.notes.length === 1 && reveal.notes[0].spent === true, "auditor should see exactly bob's one spent note");
  step("auditor reveal (bob) on signer2's state", { balance: reveal.balance, notes: reveal.notes.length });

  // ---- 5. a new node joins from the ledger: account_tx → anchor memos → rebuild from the mirror
  const txs = (await client.request({ command: "account_tx", account: pool.address, ledger_index_min: -1, ledger_index_max: -1, forward: true })).result.transactions;
  const memosOnLedger = txs
    .map((t) => (t.tx_json ?? t.tx))
    .filter((j) => j && j.Account === pool.address && j.Memos?.[0]?.Memo && convertHexToString(j.Memos[0].Memo.MemoType) === MEMO_ANCHOR)
    .map((j) => j.Memos[0].Memo.MemoData.toLowerCase());
  assert(memosOnLedger.length === 3, `expected 3 anchor memos on ledger, found ${memosOnLedger.length}`);
  assert(memosOnLedger.every((m, i) => m === report.anchors[i].memoHex), "memos on ledger differ from what the quorum signed");
  writeFileSync(`${DIR}/memos.json`, JSON.stringify(memosOnLedger));
  exportLog();
  // Backing on join is MANDATORY: for every shield memo on the ledger, the joining node looks the
  // deposit up itself and hands the core the ledger facts; the core refuses any shield whose
  // deposit is missing, delivers a different amount, or commits to another bundle.
  const deposits = {};
  for (const hex of memosOnLedger) {
    const bytes = Buffer.from(hex, "hex");
    const op = bytes[1], extLen = bytes[66], ext = bytes.subarray(67, 67 + extLen).toString("utf8");
    if (op === 1 /* shield */) {
      const d = await validateDeposit(client, ext, pool.address);
      deposits[ext] = { txHash: d.txHash, delivered: d.delivered, memoHash: d.memoHash };
    }
  }
  assert(Object.keys(deposits).length === 1, `expected 1 shield deposit to re-validate, got ${Object.keys(deposits).length}`);
  writeFileSync(`${DIR}/deposits.json`, JSON.stringify(deposits));
  const node = verifierCli("newnode")("rebuild", `${DIR}/memos.json`, LOG, `${DIR}/deposits.json`);
  const coordState = coord("state");
  assert(node.root === coordState.root && node.root === report.anchors[2].root, "rebuilt root != last anchored root");
  assert(node.bundleCount === 3 && node.consumedDeposits === 1, "rebuilt state differs");
  // Negative: a node fed a wrong deposit amount for the shield refuses to join.
  const wrongDeposits = JSON.parse(JSON.stringify(deposits));
  for (const k of Object.keys(wrongDeposits)) wrongDeposits[k].delivered -= 1;
  writeFileSync(`${DIR}/deposits-wrong.json`, JSON.stringify(wrongDeposits));
  const refused = verifierTry("newnode-wrong")("rebuild", `${DIR}/memos.json`, LOG, `${DIR}/deposits-wrong.json`);
  assert(refused.error && /value balance mismatch/.test(refused.error), `node joined on an unbacked shield: ${JSON.stringify(refused)}`);
  const noDeposits = verifierTry("newnode-nodep")("rebuild", `${DIR}/memos.json`, LOG, `${DIR}/empty.json`);
  writeFileSync(`${DIR}/empty.json`, "{}");
  const refused2 = verifierTry("newnode-nodep")("rebuild", `${DIR}/memos.json`, LOG, `${DIR}/empty.json`);
  assert(refused2.error && /needs a deposit reference/.test(refused2.error), `node joined without deposit facts: ${JSON.stringify(refused2)}`);
  void noDeposits;
  step("new node rebuilt from 3 on-ledger memos + mirror with every shield deposit re-validated; wrong/missing deposit facts refused", { bundles: node.bundleCount });

  // ---- 6. a lagging signer (never saw the log) cannot endorse; after rebuilding it agrees
  const lagging = verifierTry("lagging");
  const lag = lagging("endorse", `${DIR}/sub-lie.json`, honestMemo.memoHex);
  assert(lag.error, "a signer with no state endorsed an unshield");
  const caughtUp = verifierCli("lagging")("rebuild", `${DIR}/memos.json`, LOG, `${DIR}/deposits.json`);
  assert(caughtUp.root === coordState.root, "lagging signer did not catch up");
  step("lagging signer refused, then caught up from the ledger");

  report.final = { state: coordState, explorer: { pool: `${EXPLORER}/accounts/${pool.address}`, bobFresh: `${EXPLORER}/accounts/${bobFresh.address}` } };
  writeFileSync(`${DIR}/report.json`, JSON.stringify(report, null, 2));
  console.log(`\nOK — report: ${DIR}/report.json`);
  await client.disconnect();
}

main().catch((e) => { console.error(e); process.exit(1); });
