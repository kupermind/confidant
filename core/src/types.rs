//! Serde-ready public types. Field names are camelCase so the API layer can return them as-is
//! for the v2 endpoints in `SPEC.md` (`/log`, `/submit/*`, `/audit/reveal`).

use serde::{Deserialize, Serialize};

use crate::sighash::OpTag;

/// What a wallet publishes about itself: where to pay it and the key an auditor may be given.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletInfo {
    /// Raw Orchard payment address (43 bytes, hex).
    pub address: String,
    /// The holder's full viewing key (96 bytes, hex) — the "auditor key" for this holder.
    pub viewing_key: String,
}

/// One recipient of a private transfer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferOutput {
    /// Recipient's raw Orchard address (43 bytes, hex).
    pub recipient: String,
    pub amount: u64,
}

/// What the API layer hands a verifier about a deposit it found on XRPL. The layer is
/// responsible for the ledger facts (the tx is validated, is a Payment, its destination is the
/// pool account); the core enforces amount, binding and single use.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositRef {
    /// XRPL transaction hash of the deposit Payment.
    pub tx_hash: String,
    /// `meta.delivered_amount` in the pool's token units (drops for XRP).
    pub delivered: u64,
    /// The deposit memo (`confidant/shield`): hex of the shield bundle's hash.
    pub memo_hash: String,
}

/// Result of accepting a bundle (`POST /submit/*`). Nothing in here identifies sender,
/// recipients or amounts beyond what the ledger shows anyway.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    pub bundle_index: usize,
    pub first_leaf: u64,
    pub new_root: String,
    /// Hash of the accepted bundle (hex) — what the anchor memo carries.
    pub bundle_hash: String,
    /// One per action (real and dummy alike).
    pub nullifiers: Vec<String>,
    /// `cmx` of every action's output, in leaf order (real and dummy alike).
    pub commitments: Vec<String>,
    /// Present for unshields: the Payment the quorum must now make from the pool account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payout: Option<Payout>,
}

/// The XRPL Payment the quorum makes from the pool account after an accepted unshield.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Payout {
    pub amount: u64,
    pub destination: String,
}

/// Public verifier state (`GET /log` header, `GET /pool/state`). Contains no plaintext.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolState {
    pub root: String,
    /// The last root confirmed as written into an XRPL memo, if any.
    pub anchored_root: Option<String>,
    pub leaf_count: u64,
    pub nullifier_count: usize,
    pub bundle_count: usize,
    pub consumed_deposits: usize,
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

/// One note the viewing key could decrypt.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealedNote {
    pub direction: Direction,
    pub amount: u64,
    pub leaf_index: u64,
    pub bundle_index: usize,
    /// Unix seconds at which this verifier accepted the bundle.
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// Public effects of one bundle — what a wallet submits and what the verifier logs. Contains no
/// plaintext and no keys. `index`, `timestamp` and `first_leaf` are verifier metadata, set on
/// acceptance; a wallet leaves them at zero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleRecord {
    pub index: usize,
    pub op: OpTag,
    pub timestamp: u64,
    /// Shield: the XRPL deposit tx hash (set by the verifier on acceptance). Unshield: the XRPL
    /// destination. Transfer: empty. The anchor memo carries it; the sighash binds it for unshields.
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
