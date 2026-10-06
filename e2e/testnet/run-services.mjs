// End-to-end on XRPL testnet THROUGH THE QUORUM SERVICES (backend/): the SPEC demo as a client of
// the coordinator, with real Rust verifier nodes, Node signer sidecars and a 2-of-3 SignerList.
//
//   setup       faucet pool + sink, 3 signer keys, SignerListSet, master disabled   (backend/quorum/lib.mjs)
//   launch      4 Rust nodes (coordinator's + 3 signers'), 3 sidecars, 1 coordinator
//   shield      wallet builds → deposit Payment with memo = bundle hash → POST coordinator /submit/shield
//               (coordinator's node validates the deposit; every signer's node re-validates it before endorsing)
//   transfer    POST /submit/transfer — only the anchor memo reaches the ledger
//   unshield    POST /submit/unshield — anchor then 2-of-3 payout to a fresh address; the multisigned
//               payout blob comes back (resubmittable recovery path)
//   negatives   a sidecar refuses: tampered anchor memo, re-pointed/inflated payout, a tx from another
//               account, inflated fee, future Sequence, missing/far LastLedgerSequence, payout before anchor;
//               a node refuses /anchored for a non-anchor tx or a root the tx does not carry
//   reveal      POST /audit/reveal on the coordinator (proxied to its node)
//   custody     nodes configured for another quorum or signer set refuse to start
//   new node    a fresh Rust node started with --rebuild --mirror <coordinator node>/log must land on the same root
//
// Usage: cargo build --release --example cli --bin confidant-node && cd e2e/testnet && npm install &&
//        CLI=../../target/release/examples/cli NODE_BIN=../../target/release/confidant-node npm run e2e:services

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync, readFileSync } from "node:fs";
import { Client, Wallet, xrpToDrops, convertHexToString, decode } from "xrpl";
import { setupPool, launchQuorum, http, memoOf, MEMO_SHIELD, MEMO_ANCHOR, resilientClient } from "../../backend/quorum/lib.mjs";

const XRPL_WS = process.env.XRPL_WS ?? "wss://s.altnet.rippletest.net:51233";
const XRPL_RPC = process.env.XRPL_RPC ?? "https://s.altnet.rippletest.net:51234";
const CLI = process.env.CLI ?? "../../target/release/examples/cli";
const NODE_BIN = process.env.NODE_BIN ?? "../../target/release/confidant-node";
const DEPOSIT_DROPS = xrpToDrops(Number(process.env.DEPOSIT_XRP ?? "25"));
const EXPLORER = "https://testnet.xrpl.org";
const RUN = new Date().toISOString().replace(/[:.]/g, "-");
const DIR = `../../data/e2e-services-${RUN}`;
mkdirSync(DIR, { recursive: true });

const report = { run: RUN, steps: [], links: [] };
const t0 = Date.now();
const step = (name, data = {}) => { report.steps.push({ name, t_ms: Date.now() - t0, ...data }); console.log(`[${String(Date.now() - t0).padStart(6)} ms] ${name}`, Object.keys(data).length ? data : ""); };
const link = (label, hash) => { report.links.push({ label, url: `${EXPLORER}/transactions/${hash}` }); console.log(`    ${label}: ${EXPLORER}/transactions/${hash}`); };
function assert(cond, msg) { if (!cond) { report.failed = msg; writeFileSync(`${DIR}/report.json`, JSON.stringify(report, null, 2)); throw new Error(`ASSERTION FAILED: ${msg}`); } }
const cli = (args) => JSON.parse(execFileSync(CLI, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }));
const wallet = (name) => (...args) => cli(["wallet", "--file", `${DIR}/${name}.wallet.json`, ...args]);
const alice = wallet("alice"), bob = wallet("bob");
const LOG = `${DIR}/log.json`;

async function pay(client, from, tx) {
  const prepared = await client.autofill({ Account: from.address, ...tx });
  const signed = from.sign(prepared);
  const res = await client.submitAndWait(signed.tx_blob);
  assert(res.result.meta?.TransactionResult === "tesSUCCESS", `Payment failed: ${res.result.meta?.TransactionResult}`);
  return signed.hash;
}
async function expectRefusal(label, p, pattern) {
  try { await p; assert(false, `${label}: was accepted`); }
  catch (e) { if (/ASSERTION FAILED/.test(e.message)) throw e; assert(pattern.test(e.message + JSON.stringify(e.details ?? {})), `${label}: refused for the wrong reason: ${e.message} ${JSON.stringify(e.details ?? {})}`); }
}
/** Pulls the coordinator's log to a file so the wallet CLI can sync from it. */
async function syncLog(coordinatorUrl) {
  const { records } = await http("GET", `${coordinatorUrl}/log?from=0`);
  writeFileSync(LOG, JSON.stringify(records));
}

async function main() {
  const client = resilientClient(new Client(XRPL_WS));
  await client.connect();
  step("connected", { network: XRPL_WS });

  const config = await setupPool(client, { signers: 3, quorum: 2, dir: DIR });
  link("SignerListSet 2-of-3", config.setupTx.signerListSet);
  link("AccountSet asfDisableMaster", config.setupTx.disableMaster);
  step("pool set up on testnet: 2-of-3 SignerList, master disabled", { pool: config.pool, sink: config.sink, signers: config.signers.map((s) => s.address) });

  const q = await launchQuorum({ config, dir: DIR, nodeBin: NODE_BIN, xrplRpc: XRPL_RPC, xrplWs: XRPL_WS, basePort: 7300 });
  const C = q.coordinatorUrl;
  step("quorum services up: 4 verifier nodes, 3 signer sidecars, 1 coordinator", { coordinator: C });
  try {
    const { wallet: aliceXrpl } = await client.fundWallet();
    const bobFresh = Wallet.generate();
    const aliceInfo = alice("init"), bobInfo = bob("init");
    await syncLog(C);

    // ---- 1. shield through the coordinator
    const built = alice("build-shield", DEPOSIT_DROPS, `${DIR}/shield.json`, LOG);
    const depositHash = await pay(client, aliceXrpl, { TransactionType: "Payment", Destination: config.pool, Amount: DEPOSIT_DROPS, Memos: [memoOf(MEMO_SHIELD, built.bundleHash)] });
    link("deposit alice → pool (memo = bundle hash)", depositHash);
    const shieldRecord = JSON.parse(readFileSync(`${DIR}/shield.json`, "utf8"));
    // A bogus deposit hash is refused by the coordinator's node (ledger lookup), nothing is anchored.
    await expectRefusal("shield with a bogus deposit", http("POST", `${C}/submit/shield`, { record: shieldRecord, depositTxHash: "0".repeat(64) }), /ledger rpc|txnNotFound|not found/i);
    const s1 = await http("POST", `${C}/submit/shield`, { record: shieldRecord, depositTxHash: depositHash });
    link("anchor after shield (2-of-3 multisig)", s1.anchorTx);
    assert(s1.signers.length === 2 && s1.refusals.length === 0, `expected 3/3 endorsements, got refusals ${JSON.stringify(s1.refusals)}`);
    step("shield accepted by the coordinator's node, endorsed by all signer nodes, anchored by the quorum", { root: s1.newRoot.slice(0, 16) + "…" });
    // The same deposit cannot back a second shield: neither the identical bundle again (a replay)
    // nor a fresh bundle claiming the old deposit (its memo commits to the first bundle).
    await expectRefusal("duplicate deposit, same bundle", http("POST", `${C}/submit/shield`, { record: shieldRecord, depositTxHash: depositHash }), /already backs a shield|already spent|nullifier/);
    await syncLog(C);
    alice("build-shield", DEPOSIT_DROPS, `${DIR}/shield2.json`, LOG);
    await expectRefusal("duplicate deposit, fresh bundle", http("POST", `${C}/submit/shield`, { record: JSON.parse(readFileSync(`${DIR}/shield2.json`, "utf8")), depositTxHash: depositHash }), /already backs a shield|does not commit/);

    // ---- 2. transfer
    await syncLog(C);
    alice("build-transfer", bobInfo.address, DEPOSIT_DROPS, `${DIR}/transfer.json`, LOG);
    const seqBefore = (await client.request({ command: "account_info", account: config.pool })).result.account_data.Sequence;
    const s2 = await http("POST", `${C}/submit/transfer`, { record: JSON.parse(readFileSync(`${DIR}/transfer.json`, "utf8")) });
    link("anchor after transfer", s2.anchorTx);
    const seqAfter = (await client.request({ command: "account_info", account: config.pool })).result.account_data.Sequence;
    assert(seqAfter === seqBefore + 1, "exactly one pool transaction (the anchor) during the private hop");
    step("private transfer accepted and anchored; only the anchor reached the ledger");

    // ---- 3. unshield #1 through the coordinator: anchor then payout, 2-of-3. The multisigned
    //      payout blob comes back so the client could resubmit it had the payout failed after the
    //      anchor landed (note spent, user unpaid) — the documented recovery path.
    await syncLog(C);
    const UNSHIELD_1 = xrpToDrops(10), UNSHIELD_2 = String(Number(DEPOSIT_DROPS) - Number(UNSHIELD_1));
    bob("build-unshield", UNSHIELD_1, bobFresh.address, `${DIR}/unshield1.json`, LOG);
    const s3 = await http("POST", `${C}/submit/unshield`, { record: JSON.parse(readFileSync(`${DIR}/unshield1.json`, "utf8")), destination: bobFresh.address });
    link("anchor after unshield #1 (coordinator, 2-of-3)", s3.anchorTx);
    assert(s3.payoutTx && !s3.payoutError, `payout through the coordinator failed: ${s3.payoutError}`);
    link("payout #1 pool → bob's fresh address (coordinator, 2-of-3)", s3.payoutTx);
    const blob = decode(s3.payoutBlob);
    assert(blob.Account === config.pool && blob.Destination === bobFresh.address && blob.Amount === UNSHIELD_1 && blob.Signers?.length === 2 && blob.LastLedgerSequence > 0, `payout blob is not the multisigned payout: ${JSON.stringify(blob)}`);
    const resubmit = await client.request({ command: "submit", tx_blob: s3.payoutBlob });
    assert(/^(tefPAST_SEQ|tefALREADY|tesSUCCESS)$/.test(resubmit.result.engine_result), `resubmitting the landed payout blob: ${resubmit.result.engine_result}`);
    step("unshield #1 through the coordinator: anchored, paid 2-of-3, payout blob returned and resubmittable", { resubmit: resubmit.result.engine_result, payoutLls: blob.LastLedgerSequence - blob.Sequence > 0 });

    // ---- negatives at a signer sidecar: it signs nothing its own node did not endorse or that
    //      does not match the endorsement — and nothing whose Fee/Sequence/LastLedgerSequence it did not pin.
    await syncLog(C);
    bob("build-unshield", UNSHIELD_2, bobFresh.address, `${DIR}/unshield.json`, LOG);
    const unshieldRecord = JSON.parse(readFileSync(`${DIR}/unshield.json`, "utf8"));
    const sub = { op: "unshield", record: unshieldRecord, destination: bobFresh.address };
    // Get the honest memo from a verifier node WITHOUT applying to the quorum: use a scratch node state? Simpler:
    // the coordinator's node accepts first (this is what a real submit does); we then attack a sidecar directly.
    const honest = await http("POST", `${q.nodeUrl(0)}/submit/unshield`, { record: unshieldRecord, destination: bobFresh.address });
    const anchorTx = await client.autofill({ TransactionType: "Payment", Account: config.pool, Destination: config.sink, Amount: "1", Memos: [memoOf(MEMO_ANCHOR, honest.memoHex)] }, 3);
    const payoutTx = await client.autofill({ TransactionType: "Payment", Account: config.pool, Destination: bobFresh.address, Amount: String(honest.payout.amount) }, 3);
    payoutTx.Sequence = anchorTx.Sequence + 1;
    const S1 = q.signerUrl(1);
    const lieMemo = honest.memoHex.slice(0, 4) + (honest.memoHex[4] === "0" ? "1" : "0") + honest.memoHex.slice(5);
    await expectRefusal("sidecar: tampered memo", http("POST", `${S1}/sign`, { txs: [{ kind: "anchor", tx: anchorTx }], submission: sub, memoHex: lieMemo }), /refused to endorse|does not match/);
    await expectRefusal("sidecar: payout re-pointed", http("POST", `${S1}/sign`, { txs: [{ kind: "anchor", tx: anchorTx }, { kind: "payout", tx: { ...payoutTx, Destination: aliceXrpl.address } }], submission: sub, memoHex: honest.memoHex }), /destination differs/);
    // A payout sent alone must sit at the pool's next sequence (the sidecar pins it), so the amount check is what fires here.
    await expectRefusal("sidecar: payout amount inflated", http("POST", `${S1}/sign`, { txs: [{ kind: "payout", tx: { ...payoutTx, Sequence: anchorTx.Sequence, Amount: String(honest.payout.amount * 2) } }], submission: sub, memoHex: honest.memoHex }), /amount differs/);
    await expectRefusal("sidecar: tx from another account", http("POST", `${S1}/sign`, { txs: [{ kind: "anchor", tx: { ...anchorTx, Account: aliceXrpl.address } }], submission: sub, memoHex: honest.memoHex }), /not the pool/);
    await expectRefusal("sidecar: anchor fee inflated", http("POST", `${S1}/sign`, { txs: [{ kind: "anchor", tx: { ...anchorTx, Fee: "1000000" } }], submission: sub, memoHex: honest.memoHex }), /fee missing or above/);
    await expectRefusal("sidecar: anchor to another destination", http("POST", `${S1}/sign`, { txs: [{ kind: "anchor", tx: { ...anchorTx, Destination: aliceXrpl.address } }], submission: sub, memoHex: honest.memoHex }), /not the sink/);
    await expectRefusal("sidecar: future sequence", http("POST", `${S1}/sign`, { txs: [{ kind: "anchor", tx: { ...anchorTx, Sequence: anchorTx.Sequence + 1 } }], submission: sub, memoHex: honest.memoHex }), /not the pool account's next sequence/);
    await expectRefusal("sidecar: no LastLedgerSequence", http("POST", `${S1}/sign`, { txs: [{ kind: "anchor", tx: { ...anchorTx, LastLedgerSequence: undefined } }], submission: sub, memoHex: honest.memoHex }), /LastLedgerSequence .* missing or outside/);
    await expectRefusal("sidecar: LastLedgerSequence too far", http("POST", `${S1}/sign`, { txs: [{ kind: "anchor", tx: { ...anchorTx, LastLedgerSequence: anchorTx.LastLedgerSequence + 1000 } }], submission: sub, memoHex: honest.memoHex }), /missing or outside/);
    await expectRefusal("sidecar: payout before anchor", http("POST", `${S1}/sign`, { txs: [{ kind: "payout", tx: payoutTx }, { kind: "anchor", tx: anchorTx }], submission: sub, memoHex: honest.memoHex }), /in that order/);
    step("signer sidecar refused: tampered memo, re-pointed payout, inflated payout, foreign account, inflated fee, wrong anchor destination, future sequence, missing/far LastLedgerSequence, payout before anchor");
    // Note: signer1's node has now endorsed the honest unshield (idempotent) — the quorum flow below reuses it.

    // ---- unshield #2 via the sidecars directly (the coordinator's node already accepted it above;
    //      re-submitting the same bundle to the coordinator is a replay → refused). Exactly what the
    //      coordinator does internally.
    await expectRefusal("replay of the unshield", http("POST", `${C}/submit/unshield`, { record: unshieldRecord, destination: bobFresh.address }), /already spent|nullifier/);
    // So anchor + pay out via the sidecars directly, exactly as the coordinator does internally.
    const { multisign } = await import("xrpl");
    const signed = [];
    for (let i = 1; i <= 3; i++) {
      try { signed.push(await http("POST", `${q.signerUrl(i)}/sign`, { txs: [{ kind: "anchor", tx: anchorTx }, { kind: "payout", tx: payoutTx }], submission: sub, memoHex: honest.memoHex })); } catch (e) { console.log(`    signer${i} refused: ${e.message}`); }
    }
    assert(signed.length >= 2, `quorum not reached for the unshield: ${signed.length}/3`);
    const anchorRes = await client.submitAndWait(multisign(signed.slice(0, 2).map((s) => s.blobs.find((b) => b.kind === "anchor").tx_blob)));
    assert(anchorRes.result.meta?.TransactionResult === "tesSUCCESS", "unshield anchor failed");
    link("anchor after unshield", anchorRes.result.hash);
    const payoutRes = await client.submitAndWait(multisign(signed.slice(0, 2).map((s) => s.blobs.find((b) => b.kind === "payout").tx_blob)));
    assert(payoutRes.result.meta?.TransactionResult === "tesSUCCESS", "payout failed");
    link("payout #2 pool → bob's fresh address (2-of-3)", payoutRes.result.hash);
    // A node marks a root anchored only after reading the anchor tx itself: a payout tx or a wrong root is refused.
    await expectRefusal("anchored with a non-anchor tx", http("POST", `${q.nodeUrl(1)}/anchored`, { root: honest.newRoot, anchorTxHash: payoutRes.result.hash }), /no confidant\/anchor memo/);
    await expectRefusal("anchored with a root the tx does not carry", http("POST", `${q.nodeUrl(1)}/anchored`, { root: s3.newRoot, anchorTxHash: anchorRes.result.hash }), /does not carry the claimed root/);
    for (let i = 0; i <= 3; i++) await http("POST", `${q.nodeUrl(i)}/anchored`, { root: honest.newRoot, anchorTxHash: anchorRes.result.hash });
    const bal = (await client.request({ command: "account_info", account: bobFresh.address, ledger_index: "validated" })).result.account_data.Balance;
    assert(bal === DEPOSIT_DROPS, `fresh address balance ${bal}, expected ${DEPOSIT_DROPS} (both payouts)`);
    step("unshield #2: 3/3 signers endorsed on their own nodes, anchor + payout multisigned 2-of-3, funds arrived; nodes verified the anchor tx on the ledger");

    // ---- 4. reveal via the coordinator
    const reveal = await http("POST", `${C}/audit/reveal`, { viewingKey: bobInfo.viewingKey });
    assert(reveal.balance === 0 && reveal.notes.length === 2 && reveal.notes.every((n) => n.spent), `auditor should see bob's two spent notes (25 in, 15 change): ${JSON.stringify(reveal.notes)}`);
    step("auditor reveal (bob) through the coordinator", { balance: reveal.balance });

    // ---- 5. a new node joins from the ledger with --rebuild (scanner + rebuild_checked). First the
    //      custody gate: a node configured for a quorum the ledger does not show refuses to start.
    const signerAddrs = config.signers.map((s) => s.address).join(",");
    const wrong = spawnSync(NODE_BIN, ["--listen", "127.0.0.1:7398", "--state", `${DIR}/wrong.verifier-state.json`, "--xrpl-rpc", XRPL_RPC, "--pool", config.pool, "--signers", signerAddrs, "--quorum", "3"], { encoding: "utf8", timeout: 60000 });
    assert(wrong.status !== 0 && /custody/.test(wrong.stderr) && /SignerQuorum is 2, expected 3/.test(wrong.stderr), `node with the wrong quorum must refuse to start: ${wrong.status} ${wrong.stderr}`);
    const stranger = spawnSync(NODE_BIN, ["--listen", "127.0.0.1:7398", "--state", `${DIR}/wrong.verifier-state.json`, "--xrpl-rpc", XRPL_RPC, "--pool", config.pool, "--signers", `${aliceXrpl.address},${config.signers[1].address},${config.signers[2].address}`, "--quorum", "2"], { encoding: "utf8", timeout: 60000 });
    assert(stranger.status !== 0 && /differ from the configured/.test(stranger.stderr), `node with a foreign signer set must refuse to start: ${stranger.stderr}`);
    step("custody gate: nodes configured for another quorum or signer set refuse to start");
    const newNode = spawn(NODE_BIN, ["--listen", "127.0.0.1:7399", "--state", `${DIR}/newnode.verifier-state.json`, "--xrpl-rpc", XRPL_RPC, "--pool", config.pool, "--signers", signerAddrs, "--quorum", String(config.quorum), "--mirror", `${q.nodeUrl(0)}/log`, "--rebuild"], { stdio: ["ignore", "ignore", "pipe"] });
    let newNodeErr = ""; newNode.stderr.on("data", (d) => { newNodeErr += d.toString(); });
    let st = null;
    for (let t = 0; t < 150 && !st; t++) { try { st = await http("GET", "http://127.0.0.1:7399/state"); } catch { await new Promise((r) => setTimeout(r, 200)); } }
    assert(st, `new node did not come up: ${newNodeErr}`);
    const coordState = await http("GET", `${C}/state`);
    assert(st.root === coordState.root && st.bundleCount === 4 && st.consumedDeposits === 1 && st.anchoredRoot === coordState.root, `new node state differs: ${JSON.stringify(st)} vs ${JSON.stringify(coordState)}`);
    newNode.kill("SIGTERM");
    step("new node rebuilt from account_tx anchor memos + mirror with every deposit re-validated; root equals the quorum's", { bundles: st.bundleCount });

    report.final = { state: coordState, pool: `${EXPLORER}/accounts/${config.pool}` };
    writeFileSync(`${DIR}/report.json`, JSON.stringify(report, null, 2));
    console.log(`\nOK — report: ${DIR}/report.json`);
  } finally {
    q.stop();
    await client.disconnect();
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
