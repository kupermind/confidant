// Snapshot of the public log for a public mirror (IPFS or any static host). Read-only: fetches a
// coordinator's /log, /state, /config and /fees, writes ONE JSON file in the same shape a node's
// `--rebuild --mirror <url>` accepts (it reads `.records`), and — if an `ipfs` CLI is on PATH — adds
// and pins it, printing the CID and the exact rebuild command. Nothing secret is in the log: every
// record is public by construction and every bundle is checked by hash against the ledger's anchors.
//
//   COORDINATOR_URL=https://confidant.im/api node log-snapshot.mjs [--out DIR] [--no-ipfs]
//   then: confidant-node --rebuild --mirror https://<gateway>/ipfs/<CID> --pool … --signers … --quorum … [--fee-*]
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { http } from "./lib.mjs";

const C = (process.env.COORDINATOR_URL ?? "http://127.0.0.1:7200").replace(/\/$/, "");
const args = process.argv.slice(2);
const outDir = args.includes("--out") ? args[args.indexOf("--out") + 1] : "../../data/snapshots";
const noIpfs = args.includes("--no-ipfs");
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
if (!noIpfs) {
  try {
    cid = execFileSync("ipfs", ["add", "-Q", "--cid-version", "1", "--pin=true", file], { encoding: "utf8" }).trim();
    console.log(`ipfs: added and pinned locally, CID ${cid}`);
    console.log(`       public gateways: https://ipfs.io/ipfs/${cid}  https://dweb.link/ipfs/${cid}  (reachable once a public pinner or your node is online)`);
  } catch (e) {
    console.log(`ipfs: CLI not available or add failed (${e.message.split("\n")[0]}) — upload ${file} to a pinning service (Pinata / Filebase / web3.storage) and use the CID it returns`);
  }
}
const pin = cid ? `https://<gateway>/ipfs/${cid}` : `https://<gateway>/ipfs/<CID>`;
console.log(`\nverify from the public mirror alone (plus the ledger):\n  confidant-node --rebuild --mirror ${pin} --listen 127.0.0.1:7999 --state /tmp/rebuilt.verifier-state.json --xrpl-rpc https://s.altnet.rippletest.net:51234 --pool ${snapshot.pool ?? "<pool>"} --signers <r1,r2,r3> --quorum <k>${snapshot.fees ? ` --fee-shield-bps ${snapshot.fees.shieldBps} --fee-unshield-bps ${snapshot.fees.unshieldBps} --fee-treasury ${snapshot.treasury}` : ""}\n  then GET http://127.0.0.1:7999/state must show root ${state.root.slice(0, 16)}… and the same anchoredRoot.`);
