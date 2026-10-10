// The browser wallet runs here, off the main thread: confidant-core compiled to WebAssembly
// (frontend/wasm → pkg/). The spending key, the notes and the proving never leave this worker;
// the page only receives built bundles (record + hash), the address, the viewing key and the
// secret to persist. Proving is single-threaded WebAssembly: ~20 s per bundle, ~25 s once per
// session to build the proving keys.
import init, { BrowserWallet } from "./pkg/confidant_wasm.js";

let wallet = null;
let ready = null;

const ensureInit = () => (ready ??= init());

const handlers = {
  async create() { await ensureInit(); wallet = new BrowserWallet(); return summary(); },
  async restore({ secret }) { await ensureInit(); wallet = BrowserWallet.fromSecret(secret); return summary(); },
  async sync({ records }) { if (!wallet) throw new Error("no wallet"); const applied = wallet.sync(JSON.stringify(records)); return { applied, ...summary() }; },
  async shield({ amount }) { if (!wallet) throw new Error("no wallet"); return JSON.parse(wallet.buildShield(BigInt(amount))); },
  async transfer({ recipient, amount }) { if (!wallet) throw new Error("no wallet"); return JSON.parse(wallet.buildTransfer(recipient, BigInt(amount))); },
  async unshield({ amount, destination }) { if (!wallet) throw new Error("no wallet"); return JSON.parse(wallet.buildUnshield(BigInt(amount), destination)); },
  async forget() { wallet = null; return { ok: true }; },
};

function summary() {
  const info = wallet.info();
  return { address: info.address, viewingKey: info.viewingKey, balance: Number(wallet.balance()), synced: wallet.synced(), secret: wallet.secret() };
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
