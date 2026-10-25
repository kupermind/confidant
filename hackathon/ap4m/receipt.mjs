// Shared by relay.mjs, agent.mjs and verify-receipt.mjs: the receipt statement's canonical form and the on-ledger
// facts it names. A receipt binds one paid claim (XRPL EVM) to one anchored bundle (XRPL L1) and to the agent's
// proof-of-funds credential; both parties sign the same canonical text, so anyone can re-check it from public data.
export const POF_TYPE_HEX = "436F6E666964616E74506F46"; // "ConfidantPoF"
export const MEMO_ANCHOR_HEX = Buffer.from("confidant/anchor", "utf8").toString("hex").toUpperCase();
export const LSF_ACCEPTED = 0x00010000;
export const RIPPLE_EPOCH = 946684800;
export const rippleNow = () => Math.floor(Date.now() / 1000) - RIPPLE_EPOCH;

/** JSON with keys sorted at every level: the one spelling both signatures are over. */
export function canonicalJson(v) {
  if (Array.isArray(v)) return `[${v.map(canonicalJson).join(",")}]`;
  if (v && typeof v === "object") return `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${canonicalJson(v[k])}`).join(",")}}`;
  return JSON.stringify(v);
}
/** What is signed: EIP-191 personal message for the EVM keys, these same bytes for the XRPL key. */
export const statementText = (s) => canonicalJson(s);
export const statementHex = (s) => Buffer.from(statementText(s), "utf8").toString("hex");

export const hexToText = (h) => Buffer.from(String(h ?? ""), "hex").toString("utf8");
/** The credential URI as every node writes it: confidant/pof/v1|X|L|E|<16-hex digest of the challenge>. */
export const parsePofUri = (text) => {
  const m = /^confidant\/pof\/v1\|(\d{1,19})\|(\d{1,12})\|(\d{1,12})\|([0-9a-f]{16})$/.exec(text ?? "");
  return m ? { amount: Number(m[1]), ledgerIndex: Number(m[2]), expiry: Number(m[3]), digest: m[4] } : null;
};
/** The bundle hash inside an anchor memo: version(1) ‖ op(1) ‖ root(32) ‖ bundleHash(32) ‖ … (core/src/memo.rs). */
export function bundleHashFromMemo(memoHex) {
  const b = Buffer.from(String(memoHex ?? ""), "hex");
  if (b.length < 66 || b[0] !== 1) return null;
  return b.subarray(34, 66).toString("hex");
}
/** The pool's proof-of-funds credential among an account's credential objects; null when absent. */
export function findPofCredential(objects, pool) {
  return (objects ?? []).find((o) => o.Issuer === pool && String(o.CredentialType).toUpperCase() === POF_TYPE_HEX) ?? null;
}
/** Judges a credential object: "" when it counts, otherwise why not (accepted, unexpired, URI understood, ≥ min). */
export function judgePofCredential(cred, minDrops) {
  if (!cred) return "no proof-of-funds credential from the pool on this account";
  if (!(cred.Flags & LSF_ACCEPTED)) return "the proof-of-funds credential is issued but not accepted by the account";
  if (cred.Expiration && cred.Expiration <= rippleNow()) return `the proof-of-funds credential expired at ${new Date((cred.Expiration + RIPPLE_EPOCH) * 1000).toISOString()}`;
  const u = parsePofUri(hexToText(cred.URI));
  if (!u) return "the credential's URI is not a proof-of-funds record";
  if (u.amount < minDrops) return `the credential says controlled ≥ ${u.amount / 1e6} XRP; this relay requires ≥ ${minDrops / 1e6} XRP`;
  return "";
}
