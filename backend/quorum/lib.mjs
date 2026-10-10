// Shared pieces of the quorum services: tiny HTTP helpers, XRPL memo helpers, the transaction
// shape checks a signer enforces, pool setup, and launching a whole quorum locally.

import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync, chmodSync, existsSync, readFileSync } from "node:fs";
import { Client, Wallet, convertStringToHex, convertHexToString, DisconnectedError, NotConnectedError, hashes } from "xrpl";

export const MEMO_ANCHOR = "confidant/anchor";
export const MEMO_SHIELD = "confidant/shield";

// ---- HTTP -----------------------------------------------------------------------------------
export function serve(port, routes) {
  const server = createServer(async (req, res) => {
    const url = new URL(req.url, "http://localhost");
    const key = `${req.method} ${url.pathname}`;
    const handler = routes[key];
    const send = (status, body) => {
      res.writeHead(status, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    if (!handler) return send(404, { error: `no route ${key}` });
    try {
      let body = null;
      if (req.method === "POST") {
        const chunks = [];
        for await (const c of req) chunks.push(c);
        body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      }
      const out = await handler(body, url);
      send(200, out);
    } catch (e) {
      send(e.status ?? 500, { error: e.message ?? String(e), ...(e.details ? { details: e.details } : {}) });
    }
  });
  // Node closes idle keep-alive sockets after 5 s by default; a client reusing one at that instant
  // sees ECONNRESET ("fetch failed"). Local services talk over long-lived connections: no idle cut.
  server.keepAliveTimeout = 0;
  return new Promise((resolve) => server.listen(port, "127.0.0.1", () => resolve(server)));
}

export function refuse(status, message, details) {
  const e = new Error(message);
  e.status = status;
  if (details) e.details = details;
  return e;
}

/** JSON over HTTP. A failure *before any response* (socket reset while reusing an idle keep-alive
 *  connection, refused connection during startup) is retried once; an HTTP error is thrown as-is. */
export async function http(method, url, body, { retries = 1 } = {}) {
  for (let attempt = 0; ; attempt++) {
    let res;
    try {
      res = await fetch(url, { method, headers: { "content-type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
    } catch (e) {
      if (attempt < retries) { await new Promise((r) => setTimeout(r, 200)); continue; }
      throw refuse(502, `${method} ${url}: ${e.cause?.code ?? e.cause?.message ?? e.message}`);
    }
    const text = await res.text();
    let json = null;
    try { json = text ? JSON.parse(text) : null; } catch { json = { error: text }; }
    if (!res.ok) throw refuse(res.status, json?.error ?? `HTTP ${res.status}`, json);
    return json;
  }
}

// ---- resilient XRPL client ---------------------------------------------------------------------
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const isDisconnect = (e) => e instanceof DisconnectedError || e instanceof NotConnectedError || /not open|CLOSING|CLOSED|websocket was closed|never cleaned up|NotConnected|Disconnected/i.test(e?.message ?? "");
/**
 * Wraps an xrpl.js Client so a dropped WebSocket (idle testnet connections close; slow provers leave
 * them idle for minutes) is reconnected and the call retried instead of failing the caller. A
 * `submitAndWait` interrupted by a disconnect first looks the transaction up by its hash — the submit
 * may have gone through — and resubmits only when the ledger does not know it. Everything else on
 * the client is passed through unchanged.
 */
export function resilientClient(client, { log = (m) => console.error(`[xrpl] ${m}`), retries = 3 } = {}) {
  client.on("error", (...a) => log(`connection error: ${a.map(String).join(" ")}`));
  client.on("disconnected", (code) => { if (code !== 1000) log(`disconnected (${code}); reconnecting on the next call`); }); // 1000 = our own disconnect()
  // xrpl.js refuses connect() while the old socket is still CLOSING ("Websocket connection never
  // cleaned up"): disconnect() first — it waits for the close, clears the socket and cancels xrpl.js's
  // own reconnect timer — then connect; a few attempts with backoff.
  const ensure = async () => {
    if (client.isConnected()) return;
    log("reconnecting");
    for (let i = 0; ; i++) {
      try { await client.disconnect().catch(() => {}); await client.connect(); return; } catch (e) {
        if (i >= 5) throw e;
        await sleep(1000 * (i + 1));
      }
    }
  };
  const call = async (fn) => {
    for (let attempt = 0; ; attempt++) {
      try { await ensure(); return await fn(); } catch (e) {
        if (!isDisconnect(e) || attempt >= retries) throw e;
        log(`${e.message} — retry ${attempt + 1}/${retries}`); await sleep(1000 * (attempt + 1));
      }
    }
  };
  const lookup = async (hash, tries) => {
    for (let i = 0; i < tries; i++) {
      try { await ensure(); const r = await client.request({ command: "tx", transaction: hash }); if (r.result?.validated) return { result: { ...r.result, hash } }; }
      catch (e) { if (!/txnNotFound/.test(e?.message ?? "") && !isDisconnect(e)) throw e; }
      await sleep(4000);
    }
    return null;
  };
  const submitAndWait = async (blob, opts) => {
    const hash = hashes.hashSignedTx(blob);
    for (let attempt = 0; ; attempt++) {
      try { await ensure(); return await client.submitAndWait(blob, opts); } catch (e) {
        if (attempt >= retries) throw e;
        if (isDisconnect(e)) log(`${e.message} during submitAndWait(${hash.slice(0, 8)}) — checking the ledger before resubmitting`);
        else if (/tefPAST_SEQ|tefALREADY/.test(e?.message ?? "")) log(`resubmit answered ${e.message.match(/tef\w+/)?.[0]} — checking whether the first submit landed`);
        else throw e;
        const found = await lookup(hash, 8);
        if (found) return found;
      }
    }
  };
  const wrapped = new Set(["request", "autofill", "fundWallet", "getLedgerIndex", "getXrpBalance"]);
  return new Proxy(client, {
    get(target, prop) {
      if (prop === "submitAndWait") return submitAndWait;
      if (wrapped.has(prop)) return (...args) => call(() => target[prop](...args));
      const v = Reflect.get(target, prop, target);
      return typeof v === "function" ? v.bind(target) : v;
    },
  });
}

// ---- XRPL memo helpers -----------------------------------------------------------------------
export const memoOf = (type, dataHex) => ({ Memo: { MemoType: convertStringToHex(type), MemoData: dataHex.toUpperCase() } });
export function firstMemo(txJson) {
  const m = txJson?.Memos?.[0]?.Memo;
  if (!m) return null;
  return { type: convertHexToString(m.MemoType), data: (m.MemoData ?? "").toLowerCase() };
}

// ---- what a signer may sign -------------------------------------------------------------------
/**
 * The ONLY transactions a signer sidecar signs, and only with the values its own node endorsed:
 *   anchor: Payment pool → sink, 1 drop, exactly one memo confidant/anchor == memoHex
 *   payout: Payment pool → endorse.payout.destination for exactly endorse.payout.amount drops
 * Anything else — another account, another destination, another amount, extra memos — is refused.
 */
export const DEFAULT_MAX_FEE_DROPS = 5000;
/** Ledgers (≈ 4 s each) a signed anchor may stay valid: short, so a withheld blob dies quickly. */
export const DEFAULT_ANCHOR_LLS_WINDOW = 60;
/** Ledgers a signed payout may stay valid: generous, so a payout that failed once can be resubmitted
 *  (by the coordinator or by the user holding the blob) after the note is already spent. */
export const DEFAULT_PAYOUT_LLS_WINDOW = 600;
/**
 * The sidecar's "no blind signing" guard for an XRPL transaction the coordinator prepared. Beyond
 * the Payment's shape (Account/Destination/Amount/memo) it pins what the coordinator could otherwise
 * choose freely: the Fee (paid by the pool — unbounded, it is a drain), the Sequence (must be the
 * pool account's next, as the sidecar itself read it; no pre-signing future sequences, one anchor in
 * flight per state) and LastLedgerSequence (present and within a window — no blob that lives forever).
 */
export function checkTxShape(kind, tx, {
  pool, sink, memoHex, endorse, expectedSequence, ledgerIndex,
  maxFeeDrops = DEFAULT_MAX_FEE_DROPS, anchorLlsWindow = DEFAULT_ANCHOR_LLS_WINDOW, payoutLlsWindow = DEFAULT_PAYOUT_LLS_WINDOW,
}) {
  if (tx?.TransactionType !== "Payment") throw refuse(409, `${kind}: not a Payment`);
  if (tx.Account !== pool) throw refuse(409, `${kind}: Account is not the pool`);
  if (!/^[0-9]{1,15}$/.test(String(tx.Fee)) || Number(tx.Fee) > maxFeeDrops) throw refuse(409, `${kind}: fee missing or above ${maxFeeDrops} drops`);
  if (!Number.isInteger(expectedSequence) || tx.Sequence !== expectedSequence) throw refuse(409, `${kind}: Sequence ${tx.Sequence} is not the pool account's next sequence ${expectedSequence}`);
  const window = kind === "payout" ? payoutLlsWindow : anchorLlsWindow;
  if (!Number.isInteger(ledgerIndex)) throw refuse(409, `${kind}: current ledger index unknown`);
  if (!Number.isInteger(tx.LastLedgerSequence) || tx.LastLedgerSequence <= ledgerIndex || tx.LastLedgerSequence > ledgerIndex + window) {
    throw refuse(409, `${kind}: LastLedgerSequence ${tx.LastLedgerSequence} missing or outside (${ledgerIndex}, ${ledgerIndex + window}]`);
  }
  if (kind === "anchor") {
    if (tx.Destination !== sink) throw refuse(409, "anchor: destination is not the sink");
    if (tx.Amount !== "1") throw refuse(409, "anchor: amount is not 1 drop");
    if (!Array.isArray(tx.Memos) || tx.Memos.length !== 1) throw refuse(409, "anchor: exactly one memo required");
    const m = firstMemo(tx);
    if (m.type !== MEMO_ANCHOR || m.data !== memoHex.toLowerCase()) throw refuse(409, "anchor: memo is not the endorsed anchor memo");
    return;
  }
  if (kind === "payout") {
    if (!endorse?.payout) throw refuse(409, "payout: the endorsed submission has no payout");
    if (tx.Destination !== endorse.payout.destination) throw refuse(409, "payout: destination differs from the endorsed payout");
    if (tx.Amount !== String(endorse.payout.amount)) throw refuse(409, "payout: amount differs from the endorsed payout");
    if (tx.Memos && tx.Memos.length) throw refuse(409, "payout: unexpected memos");
    return;
  }
  throw refuse(409, `unknown tx kind ${kind}`);
}

/** One JSON-RPC call to a rippled/clio server (the sidecar's own view of the ledger). */
export async function xrplRpc(url, command, params = {}) {
  const r = await fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ method: command, params: [params] }) });
  const j = await r.json();
  if (!j.result || j.result.status === "error") throw refuse(502, `xrpl ${command}: ${j.result?.error_message ?? j.result?.error ?? "no result"}`);
  return j.result;
}

// ---- key files -------------------------------------------------------------------------------
export function writePrivate(path, obj) {
  writeFileSync(path, JSON.stringify(obj, null, 2), { mode: 0o600 });
  chmodSync(path, 0o600);
}
export function loadWallet(path) {
  const { seed } = JSON.parse(readFileSync(path, "utf8"));
  return Wallet.fromSeed(seed);
}

// ---- pool setup ------------------------------------------------------------------------------
/** Faucet-funds the pool and the anchor sink, generates N signer keys, installs the SignerList and
 *  disables the pool's master key. Writes keys 0600 under `dir`. Returns the config object. */
export async function setupPool(client, { signers, quorum, dir }) {
  mkdirSync(dir, { recursive: true });
  const { wallet: pool } = await client.fundWallet();
  const { wallet: sink } = await client.fundWallet();
  const signerWallets = Array.from({ length: signers }, () => Wallet.generate());
  const single = async (from, tx) => {
    const prepared = await client.autofill({ Account: from.address, ...tx });
    const res = await client.submitAndWait(from.sign(prepared).tx_blob);
    if (res.result.meta?.TransactionResult !== "tesSUCCESS") throw new Error(`${tx.TransactionType} failed: ${res.result.meta?.TransactionResult}`);
    return res.result.hash;
  };
  const sls = await single(pool, {
    TransactionType: "SignerListSet", SignerQuorum: quorum,
    SignerEntries: signerWallets.map((w) => ({ SignerEntry: { Account: w.address, SignerWeight: 1 } })),
  });
  const dm = await single(pool, { TransactionType: "AccountSet", SetFlag: 4 });
  const config = {
    pool: pool.address, sink: sink.address, quorum,
    signers: signerWallets.map((w, i) => {
      const keyFile = `${dir}/signer${i + 1}.key.json`;
      writePrivate(keyFile, { address: w.address, seed: w.seed });
      return { address: w.address, keyFile };
    }),
    setupTx: { signerListSet: sls, disableMaster: dm },
  };
  writeFileSync(`${dir}/config.json`, JSON.stringify(config, null, 2));
  return config;
}

// ---- launching a quorum locally -------------------------------------------------------------------
/** Spawns N verifier nodes (Rust), N signer sidecars and one coordinator (with its own node).
 *  Returns URLs and a stop() that kills everything. */
export async function launchQuorum({ config, dir, nodeBin, xrplRpc, xrplWs, basePort = 7000, quiet = true, screeningList = null, screeningStandbyLedgers = 0 }) {
  const procs = [];
  const run = (cmd, args, env) => {
    const p = spawn(cmd, args, { env: { ...process.env, ...env }, stdio: quiet ? ["ignore", "ignore", "pipe"] : "inherit" });
    if (quiet) p.stderr.on("data", (d) => { const s = d.toString(); if (/error|panicked/i.test(s)) process.stderr.write(`[${cmd.split("/").pop()} ${args[1] ?? ""}] ${s}`); });
    procs.push(p);
    return p;
  };
  const n = config.signers.length;
  const nodeUrl = (i) => `http://127.0.0.1:${basePort + i}`;          // i = 0 coordinator's node, 1..n signers' nodes
  const signerUrl = (i) => `http://127.0.0.1:${basePort + 100 + i}`; // 1..n
  const coordinatorUrl = `http://127.0.0.1:${basePort + 200}`;
  for (let i = 0; i <= n; i++) {
    run(nodeBin, ["--listen", `127.0.0.1:${basePort + i}`, "--state", `${dir}/node${i}.verifier-state.json`, "--xrpl-rpc", xrplRpc, "--pool", config.pool,
      "--signers", config.signers.map((s) => s.address).join(","), "--quorum", String(config.quorum),
      ...(screeningList ? ["--screening-list", screeningList] : []), ...(screeningStandbyLedgers > 0 ? ["--screening-standby-ledgers", String(screeningStandbyLedgers)] : [])]);
  }
  for (let i = 1; i <= n; i++) {
    run(process.execPath, [new URL("./signer.mjs", import.meta.url).pathname], {
      PORT: String(basePort + 100 + i), NODE_URL: nodeUrl(i), POOL: config.pool, SINK: config.sink, KEY_FILE: config.signers[i - 1].keyFile, XRPL_RPC: xrplRpc,
    });
  }
  run(process.execPath, [new URL("./coordinator.mjs", import.meta.url).pathname], {
    PORT: String(basePort + 200), NODE_URL: nodeUrl(0), NODES: Array.from({ length: n + 1 }, (_, i) => nodeUrl(i)).join(","),
    SIGNERS: Array.from({ length: n }, (_, i) => signerUrl(i + 1)).join(","), QUORUM: String(config.quorum),
    POOL: config.pool, SINK: config.sink, XRPL_WS: xrplWs,
  });
  const waitFor = async (url, tries = 100) => {
    for (let t = 0; t < tries; t++) {
      try { await http("GET", url); return; } catch { await new Promise((r) => setTimeout(r, 200)); }
    }
    throw new Error(`service did not come up: ${url}`);
  };
  for (let i = 0; i <= n; i++) await waitFor(`${nodeUrl(i)}/state`);
  for (let i = 1; i <= n; i++) await waitFor(`${signerUrl(i)}/address`);
  await waitFor(`${coordinatorUrl}/state`);
  return {
    nodeUrl, signerUrl, coordinatorUrl,
    stop: () => { for (const p of procs) { try { p.kill("SIGTERM"); } catch {} } },
  };
}

export { Client, Wallet, existsSync };
