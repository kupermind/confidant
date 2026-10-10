// Builds a screening list for the verifier nodes from OFAC's SDN list: every "Digital Currency
// Address - XRP" entry, plus optional extra addresses (e.g. a local blocklist). Public data only.
//
//   node fetch-ofac.mjs [--out <file>] [--extra <file>...] [--source <url>]
//   default out: ../../data/screening/ofac-xrp.txt ; default source: OFAC SDN XML (~30 MB)
//
// The output is the node's --screening-list format: one address per line, `#` comments.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const args = process.argv.slice(2);
const opt = (name, def) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : def; };
const extras = args.flatMap((a, i) => (a === "--extra" ? [args[i + 1]] : []));
const out = opt("--out", "../../data/screening/ofac-xrp.txt");
const source = opt("--source", "https://www.treasury.gov/ofac/downloads/sdn.xml");

const res = await fetch(source, { redirect: "follow", headers: { "user-agent": "confidant-screening/1" } });
if (!res.ok) { console.error(`fetch ${source}: HTTP ${res.status}`); process.exit(1); }
const xml = await res.text();
const published = xml.match(/<Publish_Date>([^<]+)<\/Publish_Date>/)?.[1] ?? "unknown";
const addrs = new Set();
for (const m of xml.matchAll(/<idType>Digital Currency Address - XRP<\/idType>\s*<idNumber>([^<]+)<\/idNumber>/g)) addrs.add(m[1].trim());
const extraLines = [];
for (const f of extras) for (const line of readFileSync(f, "utf8").split("\n")) {
  const a = line.split("#")[0].trim(); if (a) { addrs.add(a); extraLines.push(a); }
}
mkdirSync(dirname(out), { recursive: true });
const body = [
  `# Confidant screening list — OFAC SDN "Digital Currency Address - XRP" entries`,
  `# source: ${source}`, `# SDN publish date: ${published} ; fetched: ${new Date().toISOString()}`,
  `# ${addrs.size} addresses (${extraLines.length} from --extra files)`,
  ...[...addrs].sort(),
].join("\n") + "\n";
writeFileSync(out, body);
console.log(`${out}: ${addrs.size} addresses (OFAC publish date ${published})`);
