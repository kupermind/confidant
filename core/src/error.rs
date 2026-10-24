//! Error type for every fallible core operation.

use crate::wallet::MAX_OUTPUTS;

/// Everything the wallet or the verifier can refuse to do, and why.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("amount must be positive")]
    ZeroAmount,
    /// A proof-of-funds record reached an apply path (submit, endorse, replay, memo): refused
    /// before any signature check. A proof of funds is checked read-only and never applied.
    #[error("not a pool operation: a proof of funds is checked read-only, never applied")]
    NotAPoolOperation,
    /// A pool record was handed to the proof-of-funds check.
    #[error("not a proof of funds: the record is a pool operation")]
    NotAProofOfFunds,
    #[error("insufficient shielded balance: available {available}, requested {requested}")]
    InsufficientFunds { available: u64, requested: u64 },
    #[error("too many outputs: {0} (max {MAX_OUTPUTS})")]
    TooManyOutputs(usize),
    #[error("amounts overflow u64")]
    AmountOverflow,
    #[error("a {op:?} bundle has no external amount, but {amount} was vouched")]
    VouchedAmountNotApplicable {
        op: crate::sighash::OpTag,
        amount: u64,
    },
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
    #[error("deposit `{0}` already backs a shield")]
    DepositConsumed(String),
    #[error("deposit memo does not commit to this bundle")]
    DepositBindingMismatch,
    #[error("shield needs a deposit reference")]
    DepositRequired,
    #[error("anchor memo is malformed: {0}")]
    MemoMalformed(&'static str),
    #[error("anchor memo would be {0} bytes, over the 1 KB limit")]
    MemoTooLarge(usize),
    #[error("anchor memo does not match this verifier's record {index}: {what}")]
    MemoMismatch { index: usize, what: &'static str },
    #[error("mirror has no bundle with hash {0}")]
    MirrorMissing(String),
    #[error("bundle hash does not match the anchor memo")]
    BundleHashMismatch,
    #[error("log is not an append-only extension of what the wallet already synced")]
    LogRewound,
    #[error("record index {found} where {expected} was expected")]
    RecordOutOfOrder { expected: usize, found: usize },
    #[error("commitment tree error: {0}")]
    Tree(String),
    #[error("no witness for leaf position {0}")]
    NoWitness(u64),
    #[error("invalid key or hex encoding")]
    InvalidEncoding,
    #[error("snapshot is malformed: {0}")]
    Snapshot(String),
    #[error("internal invariant violated: {0}")]
    Internal(&'static str),
}

impl From<orchard::builder::BuildError> for Error {
    fn from(e: orchard::builder::BuildError) -> Self {
        Error::Build(e.to_string())
    }
}

impl From<orchard::builder::SpendError> for Error {
    fn from(e: orchard::builder::SpendError) -> Self {
        Error::Build(e.to_string())
    }
}

impl From<orchard::builder::OutputError> for Error {
    fn from(e: orchard::builder::OutputError) -> Self {
        Error::Build(e.to_string())
    }
}

impl From<orchard::bundle::CommitmentError> for Error {
    fn from(e: orchard::bundle::CommitmentError) -> Self {
        Error::Build(e.to_string())
    }
}

impl From<shardtree::error::ShardTreeError<std::convert::Infallible>> for Error {
    fn from(e: shardtree::error::ShardTreeError<std::convert::Infallible>) -> Self {
        Error::Tree(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use incrementalmerkletree::Address;
    use orchard::builder::{BuildError, OutputError, SpendError};
    use orchard::bundle::CommitmentError;
    use shardtree::error::{InsertionError, ShardTreeError};

    #[test]
    fn library_errors_convert_with_their_messages() {
        let e: Error = BuildError::SpendsDisabled.into();
        assert!(matches!(&e, Error::Build(m) if !m.is_empty()));
        let e: Error = SpendError::FvkMismatch.into();
        assert!(matches!(&e, Error::Build(m) if !m.is_empty()));
        let e: Error = OutputError::OutputsDisabled.into();
        assert!(matches!(&e, Error::Build(m) if !m.is_empty()));
        let e: Error = CommitmentError::InvalidTransactionVersion.into();
        assert!(matches!(&e, Error::Build(m) if !m.is_empty()));
        let tree_err: ShardTreeError<std::convert::Infallible> =
            InsertionError::NotContained(Address::from_parts(0.into(), 0)).into();
        let e: Error = tree_err.into();
        assert!(matches!(&e, Error::Tree(m) if !m.is_empty()));
        assert_eq!(
            Error::Internal("x").to_string(),
            "internal invariant violated: x"
        );
        assert_eq!(
            Error::NoWitness(7).to_string(),
            "no witness for leaf position 7"
        );
    }
}
