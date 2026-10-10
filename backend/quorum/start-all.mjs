// Starts N verifier nodes, N signer sidecars and the coordinator from data/quorum/config.json.
//
//   NODE_BIN=../../target/release/confidant-node DIR=../../data/quorum node start-all.mjs
//   optional: SCREENING_LIST=<file of listed XRPL addresses> SCREENING_STANDBY_LEDGERS=<n>

import { readFileSync } from "node:fs";
import { launchQuorum } from "./lib.mjs";

const DIR = process.env.DIR ?? "../../data/quorum";
const config = JSON.parse(readFileSync(`${DIR}/config.json`, "utf8"));
const q = await launchQuorum({
  config, dir: DIR, nodeBin: process.env.NODE_BIN ?? "../../target/release/confidant-node",
  xrplRpc: process.env.XRPL_RPC ?? "https://s.altnet.rippletest.net:51234",
  xrplWs: process.env.XRPL_WS ?? "wss://s.altnet.rippletest.net:51233", quiet: false,
  screeningList: process.env.SCREENING_LIST ?? null, screeningStandbyLedgers: Number(process.env.SCREENING_STANDBY_LEDGERS ?? 0),
});
console.log(`coordinator: ${q.coordinatorUrl}  nodes: ${config.signers.map((_, i) => q.nodeUrl(i + 1)).join(" ")}  (coordinator's node ${q.nodeUrl(0)})`);
process.on("SIGINT", () => { q.stop(); process.exit(0); });
