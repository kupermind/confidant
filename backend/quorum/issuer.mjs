// The demo issuer service: writes the pool's entry credential (XLS-70) to any XRPL account that asks.
//
//   node issuer.mjs    ISSUER_KEY=<issuer.key.json> COORDINATOR_URL=http://127.0.0.1:7900 [ISSUER_PORT=7960]
//                      [ISSUER_DAYS=30] [ISSUER_PER_HOUR=60] [ISSUER_PER_SUBJECT_MINUTES=10] [XRPL_WS=…]
//
// What it is: the issuer whose address the nodes trust (/config.credential.issuer), as a service, so that anyone —
// a judge on a phone — can enter the pool without the team pasting a key into their browser. The gate's mechanism is
// real: every verifier node reads the credential from the ledger before it signs. The issuer's *policy* is a stub —
// "whoever asks, rate-limited" — and every surface says so. A real deployment has a third party with its own policy.
//
// What it holds: the demo issuer's key only (ISSUER_KEY). Never a pool key, never a holder's key. It signs
// CredentialCreate (and, for an expired one, CredentialDelete) from the issuer account; the holder's own
// CredentialAccept is what binds the account, and that stays with the holder.
//
// Guards (unit- and mutation-tested in issuer-lib.mjs): classic address only; idempotent — a subject that already
// holds a live credential from this issuer gets its status back and nothing is written; an expired one is replaced;
// a sliding-window rate limit per hour and per subject; one signing at a time (the issuer account's sequence);
// the key must be the issuer the nodes trust, or the service refuses to start; body ≤ 4 KiB.
//
//   GET  /health  → { ok, issuer, typeText, perHour, issuedThisHour }
//   POST /issue   { subject } → 200 { status: "issued" | "accepted", issuer, credentialType, typeText, expiration, tx | null }
//                 400 bad request · 404 the account does not exist on the ledger (fund it first) · 429 rate limit ·
//                 502 the ledger refused or is unreadable · 503 the gate is off
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { Client, Wallet } from "xrpl";
import { RateLimiter, judgeExisting, parseIssueRequest, rippleNow } from "./issuer-lib.mjs";

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

let client = null;
async function xrpl_() { if (client?.isConnected()) return client; client = new Client(env.XRPL_WS ?? cfg.xrplWs ?? "wss://s.altnet.rippletest.net:51233"); await client.connect(); return client; }
async function submit(tx) {
  const c = await xrpl_();
  const signed = issuer.sign(await c.autofill({ ...tx, Account: issuer.address }));
  const res = await c.submitAndWait(signed.tx_blob);
  const code = res.result.meta?.TransactionResult;
  if (code !== "tesSUCCESS") throw Object.assign(new Error(`the ledger refused ${tx.TransactionType}: ${code}`), { status: 502 });
  return signed.hash;
}
let queue = Promise.resolve(); // one signing at a time: the issuer account has one sequence number
const serialized = (fn) => { const p = queue.then(fn, fn); queue = p.catch(() => {}); return p; };

async function issue(subject) {
  const c = await xrpl_();
  let objects;
  try { objects = (await c.request({ command: "account_objects", account: subject, type: "credential", ledger_index: "validated" })).result.account_objects; }
  catch (e) { if (/actNotFound/.test(e.message)) throw Object.assign(new Error("this account does not exist on the ledger yet — fund it first"), { status: 404 }); throw Object.assign(new Error(`ledger unreadable: ${e.message}`), { status: 502 }); }
  const j = judgeExisting(objects, issuer.address, TYPE);
  if (j.action === "none") return { status: j.status, issuer: issuer.address, credentialType: TYPE, typeText: TYPE_TEXT, expiration: j.expiration, tx: null };
  const why = limiter.check(subject); if (why) throw Object.assign(new Error(why), { status: 429 });
  return serialized(async () => {
    if (j.action === "replace") await submit({ TransactionType: "CredentialDelete", Subject: subject, CredentialType: TYPE });
    const expiration = rippleNow() + DAYS * 86400;
    const tx = await submit({ TransactionType: "CredentialCreate", Subject: subject, CredentialType: TYPE, Expiration: expiration });
    limiter.note(subject);
    console.log(`issuer: ${TYPE_TEXT} → ${subject} (${DAYS} d) ${tx}`);
    return { status: "issued", issuer: issuer.address, credentialType: TYPE, typeText: TYPE_TEXT, expiration, tx };
  });
}

const send = (res, status, body) => { res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" }); res.end(JSON.stringify(body)); };
createServer(async (req, res) => {
  try {
    if (req.method === "GET" && req.url === "/health") return send(res, 200, { ok: true, issuer: issuer.address, typeText: TYPE_TEXT, days: DAYS, perHour: limiter.perHour, issuedThisHour: limiter.issuedThisHour, policy: "demo issuer: writes the entry credential to any account that asks, rate-limited; the gate on the ledger is real" });
    if (req.method !== "POST" || req.url !== "/issue") return send(res, 404, { error: "no route" });
    const text = await new Promise((ok, ko) => { let d = ""; req.on("data", (ch) => { d += ch; if (d.length > 4096) ko(Object.assign(new Error("body too large"), { status: 413 })); }); req.on("end", () => ok(d)); req.on("error", ko); });
    const { subject } = parseIssueRequest(text);
    send(res, 200, await issue(subject));
  } catch (e) { send(res, e.status ?? 500, { error: e.message }); }
}).listen(PORT, "127.0.0.1", () => console.log(`issuer: ${TYPE_TEXT} from ${issuer.address} on :${PORT} (${DAYS} d, ≤ ${limiter.perHour}/h)`));
process.on("uncaughtException", (e) => console.error("issuer: uncaught:", e?.message ?? e));
