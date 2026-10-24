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
//! Protocol fee (optional, `--fee-shield-bps`, `--fee-unshield-bps`, `--fee-treasury`): a share of
//! every shield (credited net to the note) and unshield (paid net) accrues in the pool account;
//! `GET /fees` reports it, `POST /fees/swept {txHash}` records a treasury sweep the node verified
//! on the ledger. Transfers carry no fee.
//!
//! Boundary screening (optional, `--screening-list`, `--screening-standby-ledgers`): every node
//! refuses, on its own list and fail-closed, a shield whose depositor is listed or whose deposit is
//! younger than the standby, and an unshield whose destination is listed (HTTP 403).
//!
//! On startup the node refuses to serve unless the pool account's on-ledger custody is the
//! configured k-of-N SignerList with the master key disabled, and (with `--rebuild`) unless the
//! `account_tx` scan starts at the pool's creation and every anchor memo is backed by the mirror
//! and, for shields, by a deposit the node reads itself.
//!
//! The XRPL *transactions* (anchor memo Payments, payouts) are signed by the Node.js signer
//! sidecar next to this node, only after this node endorsed — see `backend/quorum/`.

pub mod address;
pub mod credential;
pub mod fees;
pub mod http;
pub mod node;
pub mod pof;
pub mod rpc;
pub mod screening;
pub mod xrpl;

pub use address::canonical;
pub use credential::{
    is_entry_not_found, ripple_now, CredentialError, CredentialGate, CredentialPolicy, LSF_ACCEPTED,
};
pub use fees::{FeeLedger, FeePolicy, FeeReport, MAX_BPS};
pub use node::{AcceptedWithMemo, Node, NodeError, POF_PERMITS};
pub use pof::{
    PofChallenge, PofCredential, PofError, PofVerdict, CHALLENGE_PREFIX, MAX_EXPIRY_WINDOW,
    MAX_LENDER_LEN, MEANING, POF_CREDENTIAL_TYPE, POF_CREDENTIAL_TYPE_TEXT, RECENT_WINDOW,
    SECONDS_PER_LEDGER,
};
pub use rpc::JsonRpc;
pub use screening::{ScreenError, Screening};
pub use xrpl::{
    account_balance, anchor_fact, anchor_memos, check_custody_info, deposit_facts, deposit_source,
    ensure_full_history, sweep_facts, sweeps_in, Ledger, LedgerError, LSF_DISABLE_MASTER,
    MEMO_SHIELD,
};
