// The browser wallet runs here, off the main thread: confidant-core compiled to WebAssembly
// (frontend/wasm → pkg/ and pkg-threads/). The spending key, the notes and the proving never leave
// this worker; the page only receives built bundles (record + hash), the address, the viewing key
// and the secret to persist.
//
// Two builds of the same crate: pkg-threads/ proves on a Web-Worker thread pool (rayon via
// wasm-bindgen-rayon) and is used when the page is cross-origin isolated (COOP/COEP headers, so
// SharedArrayBuffer exists); pkg/ is the single-threaded fallback that runs anywhere.
// The proving keys are built once per worker (eagerly, right after load — see "warm") and reused
// for every bundle; orchard 0.15 cannot serialize them, so they live as long as this worker does.

let wallet = null;
let ready = null;
let threads = 1;
let mod = null;

const canThread = () => self.crossOriginIsolated === true && typeof SharedArrayBuffer !== "undefined";

async function load() {
  if (canThread()) {
    try {
      mod = await import("./pkg-threads/confidant_wasm.js");
      await mod.default();
      threads = Math.max(1, Math.min(navigator.hardwareConcurrency || 1, 16));
      await mod.initThreadPool(threads);
      return;
    } catch (e) {
      console.warn("threaded wallet unavailable, falling back to single-threaded:", e?.message ?? e);
      mod = null; threads = 1;
    }
  }
  mod = await import("./pkg/confidant_wasm.js");
  await mod.default();
}
const ensureInit = () => (ready ??= load());

const handlers = {
  async create() { await ensureInit(); wallet = new mod.BrowserWallet(); return summary(); },
  async restore({ secret }) { await ensureInit(); wallet = mod.BrowserWallet.fromSecret(secret); return summary(); },
  // Builds the proving keys now (≈ 25 s single-threaded, a few seconds on a pool) so the first
  // operation does not pay for them. Idempotent; the page calls it as soon as a wallet exists.
  async warm() { await ensureInit(); const t = performance.now(); mod.BrowserWallet.warm(); return { threads, ms: Math.round(performance.now() - t) }; },
  async sync({ records }) { if (!wallet) throw new Error("no wallet"); const applied = wallet.sync(JSON.stringify(records)); return { applied, ...summary() }; },
  async shield({ amount }) { if (!wallet) throw new Error("no wallet"); return timed(() => wallet.buildShield(BigInt(amount))); },
  async transfer({ recipient, amount }) { if (!wallet) throw new Error("no wallet"); return timed(() => wallet.buildTransfer(recipient, BigInt(amount))); },
  async unshield({ amount, destination }) { if (!wallet) throw new Error("no wallet"); return timed(() => wallet.buildUnshield(BigInt(amount), destination)); },
  async forget() { wallet = null; return { ok: true }; },
};

function timed(build) {
  const t = performance.now();
  const out = JSON.parse(build());
  return { ...out, provingMs: Math.round(performance.now() - t), threads };
}

function summary() {
  const info = wallet.info();
  return { address: info.address, viewingKey: info.viewingKey, balance: Number(wallet.balance()), synced: wallet.synced(), secret: wallet.secret(), threads };
}

self.onmessage = async ({ data }) => {
  const { id, op, args } = data;
  try {
    const result = await handlers[op](args ?? {});
    self.postMessage({ id, result });
  } catch (e) {
    self.postMessage({ id, error: String(e?.message ?? e) });
  }
};
