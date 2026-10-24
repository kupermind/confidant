// Signer sidecar: holds ONE signer's XRPL key and signs only what its own verifier node endorsed.
//
//   POST /sign { txs: [{ kind: "anchor" | "payout", tx }], submission, memoHex }
//   POST /sign { txs: [{ kind: "fee-sweep", tx }] } — a treasury sweep: destination and amount are
//        checked against MY node's GET /fees (treasury, available), no endorsement involved
//   POST /sign { txs: [{ kind: "credential-pof", tx }], pof: { record, amount, challenge } } — a
//        proof-of-funds Credential: MY node re-verifies the proof read-only NOW (POST /pof/verify:
//        own state, unspent re-checked at signing time) and the CredentialCreate must equal its
//        verdict's credential fields (subject, type, expiration, URI) exactly
//     1. ask MY node POST /endorse { submission, proposedMemoHex } (the node validates on its own
//        state and re-reads the deposit from the ledger itself; idempotent for retries)
//     2. read the pool account's next Sequence and the current ledger index MYSELF (XRPL_RPC), then
//        check every tx against what that endorsement authorizes (lib.checkTxShape): shape, Fee
//        ceiling, Sequence pinned (anchor = next, payout = next + 1), LastLedgerSequence bounded
//     3. only then sign each tx with the multisign flag and return the blobs
//   GET  /address
//
// Env: PORT, NODE_URL, POOL, SINK, KEY_FILE (JSON {address, seed}, 0600), XRPL_RPC (default testnet),
// MAX_FEE_DROPS (default 5000). The sidecar keeps no
// verifier state; the node does. Nothing from the coordinator reaches a signature unverified.

import { serve, http, refuse, checkTxShape, loadWallet, xrplRpc } from "./lib.mjs";

const { PORT, NODE_URL, POOL, SINK, KEY_FILE } = process.env;
const MAX_FEE_DROPS = Number(process.env.MAX_FEE_DROPS ?? 5000);
const XRPL_RPC = process.env.XRPL_RPC ?? "https://s.altnet.rippletest.net:51234";
if (!PORT || !NODE_URL || !POOL || !SINK || !KEY_FILE) {
  console.error("signer: need PORT, NODE_URL, POOL, SINK, KEY_FILE");
  process.exit(2);
}
const wallet = loadWallet(KEY_FILE);
console.error(`signer ${wallet.address}: node ${NODE_URL}, pool ${POOL}, sink ${SINK}, port ${PORT} — WARNING: key file ${KEY_FILE} holds the signing seed unencrypted (test deployment)`);

await serve(Number(PORT), {
  "GET /address": async () => ({ address: wallet.address, node: NODE_URL }),
  "POST /sign": async (body) => {
    const { txs, submission, memoHex, pof } = body ?? {};
    if (!Array.isArray(txs) || !txs.length) throw refuse(400, "need txs[]");
    const kinds = txs.map((t) => t.kind).join(",");
    if (!["anchor", "payout", "anchor,payout", "fee-sweep", "credential-pof"].includes(kinds)) throw refuse(400, `txs must be [anchor], [payout], [anchor, payout] in that order, [fee-sweep] alone or [credential-pof] alone; got [${kinds}]`);
    // 1. my own node decides. An anchor/payout needs its endorsement; a fee sweep needs its fee ledger.
    let endorse = null, fees = null, pofCredential = null;
    if (kinds === "fee-sweep") {
      fees = await http("GET", `${NODE_URL}/fees`);
    } else if (kinds === "credential-pof") {
      if (!pof?.record || !pof?.challenge || !Number.isInteger(pof?.amount)) throw refuse(400, "need pof { record, amount, challenge }");
      let verdict;
      try { verdict = await http("POST", `${NODE_URL}/pof/verify`, { record: pof.record, amount: pof.amount, challenge: pof.challenge }); }
      catch (e) { throw refuse(409, `my node did not verify the proof of funds: ${e.message}`, e.details); }
      if (!verdict.credential) throw refuse(409, `my node refused to authorize the credential: ${verdict.credentialRefused ?? "no credential fields"}`);
      pofCredential = verdict.credential;
    } else {
      if (!submission || !memoHex) throw refuse(400, "need submission and memoHex");
      try { endorse = await http("POST", `${NODE_URL}/endorse`, { submission, proposedMemoHex: memoHex }); }
      catch (e) { throw refuse(409, `my node refused to endorse: ${e.message}`, e.details); }
    }
    // 2. the transactions must be exactly what that authorizes — against MY view of the pool
    //    account's next Sequence and the current ledger, not the coordinator's.
    const [info, cur] = await Promise.all([xrplRpc(XRPL_RPC, "account_info", { account: POOL, ledger_index: "current" }), xrplRpc(XRPL_RPC, "ledger_current")]);
    const nextSequence = info.account_data.Sequence, ledgerIndex = cur.ledger_current_index;
    txs.forEach(({ kind, tx }, i) => checkTxShape(kind, tx, { pool: POOL, sink: SINK, memoHex, endorse, expectedSequence: nextSequence + i, ledgerIndex, maxFeeDrops: MAX_FEE_DROPS, treasury: fees?.treasury, feesAvailable: fees?.available, pofCredential }));
    // 3. sign.
    const blobs = txs.map(({ kind, tx }) => ({ kind, tx_blob: wallet.sign(tx, true).tx_blob }));
    return { signer: wallet.address, endorsedRoot: endorse?.newRoot ?? null, blobs };
  },
});
