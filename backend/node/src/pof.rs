//! Proof-of-funds verification (hackathon feature C, Tier 1, the read side). A holder's proof is
//! checked by this node against its **own** state and a lender's challenge, read-only: the record is
//! never applied, persisted or logged. The challenge binds the borrower's XRPL account, the
//! threshold, a recent validated ledger index, an expiry ledger and the lender's id, so a proof
//! cannot be replayed for another account, amount, lender or time. Honest label, on every surface:
//! "someone controlling ≥ X answered this challenge as of ledger L" — an underwriting signal, a
//! snapshot, not a lock, and not proof that the borrower owns the notes.
use serde::{Deserialize, Serialize};

use crate::address::canonical;

/// Domain prefix of the challenge bytes; the wallet builds the same string.
pub const CHALLENGE_PREFIX: &str = "confidant/pof/v1";
/// How far behind the validated ledger a challenge's ledger index may be (≈ 13 min at 4 s/ledger).
pub const RECENT_WINDOW: u64 = 200;
/// How far past its ledger index a challenge's expiry may lie (≈ 24 h).
pub const MAX_EXPIRY_WINDOW: u64 = 21_600;
/// The lender id is opaque but bounded.
pub const MAX_LENDER_LEN: usize = 64;

/// The verdict's meaning, verbatim for the lender's surface.
pub const MEANING: &str = "someone controlling at least `amount` in this pool answered this \
    challenge as of the validated ledger: an underwriting signal, a snapshot, not a lock, and \
    not proof that the borrower owns the notes";

/// The challenge a lender hands the holder; its canonical bytes are what the proof is signed over.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PofChallenge {
    /// The borrower's XRPL account, as a classic address (what the holder signs; X-addresses are
    /// refused so both sides sign the same bytes).
    pub borrower: String,
    /// An opaque lender identifier: a name, an account, a request id (1–64 printable characters).
    pub lender: String,
    /// The validated ledger index the lender saw when issuing the challenge.
    pub ledger_index: u64,
    /// The ledger index after which the verdict is stale.
    pub expiry: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum PofError {
    #[error("challenge refused: {0}")]
    Challenge(&'static str),
}

impl PofChallenge {
    /// Field checks that need no ledger: the borrower is a classic address, the lender id is
    /// bounded and printable, the expiry lies after the ledger index and within the window.
    pub fn validate(&self) -> Result<(), PofError> {
        match canonical(&self.borrower) {
            Some(c) if c == self.borrower => {}
            Some(_) => return Err(PofError::Challenge("borrower must be a classic address")),
            None => return Err(PofError::Challenge("borrower is not an XRPL address")),
        }
        if self.lender.is_empty()
            || self.lender.chars().count() > MAX_LENDER_LEN
            || self.lender.chars().any(|c| c.is_control() || c == '|')
        {
            return Err(PofError::Challenge(
                "lender id must be 1–64 printable characters without '|'",
            ));
        }
        if self.expiry <= self.ledger_index {
            return Err(PofError::Challenge(
                "expiry must lie after the ledger index",
            ));
        }
        if self.expiry - self.ledger_index > MAX_EXPIRY_WINDOW {
            return Err(PofError::Challenge("expiry window too long"));
        }
        Ok(())
    }

    /// Freshness against the validated ledger this node read itself.
    pub fn check_window(&self, validated: u64) -> Result<(), PofError> {
        if self.ledger_index > validated {
            return Err(PofError::Challenge(
                "ledger index is ahead of the validated ledger",
            ));
        }
        if validated - self.ledger_index > RECENT_WINDOW {
            return Err(PofError::Challenge("ledger index is not recent"));
        }
        if self.expiry <= validated {
            return Err(PofError::Challenge("expired"));
        }
        Ok(())
    }

    /// The bytes the proof is signed over: `prefix ‖ borrower ‖ amount ‖ ledger ‖ expiry ‖ lender`,
    /// `|`-separated. The wallet builds exactly this string from the same fields.
    pub fn bytes(&self, amount: u64) -> Vec<u8> {
        format!(
            "{CHALLENGE_PREFIX}|{}|{amount}|{}|{}|{}",
            self.borrower, self.ledger_index, self.expiry, self.lender
        )
        .into_bytes()
    }
}

/// What `POST /pof/verify` answers on success. Nothing of the proof itself is echoed beyond its
/// hash; the node keeps no copy.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PofVerdict {
    pub ok: bool,
    pub amount: u64,
    pub challenge: PofChallenge,
    /// The validated ledger index this node judged the window against.
    pub validated_ledger: u64,
    /// This node's current root (the state the proof was checked against).
    pub root: String,
    pub bundle_hash: String,
    pub meaning: String,
}
