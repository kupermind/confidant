//! The browser wallet: `confidant_core::Wallet` behind wasm-bindgen. Everything a holder must keep
//! private — the spending key, the notes, proving and signing — happens inside the page; the page
//! only ever sends built bundles to the coordinator and the deposit from its own XRPL account.

use confidant_core::{BundleRecord, TransferOutput, Wallet, WalletSecret};
use wasm_bindgen::prelude::*;

/// Threaded build only: `await initThreadPool(n)` from JS before any proving, so rayon's global
/// pool is backed by Web Workers instead of (unsupported) `std::thread::spawn`.
#[cfg(feature = "threads")]
pub use wasm_bindgen_rayon::init_thread_pool;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct BrowserWallet {
    inner: Wallet,
}

fn js(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

#[wasm_bindgen]
impl BrowserWallet {
    /// A fresh spending key (randomness from the browser's `crypto.getRandomValues`).
    #[wasm_bindgen(constructor)]
    pub fn new() -> BrowserWallet {
        BrowserWallet {
            inner: Wallet::new(),
        }
    }

    /// Restores a wallet from the JSON `WalletSecret` the page keeps.
    #[wasm_bindgen(js_name = fromSecret)]
    pub fn from_secret(secret_json: &str) -> Result<BrowserWallet, JsValue> {
        let secret: WalletSecret = serde_json::from_str(secret_json).map_err(js)?;
        Ok(BrowserWallet {
            inner: Wallet::from_secret(&secret).map_err(js)?,
        })
    }

    /// Builds the proving key now (once per worker; the page calls it right after the wallet
    /// exists, so the first bundle does not wait for it).
    pub fn warm() {
        confidant_core::Params::get().pk();
    }

    /// The JSON `WalletSecret` to persist (this is the spending key — keep it in the browser).
    pub fn secret(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.inner.secret()).map_err(js)
    }

    /// `{ address, viewingKey }`.
    pub fn info(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.info()).map_err(js)
    }

    pub fn balance(&self) -> u64 {
        self.inner.balance()
    }

    pub fn synced(&self) -> usize {
        self.inner.synced()
    }

    /// Catches up with the public log (`GET /log` → `records` as JSON). Returns records applied.
    pub fn sync(&mut self, records_json: &str) -> Result<usize, JsValue> {
        let log: Vec<BundleRecord> = serde_json::from_str(records_json).map_err(js)?;
        self.inner.sync(&log).map_err(js)
    }

    /// Builds, proves and signs a shield. Returns `{ record, bundleHash }` as JSON.
    #[wasm_bindgen(js_name = buildShield)]
    pub fn build_shield(&self, amount: u64) -> Result<String, JsValue> {
        let p = self.inner.build_shield(amount).map_err(js)?;
        built(p)
    }

    #[wasm_bindgen(js_name = buildTransfer)]
    pub fn build_transfer(&self, recipient: &str, amount: u64) -> Result<String, JsValue> {
        let p = self
            .inner
            .build_transfer(&[TransferOutput {
                recipient: recipient.to_owned(),
                amount,
            }])
            .map_err(js)?;
        built(p)
    }

    #[wasm_bindgen(js_name = buildUnshield)]
    pub fn build_unshield(&self, amount: u64, destination: &str) -> Result<String, JsValue> {
        let p = self.inner.build_unshield(amount, destination).map_err(js)?;
        built(p)
    }
}

impl Default for BrowserWallet {
    fn default() -> Self {
        Self::new()
    }
}

fn built(p: confidant_core::Prepared) -> Result<String, JsValue> {
    let bundle_hash = p.hash_hex();
    serde_json::to_string(&serde_json::json!({ "record": p.record, "bundleHash": bundle_hash }))
        .map_err(js)
}
