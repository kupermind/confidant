// Snapshot of the public log for a public mirror (IPFS or any static host). Read-only: fetches a
// coordinator's /log, /state, /config and /fees, writes ONE JSON file in the same shape a node's
// `--rebuild --mirror <url>` accepts (it reads `.records`), pins it, and can publish the CID on the
// ledger. Nothing secret is in the log: every record is public by construction and every bundle is
// checked by hash against the ledger's anchors.
//
//   COORDINATOR_URL=https://confidant.im/api node log-snapshot.mjs [--out DIR] [--no-pin] [--memo]
//
// Pinning (first that applies):
//   PIN_RPC_URL + PIN_TOKEN   any Kubo-compatible RPC (e.g. Filebase: https://rpc.filebase.io, bearer token) → POST /api/v0/add
//   PINATA_JWT                Pinata's pinFileToIPFS
//   `ipfs` on PATH            local daemon (public gateways then depend on the DHT finding it)
// Tokens come from the environment, never from the repo.
//
// --memo  also publishes the CID on the XRPL as a 1-drop Payment from OPERATOR_SEED (env) to the pool's
//         sink account with MemoType "confidant/snapshot" and MemoData = the CID, verifiable on the explorer.
//
//   then: confidant-node --rebuild --mirror https://<gateway>/ipfs/<CID> --pool … --signers … --quorum … [--fee-*]
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";
import { http, memoOf, resilientClient } from "./lib.mjs";

const C = (process.env.COORDINATOR_URL ?? "http://127.0.0.1:7200").replace(/\/$/, "");
const args = process.argv.slice(2);
const outDir = args.includes("--out") ? args[args.indexOf("--out") + 1] : "../../data/snapshots";
const noPin = args.includes("--no-pin") || args.includes("--no-ipfs");
const withMemo = args.includes("--memo");
mkdirSync(outDir, { recursive: true });

const [log, state, config, fees] = await Promise.all([
  http("GET", `${C}/log?from=0`), http("GET", `${C}/state`), http("GET", `${C}/config`).catch(() => null), http("GET", `${C}/fees`).catch(() => null),
]);
if (state.root !== log.root) throw new Error(`log and state disagree on the root (${log.root} vs ${state.root}) — retry`);
const takenAt = new Date().toISOString();
const snapshot = {
  // What a node needs to rebuild (plus the ledger): the records, checked by hash against the anchors.
  records: log.records,
  // Context, so a reader can tell which pool and which moment this is.
  state: { root: log.root, anchoredRoot: log.anchoredRoot, leafCount: log.leafCount, nullifierCount: log.nullifierCount, bundleCount: log.bundleCount, consumedDeposits: log.consumedDeposits }, pool: config?.pool ?? null, treasury: config?.fees?.treasury ?? null, fees: config?.fees ?? null,
  feeLedger: fees ? { accrued: fees.accrued, swept: fees.swept, outstandingBacking: fees.outstandingBacking } : null,
  coordinator: C, takenAt, format: 1,
};
const file = `${outDir}/confidant-log-${takenAt.replace(/[:.]/g, "-")}.json`;
writeFileSync(file, JSON.stringify(snapshot));
console.log(`snapshot: ${file} — ${snapshot.records.length} records, root ${state.root.slice(0, 16)}…, anchored ${state.anchoredRoot?.slice(0, 16) ?? "—"}…, pool ${snapshot.pool ?? "?"}`);

let cid = null;
async function pinWithKuboRpc(url, token, path) {
  const body = new FormData();
  body.append("file", new Blob([readFileSync(path)], { type: "application/json" }), basename(path));
  const r = await fetch(`${url.replace(/\/$/, "")}/api/v0/add?cid-version=1&pin=true`, { method: "POST", headers: { authorization: `Bearer ${token}` }, body });
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status} ${(await r.text()).slice(0, 160)}`);
  const text = await r.text(); // one JSON object per line; the last is the file
  return JSON.parse(text.trim().split("\n").pop()).Hash;
}
async function pinWithPinata(jwt, path) {
  const body = new FormData();
  body.append("file", new Blob([readFileSync(path)], { type: "application/json" }), basename(path));
  body.append("pinataOptions", JSON.stringify({ cidVersion: 1 }));
  const r = await fetch("https://api.pinata.cloud/pinning/pinFileToIPFS", { method: "POST", headers: { authorization: `Bearer ${jwt}` }, body });
  if (!r.ok) throw new Error(`pinata: HTTP ${r.status} ${(await r.text()).slice(0, 160)}`);
  return (await r.json()).IpfsHash;
}
if (!noPin) {
  try {
    if (process.env.PIN_RPC_URL && process.env.PIN_TOKEN) { cid = await pinWithKuboRpc(process.env.PIN_RPC_URL, process.env.PIN_TOKEN, file); console.log(`pinned via ${process.env.PIN_RPC_URL}: CID ${cid}`); }
    else if (process.env.PINATA_JWT) { cid = await pinWithPinata(process.env.PINATA_JWT, file); console.log(`pinned via Pinata: CID ${cid}`); }
    else { cid = execFileSync("ipfs", ["add", "-Q", "--cid-version", "1", "--pin=true", file], { encoding: "utf8" }).trim(); console.log(`ipfs: added and pinned on the local daemon, CID ${cid} (public gateways resolve it once the DHT finds this node — a pinning service is faster)`); }
    console.log(`       gateways: https://ipfs.io/ipfs/${cid}  https://dweb.link/ipfs/${cid}`);
  } catch (e) {
    console.log(`pin: not done (${String(e.message).split("\n")[0]}) — set PIN_RPC_URL+PIN_TOKEN or PINATA_JWT, or install ipfs; the file is ${file}`);
  }
}
if (withMemo && cid) {
  // Publish the CID on the ledger: 1 drop operator → sink, MemoType confidant/snapshot, MemoData = CID.
  // The sink (not the pool) so the balances page never counts it as pool funding.
  const seed = process.env.OPERATOR_SEED;
  if (!seed) console.log("memo: OPERATOR_SEED not set — skipped");
  else if (!config?.sink || !config?.xrplWs) console.log("memo: coordinator config has no sink/xrplWs — skipped");
  else {
    const { Client, Wallet } = await import("xrpl");
    const client = resilientClient(new Client(config.xrplWs));
    await client.connect();
    try {
      const w = Wallet.fromSeed(seed);
      const tx = await client.autofill({ TransactionType: "Payment", Account: w.address, Destination: config.sink, Amount: "1", Memos: [memoOf("confidant/snapshot", Buffer.from(cid, "utf8").toString("hex"))] });
      const res = await client.submitAndWait(w.sign(tx).tx_blob);
      console.log(`memo: ${res.result.meta?.TransactionResult} ${res.result.hash} — ${config.explorer ?? "https://testnet.xrpl.org"}/transactions/${res.result.hash}`);
    } finally { await client.disconnect(); }
  }
}
const pin = cid ? `https://<gateway>/ipfs/${cid}` : `https://<gateway>/ipfs/<CID>`;
console.log(`\nverify from the public mirror alone (plus the ledger):\n  confidant-node --rebuild --mirror ${pin} --listen 127.0.0.1:7999 --state /tmp/rebuilt.verifier-state.json --xrpl-rpc https://s.altnet.rippletest.net:51234 --pool ${snapshot.pool ?? "<pool>"} --signers <r1,r2,r3> --quorum <k>${snapshot.fees ? ` --fee-shield-bps ${snapshot.fees.shieldBps} --fee-unshield-bps ${snapshot.fees.unshieldBps} --fee-treasury ${snapshot.treasury}` : ""}\n  then GET http://127.0.0.1:7999/state must show root ${state.root.slice(0, 16)}… and the same anchoredRoot.`);
