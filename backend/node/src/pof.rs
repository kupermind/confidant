//! Proof-of-funds verification (hackathon feature C, Tier 1, the read side). A holder's proof is
//! checked by this node against its **own** state and a lender's challenge, read-only: the record is
//! never applied, persisted or logged. The challenge binds the borrower's XRPL account, the
//! threshold, a recent validated ledger index, an expiry ledger and the lender's id, so a proof
//! cannot be replayed for another account, amount, lender or time. Honest label, on every surface:
//! "someone controlling ≥ X answered this challenge as of ledger L" — an underwriting signal, a
//! snapshot, not a lock, and not proof that the borrower owns the notes.
use serde::{Deserialize, Serialize};

use crate::address::canonical;
use crate::credential::LSF_ACCEPTED;

/// Domain prefix of the challenge bytes; the wallet builds the same string.
pub const CHALLENGE_PREFIX: &str = "confidant/pof/v1";
/// How far behind the validated ledger a challenge's ledger index may be (≈ 13 min at 4 s/ledger).
pub const RECENT_WINDOW: u64 = 200;
/// How far past its ledger index a challenge's expiry may lie (≈ 24 h).
pub const MAX_EXPIRY_WINDOW: u64 = 21_600;
/// The lender id is opaque but bounded.
pub const MAX_LENDER_LEN: usize = 64;
/// The XLS-70 credential type a verified proof is recorded under (Tier 1 signing path), and its hex.
pub const POF_CREDENTIAL_TYPE_TEXT: &str = "ConfidantPoF";
pub const POF_CREDENTIAL_TYPE: &str = "436F6E666964616E74506F46";
/// Seconds per ledger assumed when the challenge's expiry ledger becomes the Credential's `Expiration`.
pub const SECONDS_PER_LEDGER: u64 = 4;
/// BLAKE2b personalization of the challenge digest inside the Credential's URI.
pub const URI_PERSONALIZATION: &[u8; 16] = b"ConfidantPoFURI_";

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

/// The XLS-70 Credential a verified proof may be recorded as (Tier 1 signing path): every field is
/// this node's own derivation from the challenge it verified, so a sidecar can pin the coordinator's
/// `CredentialCreate` against it field by field. `Expiration` is the close time of ledger `L` (read
/// from the ledger, immutable) plus `SECONDS_PER_LEDGER` per ledger up to `E` — identical on every
/// node. The URI is UTF-8 text with the threshold, the ledger window and a short digest of the
/// challenge bytes (binds the lender without naming it); the lender recomputes the digest from the
/// package it holds.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PofCredential {
    pub subject: String,
    pub credential_type: String,
    pub credential_type_text: String,
    /// Seconds since the Ripple epoch.
    pub expiration: u64,
    /// Hex of `uri_text`, as the ledger stores it.
    pub uri: String,
    pub uri_text: String,
}

impl PofChallenge {
    /// The Credential fields for a verified proof of `amount`, given the close time of ledger
    /// `self.ledger_index`. Call after `validate()` (expiry > ledger index).
    pub fn credential(&self, amount: u64, close_time_of_ledger_index: u64) -> PofCredential {
        let digest = blake2b_simd::Params::new()
            .hash_length(8)
            .personal(URI_PERSONALIZATION)
            .hash(&self.bytes(amount));
        let uri_text = format!(
            "{CHALLENGE_PREFIX}|{amount}|{}|{}|{}",
            self.ledger_index,
            self.expiry,
            hex::encode(digest.as_bytes())
        );
        PofCredential {
            subject: self.borrower.clone(),
            credential_type: POF_CREDENTIAL_TYPE.to_owned(),
            credential_type_text: POF_CREDENTIAL_TYPE_TEXT.to_owned(),
            expiration: close_time_of_ledger_index
                + (self.expiry - self.ledger_index) * SECONDS_PER_LEDGER,
            uri: hex::encode_upper(uri_text.as_bytes()),
            uri_text,
        }
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
    /// The Credential this verdict authorizes a sidecar to sign (Tier 1 signing path) — only when
    /// the request asked for it (`credential: true`) and the node's bounds allow it: the borrower
    /// is eligible under the pool's credential gate (when on), holds no proof-of-funds credential
    /// from this pool yet, and the pool's outstanding (unaccepted) proof-of-funds credentials are
    /// under the cap. Otherwise `None`, with the reason in `credential_refused`.
    pub credential: Option<PofCredential>,
    pub credential_refused: Option<String>,
}

/// Most proof-of-funds credentials the pool account may have outstanding (issued, not yet accepted
/// by their subjects) before the node stops deriving new ones. Each one locks an owner reserve on
/// the pool account and cost a multisig fee; the cap bounds what an unauthenticated caller can make
/// the quorum spend and lock (the review's finding on an attacker-chosen `Subject`).
pub const POF_CREDENTIAL_CAP: usize = 8;
/// Counts the pool's unaccepted proof-of-funds credentials among `objects` (the pool account's
/// `account_objects` of type `credential`): issued by the pool, of the PoF type, accepted flag clear.
pub fn outstanding_pof_credentials(pool: &str, objects: &[serde_json::Value]) -> usize {
    objects
        .iter()
        .filter(|o| is_pof_credential_from(pool, o))
        .filter(|o| {
            o.get("Flags")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                & LSF_ACCEPTED
                == 0
        })
        .count()
}

/// Whether `o` is a proof-of-funds credential issued by `pool` (accepted or not).
pub fn is_pof_credential_from(pool: &str, o: &serde_json::Value) -> bool {
    let s = |k: &str| o.get(k).and_then(serde_json::Value::as_str).unwrap_or("");
    s("Issuer") == pool && s("CredentialType").eq_ignore_ascii_case(POF_CREDENTIAL_TYPE)
}
