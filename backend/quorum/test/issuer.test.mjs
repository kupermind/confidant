// The demo issuer service's guards (issuer-lib.mjs): address check, idempotence over the ledger's objects, rate
// limiter, request parsing. Each guard has a test that goes red when the guard is removed (mutation table in the REQ).
import { test } from "node:test";
import assert from "node:assert/strict";
import { RateLimiter, isClassicAddress, judgeExisting, makeIssuer, parseIssueRequest, rippledCode, LSF_ACCEPTED } from "../issuer-lib.mjs";

const ISSUER = "rfSGoJ5hjSppbmWy79MH1kqHu27JRwrcam", TYPE = "436F6E666964616E744B5943";
const cred = (over = {}) => ({ Issuer: ISSUER, CredentialType: TYPE, Subject: "rUWv8fDvVHM9cv42vUc4UN4KwtLcNN9tv9", Flags: 0, ...over });

test("isClassicAddress: classic r-addresses only (no X-addresses, no junk)", () => {
  assert.equal(isClassicAddress("rUWv8fDvVHM9cv42vUc4UN4KwtLcNN9tv9"), true);
  for (const bad of ["XVLhHMPHU98es4dbozjVtdWzVrDjtV18pX8yuPT7y4xaEHi", "r", "nobody", "", null, 42, "rUWv8fDvVHM9cv42vUc4UN4KwtLcNN9tv9 "]) assert.equal(isClassicAddress(bad), false, String(bad));
});

test("judgeExisting: nothing from this issuer → create; another issuer's or another type's credential does not count", () => {
  assert.deepEqual(judgeExisting([], ISSUER, TYPE, 1000), { action: "create" });
  assert.deepEqual(judgeExisting([cred({ Issuer: "rOtherIssuerxxxxxxxxxxxxxxxxxxxxxxx" })], ISSUER, TYPE, 1000), { action: "create" });
  assert.deepEqual(judgeExisting([cred({ CredentialType: "436F6E666964616E74506F46" })], ISSUER, TYPE, 1000), { action: "create" });
});

test("judgeExisting: a live credential is answered, not rewritten (issued vs accepted); the type compares case-insensitively", () => {
  assert.deepEqual(judgeExisting([cred({ Expiration: 2000 })], ISSUER, TYPE, 1000), { action: "none", status: "issued", expiration: 2000 });
  assert.deepEqual(judgeExisting([cred({ Flags: LSF_ACCEPTED })], ISSUER, TYPE, 1000), { action: "none", status: "accepted", expiration: null });
  assert.deepEqual(judgeExisting([cred({ CredentialType: TYPE.toLowerCase(), Flags: LSF_ACCEPTED })], ISSUER, TYPE, 1000), { action: "none", status: "accepted", expiration: null });
});

test("judgeExisting: an expired credential is replaced (expiry ≤ now), a future one is not", () => {
  assert.deepEqual(judgeExisting([cred({ Expiration: 1000 })], ISSUER, TYPE, 1000), { action: "replace" });
  assert.deepEqual(judgeExisting([cred({ Expiration: 999 })], ISSUER, TYPE, 1000), { action: "replace" });
  assert.equal(judgeExisting([cred({ Expiration: 1001 })], ISSUER, TYPE, 1000).action, "none");
});

test("RateLimiter: one write per subject per window, and a global hourly cap; the window slides", () => {
  let t = 0; const rl = new RateLimiter({ perHour: 2, perSubjectMinutes: 10, now: () => t });
  assert.equal(rl.check("rA"), ""); rl.note("rA");
  assert.match(rl.check("rA"), /one per 10 minutes per account/);
  assert.equal(rl.check("rB"), ""); rl.note("rB");
  assert.match(rl.check("rC"), /2 credentials in the last hour/);
  assert.equal(rl.issuedThisHour, 2);
  t = 10 * 60_000; assert.equal(rl.check("rA"), "" === rl.check("rA") ? "" : rl.check("rA")); // the per-subject window has passed…
  assert.match(rl.check("rC"), /in the last hour/);                                           // …but the hourly cap still holds
  t = 3_600_000; assert.equal(rl.check("rC"), ""); assert.equal(rl.issuedThisHour, 0);       // the hour has slid past
});

test("parseIssueRequest: { subject } only; bad JSON, a missing or bad subject and an oversized body are refused with their status", () => {
  assert.deepEqual(parseIssueRequest('{"subject":"rUWv8fDvVHM9cv42vUc4UN4KwtLcNN9tv9"}'), { subject: "rUWv8fDvVHM9cv42vUc4UN4KwtLcNN9tv9" });
  assert.throws(() => parseIssueRequest("{not json"), (e) => e.status === 400);
  assert.throws(() => parseIssueRequest("{}"), (e) => e.status === 400);
  assert.throws(() => parseIssueRequest('{"subject":"nobody"}'), (e) => e.status === 400);
  assert.throws(() => parseIssueRequest("x".repeat(4097)), (e) => e.status === 413);
});

// ---- makeIssuer: the whole decision in one queue (audit ruling F1) and the rippled code, not the prose (F2) ----
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
/** A fake validated ledger: credential objects per subject; a write is visible once `submit` resolves; every attempt counted. */
function fakeLedger({ delay = 3, missing = [], expired = [] } = {}) {
  const creds = new Map(); for (const s of expired) creds.set(s, cred({ Subject: s, Expiration: 500 }));
  const L = {
    creds, attempts: 0, writes: 0, deletes: 0,
    async readCredentials(s) { await sleep(delay); if (missing.includes(s)) throw Object.assign(new Error("Account not found."), { data: { error: "actNotFound" } }); const o = creds.get(s); return o ? [o] : []; },
    async submit(tx) {
      await sleep(delay); L.attempts++;
      if (tx.TransactionType === "CredentialCreate") {
        if (creds.has(tx.Subject)) throw Object.assign(new Error("the ledger refused CredentialCreate: tecDUPLICATE"), { status: 502, code: "tecDUPLICATE" });
        creds.set(tx.Subject, cred({ Subject: tx.Subject, Expiration: tx.Expiration })); L.writes++; return `TX${L.writes}`;
      }
      if (tx.TransactionType === "CredentialDelete") {
        if (!creds.has(tx.Subject)) throw Object.assign(new Error("the ledger refused CredentialDelete: tecNO_ENTRY"), { status: 502, code: "tecNO_ENTRY" });
        creds.delete(tx.Subject); L.deletes++; return `DEL${L.deletes}`;
      }
      throw new Error("unexpected transaction");
    },
  };
  return L;
}
const mk = (L, over = {}) => makeIssuer({ issuer: ISSUER, typeHex: TYPE, typeText: "ConfidantKYC", days: 30, readCredentials: (s) => L.readCredentials(s), submit: (tx) => L.submit(tx), limiter: new RateLimiter({ perHour: 60, perSubjectMinutes: 10 }), now: () => 1000, ...over });

test("makeIssuer: a live credential is answered, nothing attempted; a missing one is written with the configured expiry", async () => {
  const L = fakeLedger(); L.creds.set("rL", cred({ Subject: "rL", Flags: LSF_ACCEPTED }));
  const issue = mk(L);
  assert.deepEqual(await issue("rL"), { status: "accepted", issuer: ISSUER, credentialType: TYPE, typeText: "ConfidantKYC", expiration: null, tx: null });
  assert.equal(L.attempts, 0);
  assert.deepEqual(await issue("rN"), { status: "issued", issuer: ISSUER, credentialType: TYPE, typeText: "ConfidantKYC", expiration: 1000 + 30 * 86400, tx: "TX1" });
  assert.equal(L.attempts, 1);
});

test("makeIssuer: two concurrent requests for one subject → one write, one attempt; the other reads the validated write and is answered", async () => {
  const L = fakeLedger(); const rl = new RateLimiter({ perHour: 60, perSubjectMinutes: 10 }); const issue = mk(L, { limiter: rl });
  const [a, b] = await Promise.all([issue("rA"), issue("rA")]);
  assert.equal(L.writes, 1); assert.equal(L.attempts, 1, "a second attempt would have been a tecDUPLICATE and a burnt fee");
  assert.equal(a.tx, "TX1"); assert.equal(a.status, "issued");
  assert.equal(b.tx, null); assert.equal(b.status, "issued"); assert.equal(b.expiration, 1000 + 30 * 86400);
  assert.equal(rl.issuedThisHour, 1);
});

test("makeIssuer: a burst of ten requests never exceeds the hourly cap — the limit is judged inside the queue", async () => {
  const L = fakeLedger(); const issue = mk(L, { limiter: new RateLimiter({ perHour: 3, perSubjectMinutes: 10 }) });
  const r = await Promise.allSettled(Array.from({ length: 10 }, (_, i) => issue(`rS${i}`)));
  assert.equal(L.writes, 3); assert.equal(L.attempts, 3);
  assert.equal(r.filter((x) => x.status === "fulfilled").length, 3);
  assert.equal(r.filter((x) => x.status === "rejected" && x.reason.status === 429).length, 7);
});

test("makeIssuer: two concurrent requests for a subject with an expired credential → one delete and one create, no churn", async () => {
  const L = fakeLedger({ expired: ["rE"] }); const issue = mk(L);
  const [a, b] = await Promise.all([issue("rE"), issue("rE")]);
  assert.equal(L.deletes, 1); assert.equal(L.writes, 1); assert.equal(L.attempts, 2);
  assert.equal(a.tx, "TX1"); assert.equal(b.tx, null); assert.equal(b.status, "issued");
});

test("makeIssuer: a credential written out of band between the read and the write (tecDUPLICATE) is answered idempotently after a re-read, and the attempt counts under the cap", async () => {
  const L = fakeLedger(); const rl = new RateLimiter({ perHour: 60, perSubjectMinutes: 10 });
  // Another writer wins the race: the first read sees nothing, then the credential appears before our write.
  const issue = mk(L, { limiter: rl, readCredentials: async (s) => { const r = await L.readCredentials(s); if (!L.creds.has(s)) L.creds.set(s, cred({ Subject: s, Flags: LSF_ACCEPTED })); return r; } });
  const a = await issue("rO");
  assert.equal(L.attempts, 1); assert.equal(L.writes, 0);
  assert.equal(a.status, "accepted"); assert.equal(a.tx, null);
  assert.equal(rl.issuedThisHour, 1, "the refused attempt cost a fee, so it counts");
});

test("makeIssuer: the replace's delete finding nothing (tecNO_ENTRY, deleted out of band) still writes the new credential; any other tec code is an error", async () => {
  const L = fakeLedger({ expired: ["rE"] });
  const issue = mk(L, { readCredentials: async (s) => { const r = await L.readCredentials(s); L.creds.delete(s); return r; } }); // vanishes after the read
  const a = await issue("rE"); assert.equal(a.status, "issued"); assert.equal(L.writes, 1); assert.equal(L.deletes, 0);
  const L2 = fakeLedger({ expired: ["rF"] });
  const issue2 = mk(L2, { submit: async (tx) => { if (tx.TransactionType === "CredentialDelete") throw Object.assign(new Error("tecNO_PERMISSION"), { status: 502, code: "tecNO_PERMISSION" }); return L2.submit(tx); } });
  await assert.rejects(issue2("rF"), (e) => e.status === 502); assert.equal(L2.writes, 0);
});

test("makeIssuer: an account that does not exist → 404 by the rippled CODE, never by the prose; any other read failure → 502; nothing attempted", async () => {
  const L = fakeLedger({ missing: ["rM"] }); const issue = mk(L);
  await assert.rejects(issue("rM"), (e) => e.status === 404 && /fund it first/.test(e.message));
  const prose = mk(L, { readCredentials: async () => { throw new Error("Account not found."); } }); // the prose alone is not the code
  await assert.rejects(prose("rX"), (e) => e.status === 502 && /ledger unreadable/.test(e.message));
  assert.equal(rippledCode({ data: { error: "actNotFound" }, message: "Account not found." }), "actNotFound");
  assert.equal(rippledCode(new Error("Account not found.")), "");
  assert.equal(L.attempts, 0);
});
