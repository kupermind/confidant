// Headless-Chrome smoke test of the three static pages in demo mode (`?demo=1`, no backend, no
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
for (const name of ["index.html", "how-it-works.html", "balances.html"]) {
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
  const expect = { 3500: "on||", 4500: "✓|on|", 5700: "✓|✓|on", 7500: "✓|✓|✓" };
  for (const [budget, want] of Object.entries(expect)) {
    const r = sample(file, 390, Number(budget));
    check(`shield ticks at ${budget} ms: ${want}`, r.steps === want, r);
  }
  const end = sample(file, 390, 7500);
  check("shield: success line", /^Shielded 25 XRP/.test(end.msg ?? ""), end);
}

// 6. Shield recovery: the deposit landed, the answer was lost (?failsubmit makes the demo's first submit fail
//    once) → the page keeps the record and offers "Resume anchoring"; resuming anchors it and clears the record.
{
  const q = "?demo=1&failsubmit";
  const file = probe("index.html",
    `setTimeout(()=>document.getElementById("btn-create").click(),300);setTimeout(()=>{document.getElementById("shield-amount").value="25";document.getElementById("btn-shield").click()},3200);setTimeout(()=>document.getElementById("btn-resume-shield")?.click(),6400)`,
    `{msg:${text("msg-shield")},pending:(()=>{try{return !!localStorage.getItem("confidant.pending-shield")}catch(e){return null}})(),resume:!!document.getElementById("btn-resume-shield")}`);
  const lost = sample(file, 390, 6000, q);
  check("shield recovery: lost answer → record kept, 'Resume anchoring' offered", lost.pending === true && lost.resume === true && /not anchored yet/.test(lost.msg ?? ""), lost);
  const done = sample(file, 390, 8600, q);
  check("shield recovery: resume anchors it and clears the record", done.pending === false && /^Shielded 25 XRP/.test(done.msg ?? ""), done);
}

console.log(`\n${pass}/${pass + fail} checks ok${fail ? ` — ${fail} FAILED` : ""}`);
process.exit(fail ? 1 : 0);
