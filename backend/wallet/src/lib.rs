//! # confidant-wallet
//!
//! The **demo wallet service** behind the UI prototype: HTTP over [`confidant_core::Wallet`].
//! It creates wallets, syncs them from a coordinator's public log and builds, proves and signs
//! bundles for the UI to submit. It holds the demo wallets' **spending keys** in its data
//! directory (0600), so wherever it runs is where the keys are: on the demo host it is a
//! custodial convenience for the hackathon; in production the same service runs on the holder's
//! own machine (the honest label in `README.md`). **Testnet demo only:** this service must never be
//! the path for a real-value pool; the browser (WASM) wallet is the production path and this service
//! does not graduate with it. It never talks to XRPL and never submits
//! anything: the browser sends the deposit from the user's own XRPL account and posts the bundle
//! to the coordinator.
//!
//! - `POST /wallets` → `{id, address, viewingKey, balance, synced}`; `id` is a random 128-bit
//!   bearer secret (whoever knows it controls the wallet).
//! - `GET /wallets/{id}` / `POST /wallets/{id}/sync` → the same summary after syncing.
//! - `POST /wallets/{id}/shield {amount}`, `/transfer {recipient, amount}`,
//!   `/unshield {amount, destination}` → `{record, bundleHash}` (synced first).

pub mod http;
pub mod rpc;
pub mod store;

pub use rpc::HttpLog;
pub use store::{Built, LogSource, Store, StoreError, WalletSummary};
