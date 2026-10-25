// Pure parts of the demo issuer service (issuer.mjs), kept separate so they are unit-tested and mutation-tested:
// the request guards, the idempotence judgement over the ledger's credential objects, and the rate limiter.
export const LSF_ACCEPTED = 0x00010000;
export const RIPPLE_EPOCH = 946684800;
export const rippleNow = () => Math.floor(Date.now() / 1000) - RIPPLE_EPOCH;

/** A classic XRPL address (X-addresses are refused: the credential's Subject is the classic form). */
export const isClassicAddress = (a) => typeof a === "string" && /^r[1-9A-HJ-NP-Za-km-z]{24,34}$/.test(a);

/**
 * What to do for a subject given its credential objects on the validated ledger:
 *   { action: "none", status: "accepted" | "issued" } — a live credential from this issuer exists: answer it, write nothing;
 *   { action: "replace" }                            — an expired one exists: delete it (anyone may, after expiry), then write;
 *   { action: "create" }                             — nothing from this issuer: write.
 */
export function judgeExisting(objects, issuer, typeHex, now = rippleNow()) {
  const have = (objects ?? []).find((o) => o.Issuer === issuer && String(o.CredentialType).toUpperCase() === String(typeHex).toUpperCase());
  if (!have) return { action: "create" };
  if (have.Expiration && have.Expiration <= now) return { action: "replace" };
  return { action: "none", status: (have.Flags & LSF_ACCEPTED) !== 0 ? "accepted" : "issued", expiration: have.Expiration ?? null };
}

/** A sliding-window limiter: at most `perHour` writes an hour overall, one per subject per `perSubjectMinutes`. */
export class RateLimiter {
  constructor({ perHour = 60, perSubjectMinutes = 10, now = () => Date.now() } = {}) {
    this.perHour = perHour; this.perSubjectMs = perSubjectMinutes * 60_000; this.now = now;
    this.recent = []; this.bySubject = new Map();
  }
  /** "" when a write for `subject` may proceed now, otherwise the refusal (HTTP 429 text). */
  check(subject) {
    const t = this.now();
    this.recent = this.recent.filter((x) => t - x < 3_600_000);
    if (this.recent.length >= this.perHour) return `the issuer wrote ${this.perHour} credentials in the last hour; try again later`;
    const last = this.bySubject.get(subject);
    if (last !== undefined && t - last < this.perSubjectMs) return `a credential was written for ${subject} ${Math.ceil((t - last) / 60_000)} min ago; one per ${this.perSubjectMs / 60_000} minutes per account`;
    return "";
  }
  /** Records a write that happened. */
  note(subject) { const t = this.now(); this.recent.push(t); this.bySubject.set(subject, t); }
  get issuedThisHour() { const t = this.now(); return this.recent.filter((x) => t - x < 3_600_000).length; }
}

/** Parses and bounds the request body: { subject } only. Throws with `.status` 400 on anything else. */
export function parseIssueRequest(text) {
  if (typeof text !== "string" || text.length > 4096) throw Object.assign(new Error("body too large"), { status: 413 });
  let body; try { body = JSON.parse(text || "{}"); } catch { throw Object.assign(new Error("body must be JSON"), { status: 400 }); }
  if (!body || typeof body !== "object" || !isClassicAddress(body.subject)) throw Object.assign(new Error("send { subject: <classic XRPL address> }"), { status: 400 });
  return { subject: body.subject };
}

/** The rippled error CODE of an xrpl.js RippledError. xrpl.js puts the prose (`error_message`, "Account not found.") in
 *  `message` and the code (`actNotFound`) in `data.error`: match the code, never the prose — the same lesson as the node's
 *  `entryNotFound` hotfix (#62). */
export const rippledCode = (e) => (e && e.data && typeof e.data.error === "string" ? e.data.error : "");
export const withStatus = (message, status) => Object.assign(new Error(message), { status });

/**
 * The issuer's core with its I/O injected, so the whole decision is unit-tested:
 *   readCredentials(subject) → the subject's credential objects on the validated ledger (throws a RippledError on failure);
 *   submit(tx)               → the validated transaction's hash, or throws with `.code` = the ledger's tec code;
 *   limiter                  → a RateLimiter; now() → ripple seconds.
 * EVERYTHING runs inside one queue — read, judge, limit, write, note — one request at a time (the issuer account has one
 * sequence number anyway). So a second request for the same subject reads AFTER the first one's write validated and gets
 * the idempotent answer instead of a tecDUPLICATE, two "replace" requests do not churn Delete/Create, and the hourly cap
 * holds under a burst (every request passes `check` only after the previous one's `note`). Audit ruling F1, 2026-10-25.
 * Belt for writes that happen out of band (another operator, a later account_objects page): a tecDUPLICATE on the create
 * is answered idempotently after a re-read, and a tecNO_ENTRY on the replace's delete means there is nothing to replace.
 * Both count under the cap — a refused transaction still costs the issuer its fee.
 */
export function makeIssuer({ issuer, typeHex, typeText, days, readCredentials, submit, limiter, now = rippleNow, log = () => {} }) {
  let queue = Promise.resolve();
  const serialized = (fn) => { const p = queue.then(fn, fn); queue = p.catch(() => {}); return p; };
  const answer = (status, expiration, tx) => ({ status, issuer, credentialType: typeHex, typeText, expiration, tx });
  const read = async (subject) => {
    try { return (await readCredentials(subject)) ?? []; }
    catch (e) {
      if (rippledCode(e) === "actNotFound") throw withStatus("this account does not exist on the ledger yet — fund it first", 404);
      throw withStatus(`ledger unreadable: ${e.message}`, 502);
    }
  };
  return (subject) => serialized(async () => {
    const j = judgeExisting(await read(subject), issuer, typeHex, now());
    if (j.action === "none") return answer(j.status, j.expiration, null);
    const why = limiter.check(subject); if (why) throw withStatus(why, 429);
    if (j.action === "replace") {
      try { await submit({ TransactionType: "CredentialDelete", Subject: subject, CredentialType: typeHex }); }
      catch (e) { if (e.code !== "tecNO_ENTRY") throw e; } // deleted out of band: nothing to replace, write the new one
    }
    const expiration = now() + days * 86400;
    let tx;
    try { tx = await submit({ TransactionType: "CredentialCreate", Subject: subject, CredentialType: typeHex, Expiration: expiration }); }
    catch (e) {
      if (e.code !== "tecDUPLICATE") throw e;
      limiter.note(subject); // the refused attempt cost a fee: it counts
      const again = judgeExisting(await read(subject).catch(() => []), issuer, typeHex, now());
      return answer(again.action === "none" ? again.status : "issued", again.expiration ?? null, null);
    }
    limiter.note(subject);
    log(subject, expiration, tx);
    return answer("issued", expiration, tx);
  });
}
