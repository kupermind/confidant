// The demo issuer service: writes the pool's entry credential (XLS-70) to any XRPL account that asks.
//
//   node issuer.mjs    ISSUER_KEY=<issuer.key.json> COORDINATOR_URL=http://127.0.0.1:7900 [ISSUER_PORT=7960]
//                      [ISSUER_DAYS=30] [ISSUER_PER_HOUR=60] [ISSUER_PER_SUBJECT_MINUTES=10] [XRPL_WS=…]
//
// What it is: the issuer whose address the nodes trust (/config.credential.issuer), as a service, so that anyone —
// a judge on a phone — can enter the pool without the team pasting a key into their browser. Say what that means: this
// is KYC in demo mode. The credential gate, the issuer attestation, the fail-closed check by every node before it signs
// and the holder's on-ledger accept are the real, working system; the issuer's admission POLICY in the demo is
// allow-by-default — this service issues to any account that asks, rate-limited — the standard practice for a demo (a full
// KYC decision pipeline is outside the hackathon's scope), so a demo credential attests the request, not a vetting. A real
// deployment drops its own approve/deny logic into this same issuer with no change to the gate (audit ruling 2026-10-25,
// docs/audit/RESP-demo-issuer-2026-10-25.md).
//
// What it holds: the demo issuer's key only (ISSUER_KEY). Never a pool key, never a holder's key. It signs
// CredentialCreate (and, for an expired one, CredentialDelete) from the issuer account; the holder's own
// CredentialAccept is what binds the account, and that stays with the holder.
//
// Guards (unit- and mutation-tested in issuer-lib.mjs): classic address only; idempotent — a subject that already
// holds a live credential from this issuer gets its status back and nothing is written; an expired one is replaced;
// a sliding-window rate limit per hour and per subject; the whole decision — read, judge, limit, write — runs one
// request at a time (makeIssuer), so the cap holds under a burst and a concurrent duplicate is answered, not written;
// the key must be the issuer the nodes trust, or the service refuses to start; body ≤ 4 KiB. The internal cap bounds
// WRITES; put a per-IP request limit in front (nginx limit_req, see docs/NODE-OPERATOR.md) against read floods.
//
//   GET  /health  → { ok, issuer, typeText, days, perHour, issuedThisHour, policy }
//   POST /issue   { subject } → 200 { status: "issued" | "accepted", issuer, credentialType, typeText, expiration, tx | null }
//                 400 bad request · 404 the account does not exist on the ledger (fund it first) · 429 rate limit ·
//                 502 the ledger refused or is unreadable · 503 the gate is off
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { Client, Wallet } from "xrpl";
import { RateLimiter, makeIssuer, parseIssueRequest } from "./issuer-lib.mjs";

const env = process.env;
const PORT = Number(env.ISSUER_PORT ?? 7960);
const COORD = (env.COORDINATOR_URL ?? "http://127.0.0.1:7900").replace(/\/$/, "");
const DAYS = Number(env.ISSUER_DAYS ?? 30);
if (!env.ISSUER_KEY) { console.error("issuer: need ISSUER_KEY (the issuer's key file)"); process.exit(2); }
const key = JSON.parse(readFileSync(env.ISSUER_KEY, "utf8"));
const issuer = Wallet.fromSeed(key.seed);
const cfg = await (await fetch(`${COORD}/config`)).json();
if (!cfg.credential?.issuer) { console.error("issuer: the pool has no credential gate (/config.credential is null) — nothing to issue"); process.exit(2); }
if (cfg.credential.issuer !== issuer.address) { console.error(`issuer: ISSUER_KEY is ${issuer.address}, the nodes trust ${cfg.credential.issuer}`); process.exit(2); }
const TYPE = String(cfg.credential.type).toUpperCase(), TYPE_TEXT = cfg.credential.typeText ?? "credential";
const limiter = new RateLimiter({ perHour: Number(env.ISSUER_PER_HOUR ?? 60), perSubjectMinutes: Number(env.ISSUER_PER_SUBJECT_MINUTES ?? 10) });
const POLICY = "KYC in demo mode: the gate the nodes read on the ledger is the real system; this issuer's admission policy is allow-by-default — the entry credential is issued to any account that asks, rate-limited — so a demo credential attests the request, not a vetting";

let client = null;
async function xrpl_() {
  if (client?.isConnected()) return client;
  if (client) { await client.disconnect().catch(() => {}); client = null; } // never leak a stale connection
  const c = new Client(env.XRPL_WS ?? cfg.xrplWs ?? "wss://s.altnet.rippletest.net:51233");
  c.on("error", (e) => console.error("issuer: xrpl client:", e?.message ?? e));
  await c.connect(); client = c; return c;
}
// One page of account_objects is read; a subject holding more than a page of credentials is out of scope for a demo
// issuer, and the tecDUPLICATE belt in makeIssuer answers the case idempotently anyway.
const readCredentials = async (subject) =>
  (await (await xrpl_()).request({ command: "account_objects", account: subject, type: "credential", ledger_index: "validated" })).result.account_objects;
async function submit(tx) {
  const c = await xrpl_();
  const signed = issuer.sign(await c.autofill({ ...tx, Account: issuer.address }));
  const res = await c.submitAndWait(signed.tx_blob);
  const code = res.result.meta?.TransactionResult;
  if (code !== "tesSUCCESS") throw Object.assign(new Error(`the ledger refused ${tx.TransactionType}: ${code}`), { status: 502, code });
  return signed.hash;
}
const issue = makeIssuer({
  issuer: issuer.address, typeHex: TYPE, typeText: TYPE_TEXT, days: DAYS, readCredentials, submit, limiter,
  log: (subject, _expiration, tx) => console.log(`issuer: ${TYPE_TEXT} → ${subject} (${DAYS} d) ${tx}`),
});

const send = (res, status, body) => { res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" }); res.end(JSON.stringify(body)); };
createServer(async (req, res) => {
  try {
    if (req.method === "GET" && req.url === "/health") return send(res, 200, { ok: true, issuer: issuer.address, typeText: TYPE_TEXT, days: DAYS, perHour: limiter.perHour, issuedThisHour: limiter.issuedThisHour, policy: POLICY });
    if (req.method !== "POST" || req.url !== "/issue") return send(res, 404, { error: "no route" });
    const text = await new Promise((ok, ko) => { let d = ""; req.on("data", (ch) => { d += ch; if (d.length > 4096) ko(Object.assign(new Error("body too large"), { status: 413 })); }); req.on("end", () => ok(d)); req.on("error", ko); });
    const { subject } = parseIssueRequest(text);
    send(res, 200, await issue(subject));
  } catch (e) { send(res, e.status ?? 500, { error: e.message }); }
}).listen(PORT, "127.0.0.1", () => console.log(`issuer: ${TYPE_TEXT} from ${issuer.address} on :${PORT} (${DAYS} d, ≤ ${limiter.perHour}/h)`));
// A broken process is not kept half-alive: log, exit, and let the unit's Restart=always bring a fresh one.
process.on("uncaughtException", (e) => { console.error("issuer: uncaught, exiting for the unit to restart:", e?.message ?? e); process.exit(1); });
