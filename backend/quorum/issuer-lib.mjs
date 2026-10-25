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
