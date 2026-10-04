//! Serde-ready public types. Field names are camelCase so the API layer can return them as-is
//! for the endpoints in `SPEC.md` (`/shield`, `/transfer`, `/unshield`, `/audit/reveal`,
//! `/pool/state`).

use serde::{Deserialize, Serialize};

use crate::sighash::OpTag;

/// What the API layer learns about a registered owner.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerInfo {
    pub owner: String,
    /// Raw Orchard payment address (43 bytes, hex).
    pub address: String,
    /// The owner's full viewing key (96 bytes, hex) — the "auditor key" for this holder.
    pub viewing_key: String,
}

/// Result of `POST /shield`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShieldReceipt {
    /// `cmx` of the minted note.
    pub commitment: String,
    pub leaf_index: u64,
    pub new_root: String,
    pub bundle_index: usize,
    pub proof: String,
}

/// One recipient of a private transfer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferOutput {
    /// Registered owner id of the recipient.
    pub recipient: String,
    pub amount: u64,
}

/// Result of `POST /transfer`. Nothing in here identifies sender, recipients or amounts.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferReceipt {
    /// One per action (real and dummy spends alike); SPEC's single `nullifier` generalised.
    pub nullifiers: Vec<String>,
    /// `cmx` of every action's output, in leaf order (real and dummy alike).
    pub new_commitments: Vec<String>,
    pub new_root: String,
    pub bundle_index: usize,
    pub proof: String,
}

/// The XRPL Payment the front-end must now build and submit from the pool account.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Payout {
    pub amount: u64,
    pub destination: String,
}

/// Result of `POST /unshield`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnshieldReceipt {
    pub nullifiers: Vec<String>,
    pub new_commitments: Vec<String>,
    pub new_root: String,
    pub bundle_index: usize,
    pub proof: String,
    pub payout: Payout,
}

/// Result of `GET /pool/state`. Contains no plaintext.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolState {
    pub root: String,
    /// The last root the front-end confirmed it wrote into an XRPL memo, if any.
    pub anchored_root: Option<String>,
    pub leaf_count: u64,
    pub nullifier_count: usize,
    pub bundle_count: usize,
}

/// Direction of a revealed note relative to the holder whose key was used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// A note the holder received (payments to them, and their own change).
    Incoming,
    /// A note the holder sent to someone else, recovered through their outgoing viewing key.
    Outgoing,
}

/// One note the auditor key could decrypt.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealedNote {
    pub direction: Direction,
    pub amount: u64,
    pub leaf_index: u64,
    pub bundle_index: usize,
    /// Unix seconds at which the pool accepted the bundle.
    pub timestamp: u64,
    /// For incoming notes: whether the holder has already spent it. Always `false` for outgoing.
    pub spent: bool,
    /// For outgoing notes: the recipient's raw address (hex). `None` for incoming.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
}

/// Result of `POST /audit/reveal` for one holder's viewing key.
///
/// Honest label: a full viewing key reveals the holder's complete activity — receipts, change,
/// which notes were spent, and the recipients and amounts of their outgoing transfers — and
/// nothing about transactions the holder was not a party to.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealReport {
    /// Sum of unspent incoming notes.
    pub balance: u64,
    pub notes: Vec<RevealedNote>,
}

/// Public effects of one action, exactly what Zcash would put on chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRecord {
    pub nullifier: String,
    pub rk: String,
    pub cmx: String,
    pub cv_net: String,
    pub epk: String,
    pub enc_ciphertext: String,
    pub out_ciphertext: String,
    pub spend_auth_sig: String,
}

/// Public effects of one accepted bundle — the unit of the exportable log that a third party
/// re-verifies against the anchored roots. Contains no plaintext and no keys.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleRecord {
    pub index: usize,
    pub op: OpTag,
    pub timestamp: u64,
    /// For a shield: the XRPL deposit tx hash; for an unshield: the XRPL destination;
    /// empty for a transfer. The sighash binds it (see `sighash`).
    pub external: String,
    pub anchor: String,
    pub flags: u8,
    pub value_balance: i64,
    /// Leaf index of this bundle's first action in the commitment tree.
    pub first_leaf: u64,
    pub actions: Vec<ActionRecord>,
    pub proof: String,
    pub binding_signature: String,
}
