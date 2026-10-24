// Renders the systemd EnvironmentFiles for a quorum from the config.json that setup-pool.mjs wrote.
//
//   node render-env.mjs <config.json> <envDir> <dataDir> [basePort=7700] [xrplRpc] [xrplWs]
//
// Ports (all 127.0.0.1): nodes basePort+i (i = 0 coordinator's node, 1..N signers), sidecars
// basePort+100+i, coordinator basePort+200. Writes common.env, node-<i>.env, signer-<i>.env,
// coordinator.env. Key files stay where setup-pool put them (0600); only their paths are written.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";

const [configPath, envDir, dataDir, basePortArg, xrplRpcArg, xrplWsArg] = process.argv.slice(2);
// Optional boundary screening for every node: SCREENING_LIST (file path on the host) and
// SCREENING_STANDBY_LEDGERS; the unit passes them as --screening-list / --screening-standby-ledgers.
const screeningList = process.env.SCREENING_LIST ?? "";
const screeningStandby = Number(process.env.SCREENING_STANDBY_LEDGERS ?? 0);
// Optional credential gate for every node (XLS-70, feature A): CREDENTIAL_ISSUER (r…), CREDENTIAL_TYPE (hex),
// CREDENTIAL_TYPE_TEXT (label); the unit passes them as --credential-issuer / --credential-type / --credential-type-text.
const credIssuer = process.env.CREDENTIAL_ISSUER ?? "", credType = process.env.CREDENTIAL_TYPE ?? "", credText = process.env.CREDENTIAL_TYPE_TEXT ?? "";
if ((credIssuer && !credType) || (!credIssuer && credType)) { console.error("CREDENTIAL_ISSUER and CREDENTIAL_TYPE go together"); process.exit(2); }
if (!configPath || !envDir || !dataDir) { console.error("usage: render-env.mjs <config.json> <envDir> <dataDir> [basePort] [xrplRpc] [xrplWs]"); process.exit(2); }
const config = JSON.parse(readFileSync(configPath, "utf8"));
const basePort = Number(basePortArg ?? 7700);
const xrplRpc = xrplRpcArg ?? "https://s.altnet.rippletest.net:51234";
const xrplWs = xrplWsArg ?? "wss://s.altnet.rippletest.net:51233";
const n = config.signers.length;
const nodeUrl = (i) => `http://127.0.0.1:${basePort + i}`;
const signerUrl = (i) => `http://127.0.0.1:${basePort + 100 + i}`;
mkdirSync(envDir, { recursive: true });
const write = (name, kv) => writeFileSync(`${envDir}/${name}`, Object.entries(kv).map(([k, v]) => `${k}=${v}`).join("\n") + "\n", { mode: 0o640 });

const feeArgs = config.fees && config.treasury ? `--fee-shield-bps ${config.fees.shieldBps ?? 0} --fee-unshield-bps ${config.fees.unshieldBps ?? 0} --fee-treasury ${config.treasury}` : "";
write("common.env", { POOL: config.pool, SINK: config.sink, QUORUM: config.quorum, SIGNERS: config.signers.map((s) => s.address).join(","), XRPL_RPC: xrplRpc, FEE_ARGS: feeArgs,
  SCREENING_ARGS: [screeningList ? `--screening-list ${screeningList}` : "", screeningStandby > 0 ? `--screening-standby-ledgers ${screeningStandby}` : ""].filter(Boolean).join(" "),
  CREDENTIAL_ARGS: credIssuer ? [`--credential-issuer ${credIssuer}`, `--credential-type ${credType}`, credText ? `--credential-type-text ${credText}` : ""].filter(Boolean).join(" ") : "" });
for (let i = 0; i <= n; i++) write(`node-${i}.env`, { LISTEN: `127.0.0.1:${basePort + i}`, STATE: `${dataDir}/node${i}.verifier-state.json` });
for (let i = 1; i <= n; i++) write(`signer-${i}.env`, { PORT: basePort + 100 + i, NODE_URL: nodeUrl(i), KEY_FILE: config.signers[i - 1].keyFile, MAX_FEE_DROPS: 5000 });
// The coordinator's SIGNERS are sidecar URLs (it overrides common.env's SIGNERS, which the nodes use as addresses).
write("coordinator.env", { PORT: basePort + 200, NODE_URL: nodeUrl(0), NODES: Array.from({ length: n + 1 }, (_, i) => nodeUrl(i)).join(","), SIGNERS: Array.from({ length: n }, (_, i) => signerUrl(i + 1)).join(","), XRPL_WS: xrplWs });
// The demo wallet service behind the UI prototype (port basePort+250), syncing from the coordinator.
write("wallet.env", { LISTEN: `127.0.0.1:${basePort + 250}`, DIR: `${dataDir.replace(/\/quorum$/, "")}/wallets`, COORDINATOR_URL: `http://127.0.0.1:${basePort + 200}` });
console.log(`rendered ${envDir}: common.env, node-0..${n}.env, signer-1..${n}.env, coordinator.env (coordinator on 127.0.0.1:${basePort + 200}), wallet.env (demo wallet service on 127.0.0.1:${basePort + 250})`);
