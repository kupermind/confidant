// Headless-Chrome smoke test of the static pages in demo mode (`?demo=1`, no backend, no
// network): what a phone shows, not what the services do (that is backend/quorum/smoke-ui.mjs).
//
//   node frontend/test/browser-smoke.mjs            (CHROME=<path or command> to pick the browser)
//
// Each check loads a page inside an iframe of a phone width (headless Chrome ignores window sizes
// below 500 px, the iframe does not), lets a probe script write what it sees into <title>, and reads
// the DOM back after a virtual-time budget — so the demo's simulated delays (faucet 2.5 s, proving,
// deposit, anchor) are sampled deterministically. Exits 1 on any failed check; exits 0 with a notice
// when no Chrome is available.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const pages = join(here, "..");

function findChrome() {
  const candidates = [process.env.CHROME, "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "google-chrome", "google-chrome-stable", "chromium", "chromium-browser"].filter(Boolean);
  for (const c of candidates) {
    if (c.includes("/") ? existsSync(c) : spawnSync("which", [c], { stdio: "ignore" }).status === 0) return c;
  }
  return null;
}
const CHROME = findChrome();
if (!CHROME) { console.log("browser-smoke: no Chrome found (set CHROME=…); skipped"); process.exit(0); }

const dir = mkdtempSync(join(tmpdir(), "confidant-smoke-"));
const OVERFLOW = `const cw=document.documentElement.clientWidth;const over=[...document.querySelectorAll("body *")].filter(e=>e.getBoundingClientRect().right>cw+1).length;`;
const text = (id) => `(document.getElementById("${id}")?.textContent??"").trim()`;

// A page copy with a probe: `setup` runs at DOMContentLoaded, `report` is re-evaluated every 150 ms
// and its value becomes the document title (what --dump-dom lets us read back).
function probe(name, setup, report) {
  const src = readFileSync(join(pages, name), "utf8");
  const inj = `<script>document.addEventListener("DOMContentLoaded",()=>{${setup};setInterval(()=>{try{${OVERFLOW}document.title=JSON.stringify({over,...(${report})})}catch(e){document.title=JSON.stringify({err:String(e)})}},150)})</script></body>`;
  const file = join(dir, `${name.replace(".html", "")}-${Math.random().toString(36).slice(2, 8)}.html`);
  writeFileSync(file, src.replace("</body>", inj));
  return file;
}
function frame(file, width, query = "?demo=1") {
  const f = join(dir, `frame-${Math.random().toString(36).slice(2, 8)}.html`);
  writeFileSync(f, `<!doctype html><html><body style="margin:0"><iframe id="f" src="${file}${query}" style="width:${width}px;height:3000px;border:0;display:block"></iframe><script>setInterval(()=>{try{document.title=document.getElementById("f").contentDocument.title}catch(e){}},150)</script></body></html>`);
  return f;
}
function sample(file, width, budgetMs, query) {
  const out = execFileSync(CHROME, ["--headless=new", "--disable-gpu", "--no-sandbox", "--no-first-run", "--allow-file-access-from-files", `--window-size=${width},3000`, `--virtual-time-budget=${budgetMs}`, "--dump-dom", `file://${frame(file, width, query)}`], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 120_000 });
  const m = out.match(/<title>([^<]*)<\/title>/);
  const raw = (m?.[1] ?? "").replace(/&quot;/g, '"').replace(/&amp;/g, "&");
  try { return JSON.parse(raw); } catch { return { err: `no probe title: ${raw.slice(0, 80)}` }; }
}

let pass = 0, fail = 0;
const check = (name, ok, detail) => { if (ok) pass++; else fail++; console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok ? "" : `  →  ${JSON.stringify(detail)}`}`); };

// 1. Static layout: nothing wider than the viewport on any page at 320 and 390 px.
for (const name of ["index.html", "how-it-works.html", "balances.html", "lender.html"]) {
  const file = probe(name, "", "{}");
  for (const w of [320, 390]) {
    const r = sample(file, w, 2500);
    check(`${name} @${w}px: no horizontal overflow`, r.over === 0, r);
  }
}

// 2. The app's header stat and the balances page's live check (demo data).
{
  const r = sample(probe("index.html", "", `{stat:${text("st-pool-big")},brk:${text("pool-split")}}`), 390, 2500);
  check("index: shielded total shown in the header", /^[\d,.]+$/.test(r.stat ?? ""), r);
  check("index: reconciliation line under the stat", /Pool account .* = .* shielded \+ .* float and unswept fees/.test(r.brk ?? ""), r);
  const b = sample(probe("balances.html", "", `{chk:${text("r-check")},bal:${text("r-balance")}}`), 390, 2500);
  check("balances: the account table reconciles (check row matches)", /matches/.test(b.chk ?? "") && /\d/.test(b.bal ?? ""), b);
}

// 3. Create wallet: the card appears at once and says the faucet is running; then it is funded.
{
  const file = probe("index.html", `setTimeout(()=>document.getElementById("btn-create").click(),300)`,
    `{addr:${text("xrpl-addr")},bal:${text("xrpl-balance")},fund:document.getElementById("btn-fund").disabled,msg:${text("wallet-progress")}}`);
  const mid = sample(file, 390, 1600);
  check("create: mid-faucet — XRPL address shown, 'funding…', fund button disabled, message visible",
    /^r/.test(mid.addr ?? "") && /^Funding/.test(mid.bal ?? "") && mid.fund === true && /Funding your testnet account/.test(mid.msg ?? ""), mid);
  const done = sample(file, 390, 6000);
  check("create: funded — balance shown, fund button enabled, message cleared", /XRP$/.test(done.bal ?? "") && done.fund === false && (done.msg ?? "") === "", done);
}

// 4. Shield while the faucet is still funding is refused with a plain message (nothing is sent).
{
  const file = probe("index.html",
    `setTimeout(()=>document.getElementById("btn-create").click(),300);setTimeout(()=>{document.getElementById("shield-amount").value="25";document.getElementById("btn-shield").click()},900)`,
    `{steps:[...document.getElementById("steps-shield").children].map(e=>(e.classList.contains("on")?"on":"")+(e.classList.contains("done")?"✓":"")).join("|"),msg:${text("msg-shield")}}`);
  const r = sample(file, 390, 1400);
  check("shield during funding: refused, no step started", /still funding/.test(r.msg ?? "") && r.steps === "||", r);
}

// 5. Shield after funding: the three step ticks follow the phases
//    (demo timings: faucet 2.5 s, then prove 0.9 s, deposit 1.2 s, anchor 1.4 s; Shield is clicked at 3.2 s).
{
  const file = probe("index.html",
    `setTimeout(()=>document.getElementById("btn-create").click(),300);setTimeout(()=>{document.getElementById("shield-amount").value="25";document.getElementById("btn-shield").click()},3200)`,
    `{steps:[...document.getElementById("steps-shield").children].map(e=>(e.classList.contains("on")?"on":"")+(e.classList.contains("done")?"✓":"")).join("|"),msg:${text("msg-shield")}}`);
  // The demo holder's entry credential is issued, not confirmed: Shield confirms it first (0.9 s), then the steps run.
  const expect = { 4400: "on||", 5400: "✓|on|", 6600: "✓|✓|on", 8400: "✓|✓|✓" };
  for (const [budget, want] of Object.entries(expect)) {
    const r = sample(file, 390, Number(budget));
    check(`shield ticks at ${budget} ms: ${want}`, r.steps === want, r);
  }
  const end = sample(file, 390, 8400);
  check("shield: success line", /^Shielded 25 XRP/.test(end.msg ?? ""), end);
}

// 6. Shield recovery: the deposit landed, the answer was lost (?failsubmit makes the demo's first submit fail
//    once) → the page keeps the record and offers "Resume anchoring"; resuming anchors it and clears the record.
{
  const q = "?demo=1&failsubmit";
  const file = probe("index.html",
    `setTimeout(()=>document.getElementById("btn-create").click(),300);setTimeout(()=>{document.getElementById("shield-amount").value="25";document.getElementById("btn-shield").click()},3200);setTimeout(()=>document.getElementById("btn-resume-shield")?.click(),7300)`,
    `{msg:${text("msg-shield")},pending:(()=>{try{return !!localStorage.getItem("confidant.pending-shield")}catch(e){return null}})(),resume:!!document.getElementById("btn-resume-shield")}`);
  const lost = sample(file, 390, 6900, q);
  check("shield recovery: lost answer → record kept, 'Resume anchoring' offered", lost.pending === true && lost.resume === true && /not anchored yet/.test(lost.msg ?? ""), lost);
  const done = sample(file, 390, 9500, q);
  check("shield recovery: resume anchors it and clears the record", done.pending === false && /^Shielded 25 XRP/.test(done.msg ?? ""), done);
}

// 7. The under-the-hood page: layout at phone widths; the team fold saves an issuer key and issues the entry
//    credential to the app's own account (demo: the saved key counts as the pool's issuer).
{
  const file = probe("issuer.html", "", "{}");
  for (const w of [320, 390]) { const r = sample(file, w, 2000); check(`issuer.html @${w}px: no horizontal overflow`, r.over === 0, r); }
  const flow = probe("issuer.html",
    `localStorage.setItem("confidant.xrpl",JSON.stringify({address:"rJASAbKaKY7LybEdFwkgpv5gCc2YA9VYbF",seed:"x"}));setTimeout(()=>{document.getElementById("seed").value=xrpl.Wallet.generate().seed;document.getElementById("btn-save-key").click()},500);setTimeout(()=>document.getElementById("btn-issue").click(),900)`,
    `{key:${text("key-addr")},to:${text("issue-to")},msg:${text("key-msg")},trusted:${text("trusted")}}`);
  const r = sample(flow, 390, 2800);
  check("under the hood: key saved as the pool's issuer, credential issued to this browser's wallet", /^rDEMOissuer/.test(r.key ?? "") && /rJASAbKaKY7LybEdFwkgpv5gCc2YA9VYbF/.test(r.to ?? "") && /^Issued ✓ to rJASAbKaKY7LybEdFwkgpv5gCc2YA9VYbF/.test(r.msg ?? "") && /ConfidantKYC/.test(r.trusted ?? ""), r);
}

// 7b. The entry gate under the hood (demo, ?nocred: the account holds no entry credential at all).
//     Without the issuer key: Shield says the one step first and pays nothing; the Prove tab says it before any press.
//     With the key in this browser: Shield issues and confirms the credential, then shields; the Credential row ends "accepted ✓".
{
  const report = `{msg:${text("msg-shield")},gate:${text("prove-gate")},cred:${text("cred-status")},steps:[...document.getElementById("steps-shield").children].map(e=>e.classList.contains("on")||e.classList.contains("done")?"x":"").join("|"),acts:[...document.querySelectorAll("#activity .type")].map(e=>e.textContent).join(",")}`;
  const noKey = probe("index.html",
    `setTimeout(()=>document.getElementById("btn-create").click(),300);setTimeout(()=>{document.getElementById("shield-amount").value="25";document.getElementById("btn-shield").click()},3200)`, report);
  const n = sample(noKey, 390, 4500, "?demo=1&nocred");
  check("gate, no key: shield refused before any payment with the one step first; the Prove tab says it too", /^One step first: .*under-the-hood page/.test(n.msg ?? "") && /^One step first/.test(n.gate ?? "") && n.steps === "||", n);
  const withKey = probe("index.html",
    `localStorage.setItem("confidant.issuer",JSON.stringify({seed:"s",address:"rDEMOissuerKYCxxxxxxxxxxxxxxxxxxxx"}));setTimeout(()=>document.getElementById("btn-create").click(),300);setTimeout(()=>{document.getElementById("shield-amount").value="25";document.getElementById("btn-shield").click()},3200)`, report);
  const mid = sample(withKey, 390, 4000, "?demo=1&nocred");
  check("gate, with key: shield first issues the entry credential under the hood", /Issuing the entry credential/.test(mid.msg ?? "") && mid.steps === "||", mid);
  const end = sample(withKey, 390, 9400, "?demo=1&nocred");
  check("gate, with key: credential confirmed, shield completed, Credential row accepted, Activity has the credential line", /^Shielded 25 XRP/.test(end.msg ?? "") && /^accepted ✓/.test(end.cred ?? "") && (end.acts ?? "").split(",").includes("credential") && (end.gate ?? "") === "", end);
}

// 8. Credential gate, holder side (demo config carries a gate; the holder arrives with a credential issued):
//    the Credential row reads "issued — confirm it to shield" with a Confirm button; confirming turns it into "accepted ✓".
{
  const file = probe("index.html",
    `setTimeout(()=>document.getElementById("btn-create").click(),300);setTimeout(()=>document.getElementById("btn-accept-cred").click(),3400)`,
    `{row:!document.getElementById("cred-row").hidden,status:${text("cred-status")},btn:document.getElementById("btn-accept-cred").hidden}`);
  const before = sample(file, 390, 3200);
  check("credential: row shown, issued, Confirm offered", before.row === true && /^issued — confirm it/.test(before.status ?? "") && before.btn === false, before);
  const after = sample(file, 390, 5200);
  check("credential: accepted ✓ after the Confirm step", /^accepted ✓/.test(after.status ?? "") && after.btn === true, after);
}

// 10. Proof of funds, holder side (demo): create the wallet, shield 25 XRP (24.9375 after the demo's 25 bps fee), then the
//     one button — the whole balance is proved, the quorum writes the credential, the account confirms it, the notes are refreshed; the balance stays.
//     Demo timings: faucet 2.5 s; shield confirms the entry credential 0.9 s + shields 3.5 s; prove 0.9 s, credential 1.5 s,
//     confirm 0.9 s; refresh 0.9 + 1.4 s.
{
  const setup = `setTimeout(()=>document.getElementById("btn-create").click(),300);setTimeout(()=>{document.getElementById("shield-amount").value="25";document.getElementById("btn-shield").click()},3200);setTimeout(()=>{document.getElementById("tabbtn-prove").click();document.getElementById("btn-prove").click()},8000)`;
  const report = `{steps:[...document.getElementById("steps-prove").children].map(e=>(e.classList.contains("on")?"on":"")+(e.classList.contains("done")?"✓":"")).join("|"),msg:${text("msg-prove")},refresh:${text("msg-prove-refresh")},details:${text("prove-details")},label:${text("btn-prove")},gate:${text("prove-gate")},bal:${text("priv-balance")},acts:[...document.querySelectorAll("#activity .type")].map(e=>e.textContent).join(","),hidden:document.getElementById("tab-prove").hidden}`;
  const file = probe("index.html", setup, report);
  const mid = sample(file, 390, 11900);
  check("prove: one button — Prove, Record, Confirm done, Refresh under way; the line names the whole balance and the lender link",
    mid.steps === "✓|✓|✓|on" && /^Done ✓/.test(mid.msg ?? "") && /controlled at least 24.9375 XRP as of ledger 12345678/.test(mid.msg ?? "") && /lender page/.test(mid.msg ?? "") && /Refreshing your notes/.test(mid.refresh ?? "") && mid.hidden === false, mid);
  const fin = sample(file, 390, 14800);
  check("prove: all four steps done, notes refreshed, balance unchanged, receipt + under-the-hood link, Activity lines, button re-labelled",
    fin.steps === "✓|✓|✓|✓" && /^Notes refreshed ✓/.test(fin.refresh ?? "") && /Download the receipt/.test(fin.details ?? "") && /under the hood/.test(fin.details ?? "") && fin.bal === "24.9375" && /Request req-[0-9a-f]{12}/.test(fin.details ?? "") && /confidant\/pof\/v1\|24937500\|12345678\|12367278\|/.test(fin.details ?? "")
      && (fin.acts ?? "").split(",").filter((t) => t === "credential").length === 2 && (fin.acts ?? "").split(",").filter((t) => t === "transfer").length === 1 && fin.label === "Prove I hold 24.9375 XRP" && (fin.gate ?? "") === "", fin);
  const empty = probe("index.html",
    `setTimeout(()=>document.getElementById("btn-create").click(),300);setTimeout(()=>{document.getElementById("tabbtn-prove").click();document.getElementById("btn-prove").click()},3200)`,
    `{msg:${text("msg-prove")},steps:[...document.getElementById("steps-prove").children].map(e=>e.classList.contains("on")||e.classList.contains("done")?"x":"").join("|")}`);
  const b = sample(empty, 390, 4500);
  check("prove: an empty private balance is refused before proving", /Nothing to prove/.test(b.msg ?? "") && b.steps === "||", b);
}

// 11. Lender page (demo): the holder's link carries the borrower's address, so the verdict loads by itself; another
//     account has no credential and is told what the borrower should do; a bad address is refused.
{
  const report = `{res:${text("result")},busy:document.getElementById("btn-check").disabled}`;
  const hit = sample(probe("lender.html", "", report), 390, 2500, "?demo=1&account=rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpf");
  check("lender: the credential is read from the ledger — amount, ledger, confirmed, signers, the honest line",
    /^rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpf controlled at least 25 XRP in the pool as of ledger 12345650/.test(hit.res ?? "") && /Confirmed by the borrower's own signature ✓/.test(hit.res ?? "") && /2 of 3 signers/.test(hit.res ?? "") && /not "controls it now"/.test(hit.res ?? "") && hit.busy === false, hit);
  const none = sample(probe("lender.html", "", report), 390, 2500, "?demo=1&account=rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh");
  check("lender: an account without the credential is told so, and what the borrower should do", /^No proof of funds on this account/.test(none.res ?? "") && /Prove funds/.test(none.res ?? ""), none);
  const bad = sample(probe("lender.html", `setTimeout(()=>{document.getElementById("account").value="nobody";document.getElementById("btn-check").click()},400)`, report), 390, 1500);
  check("lender: a bad address is refused with a readable reason", /not a classic XRPL address/.test(bad.res ?? ""), bad);
}

console.log(`\n${pass}/${pass + fail} checks ok${fail ? ` — ${fail} FAILED` : ""}`);
process.exit(fail ? 1 : 0);
