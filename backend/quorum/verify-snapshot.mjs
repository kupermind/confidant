// Verifies a public log snapshot the way a stranger would — the operator rehearsal from
// docs/DATA-AVAILABILITY.md as one command, with a verdict and an exit code:
//
//   1. fetch the snapshot (an IPFS gateway URL, or any URL a node's --mirror accepts);
//   2. read the pool's SignerList and quorum from the ledger itself, the fee policy from the snapshot;
//   3. start a throwaway confidant-node with --rebuild --mirror <that URL> on a spare port — it
//      re-reads every anchor memo from the ledger and re-verifies every bundle by hash and proof;
//   4. compare what it reached (roots, counts, fee ledger) with the snapshot, and with the live
//      coordinator when one is given.
//
//   SNAPSHOT_URL=https://gateway.pinata.cloud/ipfs/<CID> NODE_BIN=../../target/release/confidant-node \
//     [COORDINATOR_URL=https://confidant.im/api] [XRPL_RPC=https://s.altnet.rippletest.net:51234] \
//     [POOL=r… SIGNERS=r1,r2,r3 QUORUM=2 FEE_ARGS="--fee-shield-bps 25 …"]   node verify-snapshot.mjs
//
// Nothing is written except a temporary state file, deleted afterwards; the node holds no keys and
// never signs. Exit 0 = every comparison matched; 1 = a mismatch or the rebuild refused (stale or
// altered snapshot); 2 = usage.
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { http } from "./lib.mjs";

const env = process.env;
const SNAPSHOT_URL = env.SNAPSHOT_URL ?? process.argv[2];
const NODE_BIN = env.NODE_BIN ?? "../../target/release/confidant-node";
const C = env.COORDINATOR_URL?.replace(/\/$/, "");
const RPC = env.XRPL_RPC ?? "https://s.altnet.rippletest.net:51234";
const PORT = Number(env.PORT ?? 7999);
const TIMEOUT_S = Number(env.TIMEOUT_S ?? 300);
if (!SNAPSHOT_URL) { console.error("usage: SNAPSHOT_URL=<url> NODE_BIN=<confidant-node> node verify-snapshot.mjs"); process.exit(2); }
if (!existsSync(NODE_BIN)) { console.error(`NODE_BIN not found: ${NODE_BIN}`); process.exit(2); }

const t0 = Date.now();
const log = (m) => console.log(`[${String(Date.now() - t0).padStart(6)} ms] ${m}`);
const xrp = (d) => (Number(d) / 1e6).toFixed(6);
let failures = 0;
const same = (what, a, b) => { const ok = String(a) === String(b); if (!ok) failures++; console.log(`  ${ok ? "ok  " : "FAIL"} ${what}: ${a}${ok ? "" : `  ≠  ${b}`}`); return ok; };
const rpc = async (method, params) => { const r = await http("POST", RPC, { method, params: [params] }); if (r.result?.status === "error" || r.result?.error) throw new Error(`${method}: ${r.result.error_message ?? r.result.error}`); return r.result; };

// 1. The snapshot.
const snap = await http("GET", SNAPSHOT_URL);
if (!Array.isArray(snap.records) || !snap.state?.root) { console.error("not a log snapshot: expected { records: [...], state: { root } }"); process.exit(2); }
const POOL = env.POOL ?? snap.pool;
if (!POOL) { console.error("no pool account: set POOL=r… (the snapshot carries none)"); process.exit(2); }
log(`snapshot ${SNAPSHOT_URL}: ${snap.records.length} records, root ${snap.state.root.slice(0, 16)}…, pool ${POOL}, taken ${snap.takenAt ?? "?"}`);

// 2. Live pool, when a coordinator is given: is the snapshot current?
let live = null, liveFees = null;
if (C) {
  live = await http("GET", `${C}/state`); liveFees = await http("GET", `${C}/fees`).catch(() => null);
  console.log("against the live coordinator:");
  same("pool", (await http("GET", `${C}/config`)).pool, POOL);
  same("root", snap.state.root, live.root);
  if (!same("bundle count", snap.records.length, live.bundleCount)) {
    log(`the live pool has moved on since the snapshot (${live.bundleCount - snap.records.length} bundle(s) with anchors but no bytes in this object); the rebuild below is expected to refuse — take a fresh snapshot`);
  }
}

// 3. Custody facts from the ledger (what a stranger has), fee policy from the snapshot.
const info = await rpc("account_info", { account: POOL, signer_lists: true, ledger_index: "validated" });
const list = info.account_data?.signer_lists?.[0];
const SIGNERS = env.SIGNERS ?? list?.SignerEntries?.map((e) => e.SignerEntry.Account).join(",");
const QUORUM = env.QUORUM ?? list?.SignerQuorum;
if (!SIGNERS || !QUORUM) { console.error("the pool account has no SignerList on the ledger and none was given (SIGNERS/QUORUM)"); process.exit(2); }
const fees = snap.fees ?? {};
const FEE_ARGS = env.FEE_ARGS ? env.FEE_ARGS.split(/\s+/).filter(Boolean)
  : [...(fees.shieldBps ? ["--fee-shield-bps", String(fees.shieldBps)] : []), ...(fees.unshieldBps ? ["--fee-unshield-bps", String(fees.unshieldBps)] : []), ...(fees.treasury ? ["--fee-treasury", fees.treasury] : [])];
log(`ledger: SignerList ${SIGNERS.split(",").length} signers, quorum ${QUORUM}; fee policy ${FEE_ARGS.join(" ") || "off"}`);

// 4. A throwaway node rebuilds from the ledger plus the snapshot.
const dir = mkdtempSync(join(tmpdir(), "confidant-verify-"));
const state = join(dir, "rebuilt.verifier-state.json");
const args = ["--rebuild", "--mirror", SNAPSHOT_URL, "--listen", `127.0.0.1:${PORT}`, "--state", state, "--xrpl-rpc", RPC, "--pool", POOL, "--signers", SIGNERS, "--quorum", String(QUORUM), ...FEE_ARGS, ...(env.SCREENING_ARGS ? env.SCREENING_ARGS.split(/\s+/).filter(Boolean) : [])];
log(`rebuilding: ${NODE_BIN} ${args.map((a) => (a === SNAPSHOT_URL ? "<snapshot url>" : a)).join(" ")}`);
const node = spawn(NODE_BIN, args, { stdio: ["ignore", "pipe", "pipe"] });
let out = ""; node.stdout.on("data", (d) => (out += d)); node.stderr.on("data", (d) => (out += d));
let exited = null; node.on("exit", (code) => (exited = code ?? -1));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let rebuilt = null;
for (let i = 0; i < TIMEOUT_S / 2 && exited === null; i++) {
  await sleep(2000);
  try { rebuilt = await http("GET", `http://127.0.0.1:${PORT}/state`); break; } catch {}
}
const interesting = out.split("\n").filter((l) => /custody|rebuild|mirror|error|refus|panic/i.test(l)).slice(0, 8);
for (const l of interesting) log(`node: ${l.slice(0, 160)}`);
try {
  if (!rebuilt) {
    failures++;
    console.log(exited !== null ? `  FAIL the node refused to serve (exit ${exited}) — a stale or altered snapshot, or the ledger disagrees` : `  FAIL no /state within ${TIMEOUT_S} s`);
  } else {
    const f = await http("GET", `http://127.0.0.1:${PORT}/fees`).catch(() => null);
    console.log("rebuilt node vs the snapshot:");
    for (const k of ["root", "anchoredRoot", "leafCount", "nullifierCount", "bundleCount", "consumedDeposits"]) same(k, rebuilt[k], snap.state[k]);
    if (f && snap.feeLedger) { same("fee ledger accrued", f.accrued, snap.feeLedger.accrued); same("fee ledger swept", f.swept, snap.feeLedger.swept); same("outstanding backing", f.outstandingBacking, snap.feeLedger.outstandingBacking); }
    if (live) { console.log("rebuilt node vs the live coordinator:"); same("root", rebuilt.root, live.root); same("anchored root", rebuilt.anchoredRoot, live.anchoredRoot); same("leaves", rebuilt.leafCount, live.leafCount); same("nullifiers", rebuilt.nullifierCount, live.nullifierCount); if (f && liveFees) same("outstanding backing", f.outstandingBacking, liveFees.outstandingBacking); }
    if (f) log(`rebuilt fee ledger: accrued ${xrp(f.accrued)} · swept ${xrp(f.swept)} · backing ${xrp(f.outstandingBacking)} XRP`);
  }
} finally {
  node.kill("SIGTERM"); await sleep(500); if (exited === null) node.kill("SIGKILL");
  rmSync(dir, { recursive: true, force: true });
}
console.log(failures ? `\nFAILED — ${failures} mismatch(es)` : `\nOK — a stranger rebuilds this pool from the XRP Ledger plus the snapshot and reaches the same state (${((Date.now() - t0) / 1000).toFixed(1)} s)`);
process.exit(failures ? 1 : 0);
