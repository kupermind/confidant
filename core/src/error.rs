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
    #[error("snapshot is malformed: {0}")]
    Snapshot(String),
    #[error("commitment tree error: {0}")]
    Tree(String),
    #[error("no witness for leaf position {0}")]
    NoWitness(u64),
    #[error("invalid key or hex encoding")]
    InvalidEncoding,
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
