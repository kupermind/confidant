// Coordinator: the public entry point of the quorum. Holds NO XRPL key (the pool's master key is
// disabled; only the signers' sidecars sign). It can delay, never steal, mint or forge.
//
//   GET  /state, GET /log?from=N, POST /audit/reveal          → proxied to its own node
//   POST /submit/shield   { record, depositTxHash }
//   POST /submit/transfer { record }
//   POST /submit/unshield { record, destination }
//     1. own node accepts (for a shield the node reads the deposit from the ledger itself) → memoHex
//     2. prepare the anchor Payment (and the payout Payment for an unshield) with xrpl.js autofill
//     3. fan {txs, submission, memoHex} out to every signer sidecar; keep the ones that signed
//     4. with ≥ QUORUM signatures: multisign + submit the anchor, then the payout
//     5. tell every node POST /anchored { root, anchorTxHash } (each verifies the tx on the ledger)
//   The signed payout blob is returned to the client too: if the payout fails after the anchor
//   landed (note spent, user unpaid), anyone may resubmit it until its LastLedgerSequence.
//
// Env: PORT, NODE_URL (own node), NODES (all nodes, comma-separated), SIGNERS (sidecar URLs),
//      QUORUM, POOL, SINK, XRPL_WS.

import { Client, multisign } from "xrpl";
import { DEFAULT_PAYOUT_LLS_WINDOW, DEFAULT_ANCHOR_LLS_WINDOW } from "./lib.mjs";
const ANCHOR_LLS_OFFSET = Math.floor(DEFAULT_ANCHOR_LLS_WINDOW / 3); // 20 ledgers ≈ 80 s
import { serve, http, refuse, memoOf, MEMO_ANCHOR, resilientClient } from "./lib.mjs";

const env = process.env;
for (const k of ["PORT", "NODE_URL", "NODES", "SIGNERS", "QUORUM", "POOL", "SINK"]) {
  if (!env[k]) { console.error(`coordinator: need ${k}`); process.exit(2); }
}
const XRPL_WS = env.XRPL_WS ?? "wss://s.altnet.rippletest.net:51233";
const SIGNERS = env.SIGNERS.split(",");
const NODES = env.NODES.split(",");
const QUORUM = Number(env.QUORUM);
const client = resilientClient(new Client(XRPL_WS));
await client.connect();
console.error(`coordinator: node ${env.NODE_URL}, ${SIGNERS.length} signers, quorum ${QUORUM}, pool ${env.POOL}, port ${env.PORT} (holds no keys)`);

async function submitMultisigned(blobs, label) {
  const res = await client.submitAndWait(blobs.length === 1 ? blobs[0] : multisign(blobs));
  const code = res.result.meta?.TransactionResult;
  if (code !== "tesSUCCESS") throw refuse(502, `${label} failed on ledger: ${code}`);
  return res.result.hash;
}

/** Runs an accepted operation through the quorum: anchor (+ payout), then marks the root anchored. */
async function anchorThroughQuorum(accepted, submission) {
  const memoHex = accepted.memoHex;
  const txs = [];
  const anchorTx = await client.autofill({
    TransactionType: "Payment", Account: env.POOL, Destination: env.SINK, Amount: "1",
    Memos: [memoOf(MEMO_ANCHOR, memoHex)],
  }, SIGNERS.length);
  // Pin the anchor's validity explicitly (do not depend on autofill's default offset): well inside
  // the sidecars' anchor window, so payout LLS below stays inside their payout window too.
  anchorTx.LastLedgerSequence = (await client.getLedgerIndex()) + ANCHOR_LLS_OFFSET;
  txs.push({ kind: "anchor", tx: anchorTx });
  if (accepted.payout) {
    const payoutTx = await client.autofill({
      TransactionType: "Payment", Account: env.POOL, Destination: accepted.payout.destination, Amount: String(accepted.payout.amount),
    }, SIGNERS.length);
    payoutTx.Sequence = anchorTx.Sequence + 1; // the anchor goes first
    // Generous validity so a failed payout can be resubmitted; well inside the sidecars' window.
    payoutTx.LastLedgerSequence = anchorTx.LastLedgerSequence + Math.floor(DEFAULT_PAYOUT_LLS_WINDOW * 0.8);
    txs.push({ kind: "payout", tx: payoutTx });
  }
  const results = await Promise.allSettled(SIGNERS.map((s) => http("POST", `${s}/sign`, { txs, submission, memoHex })));
  const signed = results.filter((r) => r.status === "fulfilled").map((r) => r.value);
  const refusals = results.filter((r) => r.status === "rejected").map((r) => r.reason?.message ?? String(r.reason));
  if (signed.length < QUORUM) throw refuse(409, `only ${signed.length}/${SIGNERS.length} signers endorsed (quorum ${QUORUM})`, { refusals });
  const use = signed.slice(0, QUORUM);
  const anchorHash = await submitMultisigned(use.map((s) => s.blobs.find((b) => b.kind === "anchor").tx_blob), "anchor");
  await Promise.allSettled(NODES.map((n) => http("POST", `${n}/anchored`, { root: accepted.newRoot, anchorTxHash: anchorHash })));
  let payoutHash = null, payoutBlob = null, payoutError = null;
  if (accepted.payout) {
    // The note is spent now (anchor landed). Pay; if the ledger refuses, hand the blob back so the
    // client can resubmit it until LastLedgerSequence — never pay before the anchor.
    payoutBlob = multisign(use.map((s) => s.blobs.find((b) => b.kind === "payout").tx_blob));
    try { payoutHash = await submitMultisigned([payoutBlob], "payout"); } catch (e) { payoutError = e.message; }
  }
  return { ...accepted, anchorTx: anchorHash, payoutTx: payoutHash, payoutBlob, payoutError, signers: use.map((s) => s.signer), refusals };
}

await serve(Number(env.PORT), {
  "GET /state": async () => http("GET", `${env.NODE_URL}/state`),
  "GET /log": async (_b, url) => http("GET", `${env.NODE_URL}/log?from=${url.searchParams.get("from") ?? 0}`),
  "POST /audit/reveal": async (body) => http("POST", `${env.NODE_URL}/audit/reveal`, body),
  "POST /submit/shield": async ({ record, depositTxHash }) => {
    const accepted = await http("POST", `${env.NODE_URL}/submit/shield`, { record, depositTxHash });
    // Signers' nodes re-read the deposit themselves; only the tx hash matters here.
    const submission = { op: "shield", record, deposit: { txHash: depositTxHash, delivered: -record.valueBalance, memoHash: accepted.bundleHash } };
    return anchorThroughQuorum(accepted, submission);
  },
  "POST /submit/transfer": async ({ record }) => {
    const accepted = await http("POST", `${env.NODE_URL}/submit/transfer`, { record });
    return anchorThroughQuorum(accepted, { op: "transfer", record });
  },
  "POST /submit/unshield": async ({ record, destination }) => {
    const accepted = await http("POST", `${env.NODE_URL}/submit/unshield`, { record, destination });
    return anchorThroughQuorum(accepted, { op: "unshield", record, destination });
  },
});
