// Signer sidecar: holds ONE signer's XRPL key and signs only what its own verifier node endorsed.
//
//   POST /sign { txs: [{ kind: "anchor" | "payout", tx }], submission, memoHex }
//     1. ask MY node POST /endorse { submission, proposedMemoHex } (the node validates on its own
//        state and re-reads the deposit from the ledger itself; idempotent for retries)
//     2. check every tx's shape against what that endorsement authorizes (lib.checkTxShape)
//     3. only then sign each tx with the multisign flag and return the blobs
//   GET  /address
//
// Env: PORT, NODE_URL, POOL, SINK, KEY_FILE (JSON {address, seed}, 0600), MAX_FEE_DROPS (default 5000). The sidecar keeps no
// verifier state; the node does. Nothing from the coordinator reaches a signature unverified.

import { serve, http, refuse, checkTxShape, loadWallet } from "./lib.mjs";

const { PORT, NODE_URL, POOL, SINK, KEY_FILE } = process.env;
const MAX_FEE_DROPS = Number(process.env.MAX_FEE_DROPS ?? 5000);
if (!PORT || !NODE_URL || !POOL || !SINK || !KEY_FILE) {
  console.error("signer: need PORT, NODE_URL, POOL, SINK, KEY_FILE");
  process.exit(2);
}
const wallet = loadWallet(KEY_FILE);
console.error(`signer ${wallet.address}: node ${NODE_URL}, pool ${POOL}, sink ${SINK}, port ${PORT} — WARNING: key file ${KEY_FILE} holds the signing seed unencrypted (test deployment)`);

await serve(Number(PORT), {
  "GET /address": async () => ({ address: wallet.address, node: NODE_URL }),
  "POST /sign": async (body) => {
    const { txs, submission, memoHex } = body ?? {};
    if (!Array.isArray(txs) || !txs.length || !submission || !memoHex) throw refuse(400, "need txs[], submission, memoHex");
    // 1. my own node decides; its refusal is my refusal.
    let endorse;
    try {
      endorse = await http("POST", `${NODE_URL}/endorse`, { submission, proposedMemoHex: memoHex });
    } catch (e) {
      throw refuse(409, `my node refused to endorse: ${e.message}`, e.details);
    }
    // 2. the transactions must be exactly what that endorsement authorizes.
    for (const { kind, tx } of txs) checkTxShape(kind, tx, { pool: POOL, sink: SINK, memoHex, endorse, maxFeeDrops: MAX_FEE_DROPS });
    // 3. sign.
    const blobs = txs.map(({ kind, tx }) => ({ kind, tx_blob: wallet.sign(tx, true).tx_blob }));
    return { signer: wallet.address, endorsedRoot: endorse.newRoot, blobs };
  },
});
