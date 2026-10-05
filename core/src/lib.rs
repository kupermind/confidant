//! # confidant-core
//!
//! The XRPL-agnostic core of **Confidant**, a compliant shielded pool for the XRP Ledger.
//!
//! v2 (trust-minimized) layout — see `SPEC.md` §v2:
//!
//! - [`Wallet`] — the holder's side: spending key, own notes tracked by scanning the public bundle
//!   log, a replica of the commitment tree for witnesses, and build / prove / sign of the three
//!   operations. Keys and plaintext never leave it.
//! - [`Verifier`] — any party's side: commitment tree, nullifier set, consumed-deposit set, public
//!   bundle log, the verify-and-apply tail, the per-op anchor memo, and rebuild from memos plus a
//!   mirror. Holds no keys; its state is public.
//!
//! Everything XRPL (deposit Payments and their validation, memo anchoring, payouts, the SignerList
//! quorum) and the HTTP layer live outside this crate and call into these two types.
//!
//! ## Trust model (honest label)
//!
//! Notes are non-custodial: only the wallet can spend them or read them. The pool account and
//! proof verification are federated: a k-of-N quorum of verifiers re-checks every proof and every
//! deposit and signs only a root it computed itself ([`Verifier::endorse`]). **Collusion of k
//! signers can drain the pool account**; it cannot spend anyone's notes. k-of-N because today's
//! XRPL has no on-ledger Halo 2 verifier; an amendment adding one upgrades this to on-ledger
//! verification. Hidden on ledger: sender, receiver and amount of every internal hop. Public:
//! shield and unshield amounts and addresses. Nothing here is "untraceable" or "trustless".

pub mod error;
pub mod keys;
pub mod memo;
pub mod record;
pub mod reveal;
pub mod sighash;
pub mod tree;
pub mod types;
pub mod verifier;
pub mod wallet;

pub use error::Error;
pub use memo::AnchorMemo;
pub use sighash::OpTag;
pub use types::*;
pub use verifier::{
    DepositLookup, Mirror, Params, Submission, Verifier, VerifierSnapshot, BUNDLE_VERSION,
};
pub use wallet::{Prepared, Wallet, WalletSecret};

/// Re-exported so the API layer can name Orchard types without a second dependency line.
pub use orchard;
