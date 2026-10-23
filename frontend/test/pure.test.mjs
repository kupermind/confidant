// Unit tests of the pure functions inside the pages' inline scripts, run with `node --test "frontend/test/*.test.mjs"`.
// The pages are single static files (no bundler, no modules), so each function under test is pulled
// out of the page source by name and evaluated on its own; if a page is edited so that a function no
// longer stands alone, the extraction fails loudly rather than silently testing something else.
//
//   balances.html  bucket()  — classifies the pool account's ledger history into the reconciliation rows
//   balances.html  feeOf()   — the protocol-fee rounding the nodes use (floor per operation)
//   index.html     standbyText() — the screening-standby message shown while a shield waits
//
// Nothing here needs a backend, a browser or the network; the ledger fixture is public data.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const page = (name) => readFileSync(join(here, "..", name), "utf8");
const inline = (html) => [...html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g)].map((m) => m[1]).join("\n");

/** Extracts top-level `const NAME = …;` one-liners and two-space-indented `function NAME(…) { … }` blocks. */
function extract(src, names) {
  const parts = names.map((n) => {
    const re = new RegExp(`^  (?:const ${n} = [^\\n]*\\n|(?:async )?function ${n}\\([^)]*\\) \\{[\\s\\S]*?\\n  \\}\\n)`, "m");
    const m = src.match(re);
    if (!m) throw new Error(`${n} not found as a stand-alone definition`);
    return m[0];
  });
  return new Function(`${parts.join("\n")}\nreturn { ${names.join(", ")} };`)();
}

const PAGES = ["index.html", "how-it-works.html", "balances.html"];

test("every page's inline script parses (node --check)", () => {
  const dir = mkdtempSync(join(tmpdir(), "confidant-pages-"));
  for (const name of PAGES) {
    const src = inline(page(name));
    if (!src.trim()) continue; // how-it-works.html carries no script, by design
    const f = join(dir, name.replace(".html", ".mjs"));
    writeFileSync(f, src);
    execFileSync(process.execPath, ["--check", f], { stdio: "pipe" });
  }
});

test("how-it-works.html stays static: no <script> at all", () => {
  assert.equal((page("how-it-works.html").match(/<script/g) ?? []).length, 0);
});

test("feeOf floors to the drop, as the nodes do", () => {
  const { feeOf } = extract(inline(page("balances.html")), ["feeOf"]);
  assert.equal(feeOf(5_000_000, 25), 12_500);
  assert.equal(feeOf(1, 25), 0);
  assert.equal(feeOf(10_000_000, 0), 0);
  assert.equal(feeOf("30000000", 25), 75_000);
  assert.equal(feeOf(999_999, 25), 2_499);
});

test("bucket() reconciles the demo pool's public history to the drop", () => {
  const { bucket } = extract(inline(page("balances.html")), ["bucket"]);
  const fx = JSON.parse(readFileSync(join(here, "fixtures", "pool-account-tx.json"), "utf8"));
  const b = bucket(fx.transactions, fx.pool, fx.treasury);
  for (const [k, v] of Object.entries(fx.expected)) assert.equal(b[k], v, k);
  const computed = b.funding + b.deposits - b.payouts - b.sweeps - b.anchors - b.txFees;
  assert.equal(computed, fx.balance, "funding + deposits − payouts − sweeps − anchors − fees = balance on the ledger");
});

test("bucket() classifies each kind of transaction and ignores what it must", () => {
  const { bucket } = extract(inline(page("balances.html")), ["bucket"]);
  const POOL = "rPOOL", SINK = "rSINK", TREAS = "rTREAS", USER = "rUSER";
  const shieldMemo = { Memo: { MemoType: "636f6e666964616e742f736869656c64", MemoData: "AB" } }; // hex("confidant/shield")
  const ok = { TransactionResult: "tesSUCCESS" };
  const txs = [
    { tx: { TransactionType: "Payment", Account: USER, Destination: POOL, Amount: "100000000" }, meta: ok }, // genesis funding, no memo
    { tx: { TransactionType: "Payment", Account: USER, Destination: POOL, Amount: "5000000", Memos: [shieldMemo] }, meta: { ...ok, delivered_amount: "5000000" } },
    { tx: { TransactionType: "Payment", Account: USER, Destination: POOL, Amount: "7000000", Memos: [shieldMemo] }, meta: { TransactionResult: "tecUNFUNDED_PAYMENT" } }, // failed: ignored
    { tx: { TransactionType: "Payment", Account: POOL, Destination: SINK, Amount: "1", Fee: "12" }, meta: ok }, // anchor
    { tx: { TransactionType: "Payment", Account: POOL, Destination: USER, DeliverMax: "2493750", Fee: "12" }, meta: { ...ok, delivered_amount: "2493750" } }, // payout (API v2 shape)
    { tx: { TransactionType: "Payment", Account: POOL, Destination: TREAS, Amount: "12500", Fee: "12" }, meta: ok }, // fee sweep
    { tx: { TransactionType: "SignerListSet", Account: POOL, Fee: "20" }, meta: ok }, // fee only
    { tx: { TransactionType: "Payment", Account: USER, Destination: POOL, Amount: { currency: "USD", issuer: USER, value: "5" }, Memos: [shieldMemo] }, meta: ok }, // issued currency: no drops
  ];
  const b = bucket(txs, POOL, TREAS);
  assert.deepEqual(b, { funding: 100_000_000, deposits: 5_000_000, depositFees: 0, payouts: 2_493_750, sweeps: 12_500, anchors: 1, txFees: 56, count: 7 });
});

test("standbyText turns the node's refusal into progress wording and keeps the original in the tooltip", () => {
  const { standbyText } = extract(inline(page("index.html")), ["standbyText"]);
  const raw = 'refused by screening policy: deposit in ledger 21738764 is younger than the standby of 2 ledgers (validated ledger 21738764); retry at ledger 21738766';
  const html = standbyText(raw);
  assert.match(html, /Anchoring — the verifiers wait until the deposit is 2 ledgers old \(≈ 8 s\)/);
  assert.ok(html.includes(`title="${raw}"`), "the node's own words stay in the tooltip");
  assert.match(standbyText("standby of 1 ledger"), /1 ledger old \(≈ 4 s\)/);
  assert.match(standbyText("something else entirely"), /a few ledgers old/);
  assert.ok(standbyText('a "quoted" message').includes("&quot;quoted&quot;"), "quotes are escaped inside the attribute");
});
