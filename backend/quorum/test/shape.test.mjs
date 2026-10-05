// The sidecar's "no blind signing" guard, in isolation: checkTxShape must accept exactly the
// transactions an endorsement authorizes and refuse every deviation. Mutation-tested: each `if` in
// checkTxShape has a case here that goes red when the `if` is removed.
import { test } from "node:test";
import assert from "node:assert/strict";
import { checkTxShape, memoOf, MEMO_ANCHOR } from "../lib.mjs";

const pool = "rPOOL", sink = "rSINK", memoHex = "01ab".repeat(8);
const endorse = { newRoot: "00".repeat(32), payout: { destination: "rBOB", amount: 25000000 } };
// The sidecar read: the pool account's next Sequence is 7 and the current ledger is 1000.
const ctx = { pool, sink, memoHex, endorse, expectedSequence: 7, ledgerIndex: 1000 };
const pctx = { ...ctx, expectedSequence: 8 }; // the payout follows the anchor
const anchor = () => ({ TransactionType: "Payment", Account: pool, Destination: sink, Amount: "1", Fee: "40", Sequence: 7, LastLedgerSequence: 1020, Memos: [memoOf(MEMO_ANCHOR, memoHex)] });
const payout = () => ({ TransactionType: "Payment", Account: pool, Destination: "rBOB", Amount: "25000000", Fee: "40", Sequence: 8, LastLedgerSequence: 1500 });
const refused = (kind, tx, re, c = kind === "payout" ? pctx : ctx) => assert.throws(() => checkTxShape(kind, tx, c), (e) => { assert.equal(e.status, 409); assert.match(e.message, re); return true; });

test("honest anchor and payout pass", () => {
  checkTxShape("anchor", anchor(), ctx);
  checkTxShape("payout", payout(), pctx);
  checkTxShape("anchor", { ...anchor(), Memos: [memoOf(MEMO_ANCHOR, memoHex.toUpperCase())] }, ctx); // hex case-insensitive
  checkTxShape("anchor", { ...anchor(), LastLedgerSequence: 1060 }, ctx); // window edge inclusive
  checkTxShape("payout", { ...payout(), LastLedgerSequence: 1600 }, pctx);
});

test("sequence is pinned to the pool account's next sequence as the sidecar read it", () => {
  refused("anchor", { ...anchor(), Sequence: 8 }, /Sequence 8 is not the pool account's next sequence 7/);
  refused("anchor", { ...anchor(), Sequence: 6 }, /not the pool account's next sequence/);
  refused("anchor", { ...anchor(), Sequence: undefined }, /not the pool account's next sequence/);
  refused("anchor", { ...anchor(), Sequence: "7" }, /not the pool account's next sequence/);
  refused("payout", { ...payout(), Sequence: 7 }, /not the pool account's next sequence 8/);
  refused("anchor", anchor(), /not the pool account's next sequence/, { ...ctx, expectedSequence: undefined });
});

test("LastLedgerSequence is required and bounded (short for anchors, generous for payouts)", () => {
  refused("anchor", { ...anchor(), LastLedgerSequence: undefined }, /LastLedgerSequence .* missing or outside \(1000, 1060\]/);
  refused("anchor", { ...anchor(), LastLedgerSequence: 1000 }, /missing or outside/); // already expired
  refused("anchor", { ...anchor(), LastLedgerSequence: 900 }, /missing or outside/);
  refused("anchor", { ...anchor(), LastLedgerSequence: 1061 }, /missing or outside/);
  refused("anchor", { ...anchor(), LastLedgerSequence: 1500 }, /missing or outside/); // payout-sized window is not for anchors
  refused("payout", { ...payout(), LastLedgerSequence: 1601 }, /outside \(1000, 1600\]/);
  refused("payout", { ...payout(), LastLedgerSequence: undefined }, /missing or outside/);
  refused("anchor", anchor(), /current ledger index unknown/, { ...ctx, ledgerIndex: undefined });
  refused("anchor", { ...anchor(), LastLedgerSequence: 1011 }, /outside \(1000, 1010\]/, { ...ctx, anchorLlsWindow: 10 });
});

test("common: not a Payment, foreign account, fee", () => {
  refused("anchor", { ...anchor(), TransactionType: "AccountSet" }, /not a Payment/);
  refused("anchor", undefined, /not a Payment/);
  refused("anchor", { ...anchor(), Account: "rEVE" }, /Account is not the pool/);
  refused("payout", { ...payout(), Account: "rEVE" }, /Account is not the pool/);
  refused("anchor", { ...anchor(), Fee: "5001" }, /fee missing or above 5000/);
  refused("anchor", { ...anchor(), Fee: undefined }, /fee missing/);
  refused("anchor", { ...anchor(), Fee: "1e3" }, /fee missing/);
  checkTxShape("anchor", { ...anchor(), Fee: "5000" }, ctx);
  refused("anchor", { ...anchor(), Fee: "900" }, /above 100 drops/, { ...ctx, maxFeeDrops: 100 });
});

test("anchor: destination, amount, memo count, memo type and bytes", () => {
  refused("anchor", { ...anchor(), Destination: "rEVE" }, /destination is not the sink/);
  refused("anchor", { ...anchor(), Amount: "2" }, /amount is not 1 drop/);
  refused("anchor", { ...anchor(), Amount: "25000000" }, /amount is not 1 drop/);
  refused("anchor", { ...anchor(), Memos: [] }, /exactly one memo/);
  refused("anchor", { ...anchor(), Memos: undefined }, /exactly one memo/);
  refused("anchor", { ...anchor(), Memos: [memoOf(MEMO_ANCHOR, memoHex), memoOf(MEMO_ANCHOR, memoHex)] }, /exactly one memo/);
  refused("anchor", { ...anchor(), Memos: [memoOf("confidant/shield", memoHex)] }, /memo is not the endorsed/);
  const lie = "02" + memoHex.slice(2);
  refused("anchor", { ...anchor(), Memos: [memoOf(MEMO_ANCHOR, lie)] }, /memo is not the endorsed/);
});

test("payout: only what the sidecar's own node endorsed", () => {
  refused("payout", payout(), /has no payout/, { ...pctx, endorse: { newRoot: endorse.newRoot } });
  refused("payout", payout(), /has no payout/, { ...pctx, endorse: undefined });
  refused("payout", { ...payout(), Destination: "rEVE" }, /destination differs/);
  refused("payout", { ...payout(), Amount: "50000000" }, /amount differs/);
  refused("payout", { ...payout(), Amount: "24999999" }, /amount differs/);
  refused("payout", { ...payout(), Memos: [memoOf(MEMO_ANCHOR, memoHex)] }, /unexpected memos/);
  checkTxShape("payout", { ...payout(), Memos: [] }, pctx);
});

test("unknown kinds are refused", () => {
  refused("refund", { ...payout(), LastLedgerSequence: 1020 }, /unknown tx kind/, pctx);
});
