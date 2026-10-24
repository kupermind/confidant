//! The verifier: public state, the verify-and-apply tail, deposit binding, the anchor memo and
//! rebuild from the ledger. Any party can run one; it holds no keys.
//!
//! See the crate docs for the trust model and `SPEC.md` §v2 for the design of record.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use orchard::bundle::{Authorized, BundleVersion, TxVersion};
use orchard::circuit::{ProvingKey, VerifyingKey};
use orchard::tree::{MerkleHashOrchard, MerklePath};
use orchard::{Anchor, Bundle};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::keys::fvk_from_hex;
use crate::memo::AnchorMemo;
use crate::record::{bundle_from_record, bundle_hash, record_from_bundle};
use crate::sighash::{external_ctx, pof_ctx, pof_sighash, sighash, OpTag};
use crate::tree::CommitmentTree;
use crate::types::*;

/// A source of bundle records by hash (any verifier's `GET /log`, a file, a cache).
pub type Mirror<'a> = &'a mut dyn FnMut(&[u8; 32]) -> Option<BundleRecord>;
/// A source of ledger facts about a deposit, by XRPL tx hash (the XRPL side's `tx` lookup).
pub type DepositLookup<'a> = &'a mut dyn FnMut(&str) -> Option<DepositRef>;

/// The deposit memo must commit to exactly this bundle: `memo_hash == BLAKE2b(bundle)`.
fn check_deposit_binding(record: &BundleRecord, deposit: &DepositRef) -> Result<(), Error> {
    let memo: [u8; 32] = hex::decode(deposit.memo_hash.trim())
        .map_err(|_| Error::InvalidEncoding)?
        .try_into()
        .map_err(|_| Error::InvalidEncoding)?;
    if bundle_hash(record)? != memo {
        return Err(Error::DepositBindingMismatch);
    }
    Ok(())
}

/// The reused circuit, fixed for this pool: Zcash Orchard under protocol version 2
/// (`OrchardCircuitVersion::FixedPostNu6_2`), V2 notes, cross-address transfers permitted.
pub const BUNDLE_VERSION: BundleVersion = BundleVersion::orchard_v2();

/// Proving and verifying keys, each built once per process from the circuit itself — lazily, so a
/// wallet never pays for the verifying key and a verifier never pays for the proving key (the
/// browser wallet builds its proving key in the background right after the wallet exists).
pub struct Params {
    pub version: BundleVersion,
    pk: OnceLock<ProvingKey>,
    vk: OnceLock<VerifyingKey>,
}

impl Params {
    /// The process-wide parameters (cheap; each key builds on its first use).
    pub fn get() -> Arc<Params> {
        static PARAMS: OnceLock<Arc<Params>> = OnceLock::new();
        PARAMS
            .get_or_init(|| {
                Arc::new(Params {
                    version: BUNDLE_VERSION,
                    pk: OnceLock::new(),
                    vk: OnceLock::new(),
                })
            })
            .clone()
    }

    /// The proving key; the first call builds it (seconds natively, ≈ 25 s single-threaded in a
    /// browser).
    pub fn pk(&self) -> &ProvingKey {
        self.pk
            .get_or_init(|| ProvingKey::build(self.version.circuit_version()))
    }

    /// The verifying key; the first call builds it.
    pub fn vk(&self) -> &VerifyingKey {
        self.vk
            .get_or_init(|| VerifyingKey::build(self.version.circuit_version()))
    }
}

/// What a coordinator hands a signer (and what a client hands any verifier): one of the three
/// operations with everything needed to validate it independently.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Submission {
    Shield {
        record: BundleRecord,
        deposit: DepositRef,
    },
    Transfer {
        record: BundleRecord,
    },
    Unshield {
        record: BundleRecord,
        destination: String,
    },
}

/// Public, key-free persisted state. Safe to publish; `restore` replays it through the tail.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifierSnapshot {
    pub format: u32,
    pub anchored_root: Option<String>,
    pub records: Vec<BundleRecord>,
}

pub struct Verifier {
    params: Arc<Params>,
    tree: CommitmentTree,
    nullifiers: HashSet<[u8; 32]>,
    consumed_deposits: HashSet<String>,
    bundles: Vec<Bundle<Authorized, i64>>,
    records: Vec<BundleRecord>,
    anchored_root: Option<[u8; 32]>,
}

impl Default for Verifier {
    /// An empty verifier. Builds the circuit parameters on first use in the process.
    fn default() -> Self {
        Self {
            params: Params::get(),
            tree: CommitmentTree::new(),
            nullifiers: HashSet::new(),
            consumed_deposits: HashSet::new(),
            bundles: Vec::new(),
            records: Vec::new(),
            anchored_root: None,
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Verifier {
    /// An empty verifier (same as `Default`).
    pub fn new() -> Self {
        Self::default()
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    // ----- public state --------------------------------------------------------------------

    pub fn state(&self) -> PoolState {
        PoolState {
            root: hex::encode(self.tree.current_anchor().to_bytes()),
            anchored_root: self.anchored_root.map(hex::encode),
            leaf_count: self.tree.leaf_count(),
            nullifier_count: self.nullifiers.len(),
            bundle_count: self.bundles.len(),
            consumed_deposits: self.consumed_deposits.len(),
        }
    }

    /// The public log, oldest first — what `GET /log` serves and what wallets sync from.
    pub fn records(&self) -> &[BundleRecord] {
        &self.records
    }

    pub fn bundle(&self, index: usize) -> Option<&Bundle<Authorized, i64>> {
        self.bundles.get(index)
    }

    /// Every root the pool has had, oldest first (hex).
    pub fn anchor_history(&self) -> Vec<String> {
        self.tree.anchors().iter().map(hex::encode).collect()
    }

    /// The root new bundles should be built against.
    pub fn current_anchor(&self) -> Anchor {
        self.tree.current_anchor()
    }

    /// Merkle witness for a leaf under the current root (wallets use this on their replica).
    pub fn witness(&self, position: u64) -> Result<MerklePath, Error> {
        self.tree.witness(position)
    }

    pub fn is_spent(&self, nullifier: &[u8; 32]) -> bool {
        self.nullifiers.contains(nullifier)
    }

    pub fn is_deposit_consumed(&self, tx_hash: &str) -> bool {
        self.consumed_deposits.contains(tx_hash)
    }

    /// The XRPL side reports which root it wrote into a memo. Must be a root this pool has had.
    pub fn set_anchored_root(&mut self, root_hex: &str) -> Result<(), Error> {
        let bytes: [u8; 32] = hex::decode(root_hex)
            .map_err(|_| Error::InvalidEncoding)?
            .try_into()
            .map_err(|_| Error::InvalidEncoding)?;
        let anchor = Anchor::from_bytes(bytes)
            .into_option()
            .ok_or(Error::InvalidEncoding)?;
        if !self.tree.is_known_anchor(&anchor) {
            return Err(Error::UnknownAnchor);
        }
        self.anchored_root = Some(bytes);
        Ok(())
    }

    // ----- the common tail -----------------------------------------------------------------

    /// Pure verification of a bundle against this verifier's current state. Returns the sighash
    /// the signatures were checked against. Mutates nothing.
    ///
    /// Order, cheapest first: value-balance sign per op → vouched amount == value balance →
    /// anchor ∈ history → no nullifier spent or repeated → proof → spend-auth signatures →
    /// binding signature. Replays and double-spends are refused before the SNARK is verified.
    ///
    /// `external` is the XRPL destination for an unshield; it is ignored by the sighash for
    /// shields and transfers. `expected_amount`, when given, must equal the deposit (shield) or
    /// payout (unshield) the caller vouches for; the payout amount bound into the sighash is
    /// always taken from the bundle's own value balance.
    pub fn verify(
        &self,
        bundle: &Bundle<Authorized, i64>,
        op: OpTag,
        external: &str,
        expected_amount: Option<u64>,
    ) -> Result<[u8; 32], Error> {
        // A proof of funds is never a pool operation: refused here, before the sign check and
        // before any signature, on every apply path (submit, endorse, replay). It is checked
        // read-only by `verify_proof_of_funds` and never applied.
        if op == OpTag::ProofOfFunds {
            return Err(Error::NotAPoolOperation);
        }
        let vb = *bundle.value_balance();
        let sign_ok = match op {
            OpTag::Shield => vb < 0,
            OpTag::Transfer => vb == 0,
            OpTag::Unshield | OpTag::ProofOfFunds => vb > 0,
        };
        if !sign_ok {
            return Err(Error::ValueBalanceSign { op, actual: vb });
        }
        if let Some(amount) = expected_amount {
            let expected = match op {
                OpTag::Shield => -(amount as i64),
                OpTag::Transfer if amount != 0 => {
                    return Err(Error::VouchedAmountNotApplicable { op, amount });
                }
                OpTag::Transfer => 0,
                OpTag::Unshield | OpTag::ProofOfFunds => amount as i64,
            };
            if vb != expected {
                return Err(Error::ValueBalance {
                    expected,
                    actual: vb,
                });
            }
        }
        if !self.tree.is_known_anchor(bundle.anchor()) {
            return Err(Error::UnknownAnchor);
        }
        // Nullifiers before the proof: a replay costs a set lookup, not a SNARK verification.
        // This makes DoubleSpend vs InvalidProof an oracle for "is this nullifier spent?" that
        // needs no valid proof. It leaks nothing because the spent set is public (on-ledger in
        // every anchor memo, and in the exported log). CAVEAT (audit ruling on PR #2): if the
        // nullifier set ever stopped being public, move this block after the signature checks
        // or collapse the two errors at the wire.
        let mut seen = HashSet::with_capacity(bundle.actions().len());
        for action in bundle.actions() {
            let nf = action.nullifier().to_bytes();
            if self.nullifiers.contains(&nf) || !seen.insert(nf) {
                return Err(Error::DoubleSpend);
            }
        }
        bundle
            .verify_proof(self.params.vk())
            .map_err(|_| Error::InvalidProof)?;

        let commitment: [u8; 32] = bundle.commitment(TxVersion::V5)?.into();
        let sh = sighash(&commitment, op, &external_ctx(op, external, vb));
        for action in bundle.actions() {
            action
                .rk()
                .verify(&sh, action.authorization())
                .map_err(|_| Error::InvalidSpendAuth)?;
        }
        bundle
            .binding_validating_key()
            .verify(&sh, bundle.authorization().binding_signature())
            .map_err(|_| Error::InvalidBindingSignature)?;
        Ok(sh)
    }

    /// Decodes a record for an apply path. A proof of funds is refused here by its op tag — before
    /// decoding, before the sign check, before any signature — on every path that could apply or
    /// anchor it (submit, endorse, replay, and the mirror's records during a rebuild).
    fn pool_bundle(&self, record: &BundleRecord) -> Result<Bundle<Authorized, i64>, Error> {
        if record.op == OpTag::ProofOfFunds {
            return Err(Error::NotAPoolOperation);
        }
        bundle_from_record(record, self.params.version)
    }

    /// Read-only check of a proof of funds against this verifier's state (hackathon feature C):
    /// the holder controls unspent notes in this pool worth exactly `amount` (the holder chooses
    /// the threshold; the bundle's value balance is it), answered `challenge`, and signed over the
    /// proof-of-funds domain. Mutates nothing; the bundle is never applied. Honest reading: "someone
    /// controlling ≥ amount answered this challenge" as of this state — a snapshot, not a lock.
    pub fn verify_proof_of_funds(
        &self,
        record: &BundleRecord,
        amount: u64,
        challenge: &[u8],
    ) -> Result<(), Error> {
        if record.op != OpTag::ProofOfFunds {
            return Err(Error::NotAProofOfFunds);
        }
        let bundle = bundle_from_record(record, self.params.version)?;
        let vb = *bundle.value_balance();
        if amount == 0 || vb != amount as i64 {
            return Err(Error::ValueBalance {
                expected: amount as i64,
                actual: vb,
            });
        }
        if !self.tree.is_known_anchor(bundle.anchor()) {
            return Err(Error::UnknownAnchor);
        }
        let mut seen = HashSet::with_capacity(bundle.actions().len());
        for action in bundle.actions() {
            let nf = action.nullifier().to_bytes();
            if self.nullifiers.contains(&nf) || !seen.insert(nf) {
                return Err(Error::DoubleSpend);
            }
        }
        bundle
            .verify_proof(self.params.vk())
            .map_err(|_| Error::InvalidProof)?;
        let commitment: [u8; 32] = bundle.commitment(TxVersion::V5)?.into();
        let sh = pof_sighash(&commitment, &pof_ctx(challenge, amount));
        for action in bundle.actions() {
            action
                .rk()
                .verify(&sh, action.authorization())
                .map_err(|_| Error::InvalidSpendAuth)?;
        }
        bundle
            .binding_validating_key()
            .verify(&sh, bundle.authorization().binding_signature())
            .map_err(|_| Error::InvalidBindingSignature)?;
        Ok(())
    }

    /// Shield-only preconditions on this verifier's own state: a deposit must be named and must
    /// not already back a shield.
    fn check_deposit_state(&self, op: OpTag, external: &str) -> Result<(), Error> {
        if op == OpTag::Shield {
            if external.is_empty() {
                return Err(Error::DepositRequired);
            }
            if self.consumed_deposits.contains(external) {
                return Err(Error::DepositConsumed(external.to_owned()));
            }
        }
        Ok(())
    }

    /// Verifies and, only if every check passes, applies a bundle. Any error leaves the
    /// verifier unchanged. Shields must carry the deposit tx hash as `external`.
    fn accept(
        &mut self,
        bundle: Bundle<Authorized, i64>,
        op: OpTag,
        external: &str,
        expected_amount: Option<u64>,
        timestamp: u64,
    ) -> Result<Accepted, Error> {
        self.check_deposit_state(op, external)?;
        self.verify(&bundle, op, external, expected_amount)?;
        Ok(self.apply_verified(bundle, op, external, timestamp))
    }

    /// Applies a bundle that passed `check_deposit_state` and `verify` against this exact state.
    /// Infallible by construction: the in-memory tree cannot fail after a successful verify.
    fn apply_verified(
        &mut self,
        bundle: Bundle<Authorized, i64>,
        op: OpTag,
        external: &str,
        timestamp: u64,
    ) -> Accepted {
        let nullifiers: Vec<[u8; 32]> = bundle
            .actions()
            .iter()
            .map(|a| a.nullifier().to_bytes())
            .collect();
        let commitments: Vec<[u8; 32]> = bundle
            .actions()
            .iter()
            .map(|a| a.cmx().to_bytes())
            .collect();
        let leaves: Vec<MerkleHashOrchard> = bundle
            .actions()
            .iter()
            .map(|a| MerkleHashOrchard::from_cmx(a.cmx()))
            .collect();

        let first_leaf = self.tree.leaf_count();
        let new_root = self
            .tree
            .append_batch(&leaves)
            .expect("a verified, non-empty bundle appends to an in-memory tree");

        for nf in &nullifiers {
            self.nullifiers.insert(*nf);
        }
        if op == OpTag::Shield {
            self.consumed_deposits.insert(external.to_owned());
        }
        let bundle_index = self.bundles.len();
        let record = record_from_bundle(&bundle, bundle_index, op, external, first_leaf, timestamp);
        let hash = bundle_hash(&record).expect("a record made from a bundle is well-formed");
        let payout = (op == OpTag::Unshield).then(|| Payout {
            amount: *bundle.value_balance() as u64,
            destination: external.to_owned(),
        });
        self.records.push(record);
        self.bundles.push(bundle);
        Accepted {
            bundle_index,
            first_leaf,
            new_root: hex::encode(new_root.to_bytes()),
            bundle_hash: hex::encode(hash),
            nullifiers: nullifiers.iter().map(hex::encode).collect(),
            commitments: commitments.iter().map(hex::encode).collect(),
            payout,
        }
    }

    // ----- submissions (`POST /submit/*`) -------------------------------------------------

    /// Accepts a client-built shield against a deposit the API layer found on XRPL.
    ///
    /// The layer vouches that `deposit.tx_hash` is a validated Payment to the pool account; the
    /// core enforces that the deposit is unconsumed, that its memo commits to exactly this
    /// bundle, and that `delivered == −valueBalance`, then runs the common tail.
    pub fn submit_shield(
        &mut self,
        record: &BundleRecord,
        deposit: &DepositRef,
    ) -> Result<Accepted, Error> {
        check_deposit_binding(record, deposit)?;
        let bundle = self.pool_bundle(record)?;
        self.accept(
            bundle,
            OpTag::Shield,
            &deposit.tx_hash,
            Some(deposit.delivered),
            now(),
        )
    }

    /// A signer's whole duty in one call (no blind signing): validates the coordinator's
    /// `proposed` memo against the submission and against this signer's **own** state, computes
    /// the root the transition would produce here, and only if it equals the proposed root
    /// applies the transition and returns what to sign. Any disagreement refuses and leaves this
    /// signer's state unchanged.
    pub fn endorse(
        &mut self,
        submission: &Submission,
        proposed: &AnchorMemo,
    ) -> Result<Accepted, Error> {
        let index = self.records.len();
        let (record, op, external, expected) = match submission {
            Submission::Shield { record, deposit } => {
                check_deposit_binding(record, deposit)?;
                (
                    record,
                    OpTag::Shield,
                    deposit.tx_hash.as_str(),
                    Some(deposit.delivered),
                )
            }
            Submission::Transfer { record } => (record, OpTag::Transfer, "", None),
            Submission::Unshield {
                record,
                destination,
            } => {
                if destination.is_empty() {
                    return Err(Error::Build("empty destination".into()));
                }
                (record, OpTag::Unshield, destination.as_str(), None)
            }
        };
        // The proposed memo must describe exactly this submission...
        let mismatch = |what: &'static str| Error::MemoMismatch { index, what };
        if proposed.op != op {
            return Err(mismatch("op"));
        }
        if proposed.external != external {
            return Err(mismatch("external context"));
        }
        if proposed.bundle_hash != bundle_hash(record)? {
            return Err(mismatch("bundle hash"));
        }
        let own_memo = AnchorMemo::from_record(record, [0u8; 32])?;
        if proposed.actions != own_memo.actions {
            return Err(mismatch("actions"));
        }
        // ...and pass every check against this signer's own state...
        self.check_deposit_state(op, external)?;
        let bundle = self.pool_bundle(record)?;
        self.verify(&bundle, op, external, expected)?;
        // ...and produce, on this signer's own tree, the root the coordinator claims.
        let leaves: Vec<MerkleHashOrchard> = bundle
            .actions()
            .iter()
            .map(|a| MerkleHashOrchard::from_cmx(a.cmx()))
            .collect();
        if self.tree.root_after(&leaves).to_bytes() != proposed.root {
            return Err(mismatch("root"));
        }
        Ok(self.apply_verified(bundle, op, external, now()))
    }

    /// Accepts a client-built private hop.
    pub fn submit_transfer(&mut self, record: &BundleRecord) -> Result<Accepted, Error> {
        let bundle = self.pool_bundle(record)?;
        self.accept(bundle, OpTag::Transfer, "", None, now())
    }

    /// Accepts a client-built exit. The signatures inside the bundle bind `destination` and the
    /// bundle's own value balance; the returned `payout` is what the quorum must now pay.
    pub fn submit_unshield(
        &mut self,
        record: &BundleRecord,
        destination: &str,
    ) -> Result<Accepted, Error> {
        if destination.is_empty() {
            return Err(Error::Build("empty destination".into()));
        }
        let bundle = self.pool_bundle(record)?;
        self.accept(bundle, OpTag::Unshield, destination, None, now())
    }

    /// Re-applies a record from a trusted log (snapshot restore, wallet sync): same tail, the
    /// record's own op / external / timestamp, and the index and leaf position must line up.
    pub fn replay(&mut self, record: &BundleRecord) -> Result<Accepted, Error> {
        if record.index != self.records.len() {
            return Err(Error::RecordOutOfOrder {
                expected: self.records.len(),
                found: record.index,
            });
        }
        let bundle = self.pool_bundle(record)?;
        let acc = self.accept(bundle, record.op, &record.external, None, record.timestamp)?;
        if acc.first_leaf != record.first_leaf {
            return Err(Error::Snapshot(format!(
                "record {} leaf index mismatch",
                record.index
            )));
        }
        Ok(acc)
    }

    // ----- anchor memos and rebuild from the ledger ---------------------------------------

    /// The anchor memo for accepted bundle `index`: op, root after it, bundle hash, external
    /// context, and every (nullifier, cmx).
    pub fn anchor_memo(&self, index: usize) -> Result<AnchorMemo, Error> {
        let record = self.records.get(index).ok_or(Error::MemoMismatch {
            index,
            what: "no such record",
        })?;
        AnchorMemo::from_record(record, self.tree.anchors()[index + 1])
    }

    /// Checks a memo read back from the ledger against this verifier's own record.
    pub fn check_memo(&self, index: usize, memo: &AnchorMemo) -> Result<(), Error> {
        let ours = self.anchor_memo(index)?;
        let what = if ours.op != memo.op {
            "op"
        } else if ours.root != memo.root {
            "root"
        } else if ours.bundle_hash != memo.bundle_hash {
            "bundle hash"
        } else if ours.external != memo.external {
            "external context"
        } else if ours.actions != memo.actions {
            "actions"
        } else {
            return Ok(());
        };
        Err(Error::MemoMismatch { index, what })
    }

    /// Builds a verifier from the anchor memos read off the ledger and a mirror that serves
    /// bundle records by hash. Each bundle is hash-checked, re-verified and applied; the root it
    /// produces must equal the memo's. Note validity (no forged note, no double-spend) therefore
    /// rests on nobody but this node. **Pool backing is not checked here**: the node trusts the
    /// k-signed anchors on which deposits backed which shields. Use [`Self::rebuild_checked`] to
    /// enforce backing too.
    pub fn rebuild(memos: &[AnchorMemo], mirror: Mirror<'_>) -> Result<Self, Error> {
        Self::rebuild_inner(memos, mirror, None)
    }

    /// [`Self::rebuild`] plus the backing check on every shield: `deposits` supplies the ledger
    /// facts for a deposit tx hash (validated Payment to the pool, `delivered_amount`, memo), and
    /// the core enforces `delivered == −valueBalance` and `memo == bundle hash`. A shield whose
    /// deposit cannot be produced is refused (`DepositRequired`). This is the join path a signer
    /// or any node that wants to confirm pool backing itself must use.
    pub fn rebuild_checked(
        memos: &[AnchorMemo],
        mirror: Mirror<'_>,
        deposits: DepositLookup<'_>,
    ) -> Result<Self, Error> {
        Self::rebuild_inner(memos, mirror, Some(deposits))
    }

    fn rebuild_inner(
        memos: &[AnchorMemo],
        mirror: Mirror<'_>,
        mut deposits: Option<DepositLookup<'_>>,
    ) -> Result<Self, Error> {
        let mut v = Self::new();
        for (index, memo) in memos.iter().enumerate() {
            let record = mirror(&memo.bundle_hash)
                .ok_or_else(|| Error::MirrorMissing(hex::encode(memo.bundle_hash)))?;
            if bundle_hash(&record)? != memo.bundle_hash {
                return Err(Error::BundleHashMismatch);
            }
            // Backing on join: the deposit named by the memo must exist on the ledger, deliver
            // exactly the shielded amount, and commit to this very bundle.
            let mut expected = None;
            if let (OpTag::Shield, Some(lookup)) = (memo.op, deposits.as_deref_mut()) {
                let deposit = lookup(&memo.external).ok_or(Error::DepositRequired)?;
                if deposit.tx_hash != memo.external {
                    return Err(Error::DepositBindingMismatch);
                }
                check_deposit_binding(&record, &deposit)?;
                expected = Some(deposit.delivered);
            }
            // The mirror's record is refused by type here too: a proof of funds under a genuine
            // pool memo must not rest on the signature domain alone (audit condition 2).
            let bundle = v.pool_bundle(&record)?;
            let acc = v.accept(bundle, memo.op, &memo.external, expected, record.timestamp)?;
            if acc.new_root != hex::encode(memo.root) {
                return Err(Error::MemoMismatch {
                    index,
                    what: "root",
                });
            }
            let ours: Vec<([u8; 32], [u8; 32])> = v.records[index]
                .actions
                .iter()
                .map(|a| {
                    let nf: [u8; 32] = hex::decode(&a.nullifier)
                        .expect("own record")
                        .try_into()
                        .expect("32 bytes");
                    let cmx: [u8; 32] = hex::decode(&a.cmx)
                        .expect("own record")
                        .try_into()
                        .expect("32 bytes");
                    (nf, cmx)
                })
                .collect();
            if ours != memo.actions {
                return Err(Error::MemoMismatch {
                    index,
                    what: "actions",
                });
            }
        }
        Ok(v)
    }

    // ----- disclosure and persistence ------------------------------------------------------

    /// Selective disclosure for the holder whose full viewing key (96 bytes hex) is given.
    pub fn reveal(&self, viewing_key_hex: &str) -> Result<RevealReport, Error> {
        let fvk = fvk_from_hex(viewing_key_hex)?;
        Ok(crate::reveal::reveal(
            &fvk,
            self.records.iter().zip(self.bundles.iter()),
            |nf| self.nullifiers.contains(nf),
        ))
    }

    /// Key-free snapshot of the public state.
    pub fn snapshot(&self) -> VerifierSnapshot {
        VerifierSnapshot {
            format: 1,
            anchored_root: self.anchored_root.map(hex::encode),
            records: self.records.clone(),
        }
    }

    /// Rebuilds a verifier by replaying the log through the same tail, so a restored verifier
    /// refuses exactly what the original would have.
    pub fn restore(snapshot: &VerifierSnapshot) -> Result<Self, Error> {
        if snapshot.format != 1 {
            return Err(Error::Snapshot(format!(
                "unsupported format {}",
                snapshot.format
            )));
        }
        let mut v = Self::new();
        for record in &snapshot.records {
            v.replay(record)?;
        }
        if let Some(root) = &snapshot.anchored_root {
            v.set_anchored_root(root)?;
        }
        Ok(v)
    }
}
