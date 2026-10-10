// Replayable screening audit: anyone can check, from the public log and the public ledger, that a
// pool never admitted a shield from a listed depositor nor paid an unshield to a listed address.
// The pool's entries and exits are public XRPL transactions, so no viewing key is involved.
//
//   node audit.mjs --list <file> [--log <nodeUrl>/log | --log-file <records.json>] [--xrpl-rpc <url>]
//   exit 0 = clean, 1 = at least one listed party found, 2 = usage / lookup error
import { readFileSync } from "node:fs";
import { xrplRpc, http } from "../lib.mjs";

const args = process.argv.slice(2);
const opt = (name, def) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : def; };
const listFile = opt("--list"); const logUrl = opt("--log"); const logFile = opt("--log-file");
const rpc = opt("--xrpl-rpc", "https://s.altnet.rippletest.net:51234");
if (!listFile || (!logUrl && !logFile)) { console.error("usage: audit.mjs --list <file> (--log <url> | --log-file <file>) [--xrpl-rpc <url>]"); process.exit(2); }

const listed = new Set(readFileSync(listFile, "utf8").split("\n").map((l) => l.split("#")[0].trim()).filter(Boolean));
const records = logUrl ? (await http("GET", logUrl.includes("?") ? logUrl : `${logUrl}?from=0`)).records : JSON.parse(readFileSync(logFile, "utf8"));
let hits = 0, shields = 0, unshields = 0;
for (const r of records) {
  if (r.op === "shield") {
    shields++;
    const tx = await xrplRpc(rpc, "tx", { transaction: r.external });
    const account = (tx.tx_json ?? tx).Account;
    const bad = listed.has(account);
    if (bad) hits++;
    console.log(`#${r.index} shield   deposit ${r.external.slice(0, 12)}… from ${account} ${bad ? "LISTED" : "ok"}`);
  } else if (r.op === "unshield") {
    unshields++;
    const bad = listed.has(r.external);
    if (bad) hits++;
    console.log(`#${r.index} unshield to ${r.external} ${bad ? "LISTED" : "ok"}`);
  }
}
console.log(`\n${records.length} bundles: ${shields} shields, ${unshields} unshields, ${listed.size} listed addresses, ${hits} hit(s) — ${hits ? "VIOLATION" : "clean"}`);
process.exit(hits ? 1 : 0);
