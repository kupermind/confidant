//! # confidant-node
//!
//! A **verifier node**: HTTP over [`confidant_core::Verifier`]. One runs next to every signer of the
//! pool account's SignerList and anyone else may run one. It holds **no XRPL key** and signs
//! nothing; it only *reads* the ledger (JSON-RPC) to validate deposits and to rebuild its state
//! from the pool account's anchor memos on startup.
//!
//! - `GET /state`, `GET /log?from=N` — public state and the mirror wallets sync from.
//! - `POST /submit/shield|transfer|unshield` — accept a client-built bundle; for a shield the node
//!   looks the deposit up on the ledger itself.
//! - `POST /endorse` — a signer's whole duty ([`Verifier::endorse`]), idempotent for retries; for a
//!   shield the node re-looks the deposit up itself and ignores the coordinator's facts.
//! - `POST /anchored`, `POST /audit/reveal`.
//!
//! The XRPL *transactions* (anchor memo Payments, payouts) are signed by the Node.js signer
//! sidecar next to this node, only after this node endorsed — see `backend/quorum/`.

pub mod http;
pub mod node;
pub mod rpc;
pub mod xrpl;

pub use node::{AcceptedWithMemo, Node, NodeError};
pub use rpc::JsonRpc;
pub use xrpl::{anchor_memos, deposit_facts, Ledger, LedgerError, MEMO_SHIELD};
