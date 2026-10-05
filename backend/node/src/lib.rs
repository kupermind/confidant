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
//! - `POST /anchored {root, anchorTxHash}` — the node reads the anchor Payment from the ledger and
//!   checks its memo carries the root before marking it anchored; `POST /audit/reveal`.
//!
//! On startup the node refuses to serve unless the pool account's on-ledger custody is the
//! configured k-of-N SignerList with the master key disabled, and (with `--rebuild`) unless the
//! `account_tx` scan starts at the pool's creation and every anchor memo is backed by the mirror
//! and, for shields, by a deposit the node reads itself.
//!
//! The XRPL *transactions* (anchor memo Payments, payouts) are signed by the Node.js signer
//! sidecar next to this node, only after this node endorsed — see `backend/quorum/`.

pub mod http;
pub mod node;
pub mod rpc;
pub mod xrpl;

pub use node::{AcceptedWithMemo, Node, NodeError};
pub use rpc::JsonRpc;
pub use xrpl::{
    anchor_fact, anchor_memos, check_custody_info, deposit_facts, ensure_full_history, Ledger,
    LedgerError, LSF_DISABLE_MASTER, MEMO_SHIELD,
};
