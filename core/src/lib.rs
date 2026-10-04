//! # confidant-core
//!
//! The XRPL-agnostic core of **Confidant**, a compliant shielded pool for the XRP Ledger.
//!
//! What lives here: the commitment tree, the nullifier set, building/proving/verifying of the
//! three pool operations (shield, private transfer, unshield) on the **reused Zcash Orchard
//! circuit**, and the auditor reveal. What does *not* live here: anything XRPL (Payments, memo
//! anchoring, custody) and the HTTP layer; both belong to the API/front-end side and call into
//! [`Pool`].
//!
//! Design of record: `docs/audit/REQ-pool-design-2026-10-04.md`.
//!
//! ## Trust model (honest label)
//!
//! The operator running this code holds every owner's spending key, selects notes, proves and
//! signs, and therefore sees every internal plaintext. The proofs are still real: anyone holding
//! the exported bundle log can re-verify every proof and signature against the roots anchored on
//! XRPL. Hidden on ledger: sender, receiver and amount of every internal hop. Public: shield and
//! unshield amounts and their XRPL addresses. Nothing here is "untraceable" or "trustless".

pub mod error;
pub mod keys;
pub mod pool;
pub mod record;
pub mod reveal;
pub mod sighash;
pub mod tree;
pub mod types;

pub use error::Error;
pub use pool::{Accepted, OwnerSecret, Pool, Snapshot};
pub use sighash::OpTag;
pub use types::*;

/// Re-exported so the API layer can name Orchard types without a second dependency line.
pub use orchard;
