//! Error type for every fallible core operation.

use crate::pool::MAX_OUTPUTS;

/// Everything the pool can refuse to do, and why.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown owner `{0}`")]
    UnknownOwner(String),
    #[error("owner `{0}` is already registered")]
    OwnerExists(String),
    #[error("amount must be positive")]
    ZeroAmount,
    #[error("insufficient shielded balance: available {available}, requested {requested}")]
    InsufficientFunds { available: u64, requested: u64 },
    #[error("too many outputs: {0} (max {MAX_OUTPUTS})")]
    TooManyOutputs(usize),
    #[error("bundle construction failed: {0}")]
    Build(String),
    #[error("proof does not verify")]
    InvalidProof,
    #[error("spend authorization signature does not verify")]
    InvalidSpendAuth,
    #[error("binding signature does not verify")]
    InvalidBindingSignature,
    #[error("bundle anchor is not a known pool root")]
    UnknownAnchor,
    #[error("nullifier already spent")]
    DoubleSpend,
    #[error("value balance mismatch: expected {expected}, got {actual}")]
    ValueBalance { expected: i64, actual: i64 },
    #[error("value balance {actual} has the wrong sign for a {op:?} bundle")]
    ValueBalanceSign {
        op: crate::sighash::OpTag,
        actual: i64,
    },
    #[error("snapshot is malformed: {0}")]
    Snapshot(String),
    #[error("commitment tree error: {0}")]
    Tree(String),
    #[error("invalid key or hex encoding")]
    InvalidEncoding,
}
