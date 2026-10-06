// The resilient client: a dropped WebSocket is reconnected and the call retried; an interrupted
// submitAndWait is resolved from the ledger by hash instead of being resubmitted blindly.
import { test } from "node:test";
import assert from "node:assert/strict";
import { DisconnectedError, Wallet } from "xrpl";
import { resilientClient } from "../lib.mjs";

function fake(script) {
  const calls = [];
  let connected = true, dirty = false; // dirty = old socket still CLOSING: xrpl.js refuses connect() until disconnect() cleaned it
  const c = {
    on() {}, isConnected: () => connected,
    async connect() { if (dirty) throw new Error("Websocket connection never cleaned up."); connected = true; calls.push("connect"); },
    async disconnect() { connected = false; dirty = false; calls.push("disconnect"); },
    async request(r) { calls.push(`request:${r.command}`); return script.request(r); },
    async submitAndWait(blob) { calls.push("submitAndWait"); return script.submitAndWait(blob); },
    async autofill(tx) { calls.push("autofill"); return script.autofill ? script.autofill(tx) : { ...tx, Fee: "12", Sequence: 1 }; },
    drop() { connected = false; dirty = true; },
  };
  return { c, calls };
}
const quiet = { log: () => {} };
const w = Wallet.generate();
const blob = w.sign({ TransactionType: "Payment", Account: w.address, Destination: Wallet.generate().address, Amount: "1", Fee: "12", Sequence: 1, LastLedgerSequence: 99 }).tx_blob;

test("a request that hits a closing socket reconnects and succeeds", async () => {
  let n = 0;
  const { c, calls } = fake({ request: () => { if (n++ === 0) { c.drop(); throw new DisconnectedError("WebSocket is not open: readyState 2 (CLOSING)"); } return { result: { ok: true } }; } });
  const r = await resilientClient(c, quiet).request({ command: "ledger_current" });
  assert.deepEqual(r, { result: { ok: true } });
  assert.deepEqual(calls, ["request:ledger_current", "disconnect", "connect", "request:ledger_current"], "disconnect() cleans the closing socket before connect()");
});

test("other errors are not retried", async () => {
  const { c, calls } = fake({ request: () => { throw new Error("actNotFound"); } });
  await assert.rejects(resilientClient(c, quiet).request({ command: "account_info" }), /actNotFound/);
  assert.equal(calls.filter((x) => x.startsWith("request")).length, 1);
});

test("submitAndWait interrupted by a disconnect resolves from the ledger when the tx landed", async () => {
  const { c, calls } = fake({
    submitAndWait: () => { c.drop(); throw new DisconnectedError("websocket was closed"); },
    request: (r) => { assert.equal(r.command, "tx"); return { result: { validated: true, meta: { TransactionResult: "tesSUCCESS" }, hash: r.transaction } }; },
  });
  const res = await resilientClient(c, quiet).submitAndWait(blob);
  assert.equal(res.result.meta.TransactionResult, "tesSUCCESS");
  assert.match(res.result.hash, /^[0-9A-F]{64}$/);
  assert.equal(calls.filter((x) => x === "submitAndWait").length, 1, "never resubmitted");
});

test("submitAndWait interrupted by a disconnect resubmits when the ledger does not know the tx", async () => {
  let submits = 0;
  const { c } = fake({
    submitAndWait: () => { if (submits++ === 0) { c.drop(); throw new DisconnectedError("websocket was closed"); } return { result: { meta: { TransactionResult: "tesSUCCESS" }, hash: "X" } }; },
    request: () => { throw new Error("txnNotFound"); },
  });
  const res = await resilientClient(c, { ...quiet }).submitAndWait(blob);
  assert.equal(res.result.meta.TransactionResult, "tesSUCCESS");
  assert.equal(submits, 2);
});

test("a resubmit answered tefPAST_SEQ is resolved by hash", async () => {
  let submits = 0;
  const { c } = fake({
    submitAndWait: () => { submits++; if (submits === 1) { c.drop(); throw new DisconnectedError("websocket was closed"); } throw new Error("Transaction failed, tefPAST_SEQ"); },
    request: () => (submits < 2 ? (() => { throw new Error("txnNotFound"); })() : { result: { validated: true, meta: { TransactionResult: "tesSUCCESS" } } }),
  });
  const res = await resilientClient(c, quiet).submitAndWait(blob);
  assert.equal(res.result.meta.TransactionResult, "tesSUCCESS");
  assert.equal(submits, 2);
});

test("pass-through: connect/isConnected/on stay bound to the real client", async () => {
  const { c } = fake({ request: () => ({}) });
  const rc = resilientClient(c, quiet);
  assert.equal(rc.isConnected(), true);
  await rc.disconnect(); assert.equal(rc.isConnected(), false);
  await rc.connect(); assert.equal(rc.isConnected(), true);
  // A socket left CLOSING by the server: the wrapper's path works, a bare connect() would throw.
  c.drop(); await assert.rejects(c.connect(), /never cleaned up/);
  await rc.request({ command: "ping" }); assert.equal(rc.isConnected(), true);
});
