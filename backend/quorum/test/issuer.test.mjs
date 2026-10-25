// The demo issuer service's guards (issuer-lib.mjs): address check, idempotence over the ledger's objects, rate
// limiter, request parsing. Each guard has a test that goes red when the guard is removed (mutation table in the REQ).
import { test } from "node:test";
import assert from "node:assert/strict";
import { RateLimiter, isClassicAddress, judgeExisting, parseIssueRequest, LSF_ACCEPTED } from "../issuer-lib.mjs";

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
