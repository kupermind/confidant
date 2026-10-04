//! The pool: state, the three operations, the common verify-and-apply tail, snapshots.
//!
//! See the crate docs for the trust model and `docs/audit/REQ-pool-design-2026-10-04.md` for
//! the design of record.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use orchard::builder::{Builder, BundleType};
use orchard::bundle::{Authorized, BundleVersion, Flags, TxVersion};
use orchard::circuit::{ProvingKey, VerifyingKey};
use orchard::keys::{Scope, SpendingKey};
use orchard::note::Note;
use orchard::note_encryption::OrchardDomain;
use orchard::tree::MerkleHashOrchard;
use orchard::value::NoteValue;
use orchard::{Address, Anchor, Bundle};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use zcash_note_encryption::try_note_decryption;

use crate::error::Error;
use crate::keys::{address_to_hex, fvk_from_hex, fvk_to_hex, Keystore, OwnerId, OwnerKeys};
use crate::record::{bundle_from_record, record_from_bundle};
use crate::sighash::{shield_ctx, sighash, unshield_ctx, OpTag};
use crate::tree::CommitmentTree;
use crate::types::*;

/// Upper bound on recipients per private transfer (each action proves in ~0.5 s).
pub const MAX_OUTPUTS: usize = 8;
const MEMO: [u8; 512] = [0u8; 512];

/// The reused circuit, fixed for this pool: Zcash Orchard under protocol version 2
/// (`OrchardCircuitVersion::FixedPostNu6_2`), V2 notes, cross-address transfers permitted.
pub const BUNDLE_VERSION: BundleVersion = BundleVersion::orchard_v2();

/// Proving and verifying keys, built once per process from the circuit itself.
pub struct Params {
    pub version: BundleVersion,
    pub pk: ProvingKey,
    pub vk: VerifyingKey,
}

impl Params {
    /// Builds (first call, a few seconds) or returns the process-wide parameters.
    pub fn get() -> Arc<Params> {
        static PARAMS: OnceLock<Arc<Params>> = OnceLock::new();
        PARAMS
            .get_or_init(|| {
                let version = BUNDLE_VERSION;
                Arc::new(Params {
                    version,
                    pk: ProvingKey::build(version.circuit_version()),
                    vk: VerifyingKey::build(version.circuit_version()),
                })
            })
            .clone()
    }
}

/// A note the pool knows belongs to a registered owner.
struct TrackedNote {
    owner: OwnerId,
    note: Note,
    position: u64,
    nullifier: [u8; 32],
    spent: bool,
}

/// What `submit` reports after a bundle has been verified and applied.
#[derive(Clone, Debug)]
pub struct Accepted {
    pub bundle_index: usize,
    pub first_leaf: u64,
    pub new_root: Anchor,
    pub sighash: [u8; 32],
    pub nullifiers: Vec<[u8; 32]>,
    pub commitments: Vec<[u8; 32]>,
}

/// Operator-side persisted state. **Contains spending keys** — keep it out of git.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub format: u32,
    pub owners: Vec<OwnerSecret>,
    pub anchored_root: Option<String>,
    pub records: Vec<BundleRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerSecret {
    pub owner: String,
    pub spending_key: String,
}

pub struct Pool {
    params: Arc<Params>,
    tree: CommitmentTree,
    nullifiers: HashSet<[u8; 32]>,
    bundles: Vec<Bundle<Authorized, i64>>,
    records: Vec<BundleRecord>,
    keystore: Keystore,
    notes: Vec<TrackedNote>,
    anchored_root: Option<[u8; 32]>,
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Pool {
    /// An empty pool. Builds the circuit parameters on first use in the process.
    pub fn new() -> Self {
        Self {
            params: Params::get(),
            tree: CommitmentTree::new(),
            nullifiers: HashSet::new(),
            bundles: Vec::new(),
            records: Vec::new(),
            keystore: Keystore::default(),
            notes: Vec::new(),
            anchored_root: None,
        }
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    // ----- owners -------------------------------------------------------------------------

    fn info(id: &str, keys: &OwnerKeys) -> OwnerInfo {
        OwnerInfo {
            owner: id.to_owned(),
            address: address_to_hex(&keys.address()),
            viewing_key: fvk_to_hex(keys.full_viewing_key()),
        }
    }

    /// Creates a fresh owner with a random spending key held by this pool (operator custody).
    pub fn register_owner(&mut self, id: &str) -> Result<OwnerInfo, Error> {
        let keys = self.keystore.register(id, &mut OsRng)?;
        Ok(Self::info(id, keys))
    }

    pub fn owner_info(&self, id: &str) -> Result<OwnerInfo, Error> {
        Ok(Self::info(id, self.keystore.get(id)?))
    }

    /// Sum of the owner's unspent notes (the operator's view; the auditor path is `reveal`).
    pub fn balance(&self, id: &str) -> Result<u64, Error> {
        self.keystore.get(id)?;
        Ok(self
            .notes
            .iter()
            .filter(|n| n.owner == id && !n.spent)
            .map(|n| n.note.value().inner())
            .sum())
    }

    // ----- public state --------------------------------------------------------------------

    pub fn state(&self) -> PoolState {
        PoolState {
            root: hex::encode(self.tree.current_anchor().to_bytes()),
            anchored_root: self.anchored_root.map(hex::encode),
            leaf_count: self.tree.leaf_count(),
            nullifier_count: self.nullifiers.len(),
            bundle_count: self.bundles.len(),
        }
    }

    /// The front-end reports which root it wrote into an XRPL memo. Must be a root this pool
    /// has had.
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

    /// The exportable public log, oldest first.
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

    pub fn is_spent(&self, nullifier: &[u8; 32]) -> bool {
        self.nullifiers.contains(nullifier)
    }

    // ----- operations ----------------------------------------------------------------------

    /// Mints `amount` to `owner` after the front-end has submitted the XRPL deposit Payment
    /// `deposit_tx_hash`. The pool takes the (amount, deposit) pairing on the caller's word:
    /// backing is operator-enforced in this build, not cryptographic.
    pub fn shield(
        &mut self,
        owner: &str,
        amount: u64,
        deposit_tx_hash: &str,
    ) -> Result<ShieldReceipt, Error> {
        let (bundle, output_action) = self.build_shield(owner, amount, deposit_tx_hash)?;
        let acc = self.submit(bundle, OpTag::Shield, deposit_tx_hash, Some(amount))?;
        Ok(ShieldReceipt {
            commitment: hex::encode(acc.commitments[output_action]),
            leaf_index: acc.first_leaf + output_action as u64,
            new_root: hex::encode(acc.new_root.to_bytes()),
            bundle_index: acc.bundle_index,
            proof: self.records[acc.bundle_index].proof.clone(),
        })
    }

    /// One private hop: spends `owner`'s notes, pays each recipient, returns change. Nothing
    /// about it reaches the ledger.
    pub fn transfer(
        &mut self,
        owner: &str,
        outputs: &[TransferOutput],
    ) -> Result<TransferReceipt, Error> {
        let bundle = self.build_transfer(owner, outputs)?;
        let acc = self.submit(bundle, OpTag::Transfer, "", None)?;
        Ok(TransferReceipt {
            nullifiers: acc.nullifiers.iter().map(hex::encode).collect(),
            new_commitments: acc.commitments.iter().map(hex::encode).collect(),
            new_root: hex::encode(acc.new_root.to_bytes()),
            bundle_index: acc.bundle_index,
            proof: self.records[acc.bundle_index].proof.clone(),
        })
    }

    /// Spends `amount` of `owner`'s notes out of the pool. The front-end then pays
    /// `payout.amount` to `payout.destination` from the pool account. The sighash binds the
    /// proof to that destination and to the bundle's own value balance.
    pub fn unshield(
        &mut self,
        owner: &str,
        amount: u64,
        destination: &str,
    ) -> Result<UnshieldReceipt, Error> {
        let bundle = self.build_unshield(owner, amount, destination)?;
        let acc = self.submit(bundle, OpTag::Unshield, destination, Some(amount))?;
        Ok(UnshieldReceipt {
            nullifiers: acc.nullifiers.iter().map(hex::encode).collect(),
            new_commitments: acc.commitments.iter().map(hex::encode).collect(),
            new_root: hex::encode(acc.new_root.to_bytes()),
            bundle_index: acc.bundle_index,
            proof: self.records[acc.bundle_index].proof.clone(),
            payout: Payout {
                amount,
                destination: destination.to_owned(),
            },
        })
    }

    /// Selective disclosure for the holder whose full viewing key (96 bytes hex) is given.
    pub fn reveal(&self, viewing_key_hex: &str) -> Result<RevealReport, Error> {
        let fvk = fvk_from_hex(viewing_key_hex)?;
        Ok(crate::reveal::reveal(
            &fvk,
            self.records.iter().zip(self.bundles.iter()),
            |nf| self.nullifiers.contains(nf),
        ))
    }

    // ----- builders (public so tests and advanced callers can submit hand-built bundles) ---

    /// Builds and proves a shield bundle. Returns the bundle and the index of the action that
    /// carries the real output.
    pub fn build_shield(
        &self,
        owner: &str,
        amount: u64,
        deposit_tx_hash: &str,
    ) -> Result<(Bundle<Authorized, i64>, usize), Error> {
        if amount == 0 {
            return Err(Error::ZeroAmount);
        }
        let keys = self.keystore.get(owner)?;
        let version = self.params.version;
        let anchor = self.tree.current_anchor();
        let mut builder =
            Builder::new(BundleType::DEFAULT, version, Flags::SPENDS_DISABLED, anchor)?;
        let ovk = Some(keys.ovk(Scope::External));
        builder.add_output(ovk, keys.address(), NoteValue::from_raw(amount), MEMO)?;
        let (unauthorized, meta) = builder
            .build::<i64>(OsRng)?
            .ok_or(Error::Internal("empty bundle"))?;
        let output_action = meta
            .output_action_index(0)
            .ok_or(Error::Internal("output not placed"))?;
        let commitment: [u8; 32] = unauthorized.commitment(TxVersion::V5)?.into();
        let sh = sighash(&commitment, OpTag::Shield, &shield_ctx(deposit_tx_hash));
        let proven = unauthorized.create_proof(&self.params.pk, OsRng)?;
        let bundle = proven.apply_signatures(OsRng, sh, &[])?;
        Ok((bundle, output_action))
    }

    /// Builds and proves a private transfer bundle (value balance 0).
    pub fn build_transfer(
        &self,
        owner: &str,
        outputs: &[TransferOutput],
    ) -> Result<Bundle<Authorized, i64>, Error> {
        if outputs.is_empty() {
            return Err(Error::ZeroAmount);
        }
        if outputs.len() > MAX_OUTPUTS {
            return Err(Error::TooManyOutputs(outputs.len()));
        }
        let mut resolved = Vec::with_capacity(outputs.len());
        for o in outputs {
            if o.amount == 0 {
                return Err(Error::ZeroAmount);
            }
            resolved.push((self.keystore.get(&o.recipient)?.address(), o.amount));
        }
        self.build_spending(owner, &resolved, 0, OpTag::Transfer, "")
    }

    /// Builds and proves an unshield bundle (value balance `+amount`).
    pub fn build_unshield(
        &self,
        owner: &str,
        amount: u64,
        destination: &str,
    ) -> Result<Bundle<Authorized, i64>, Error> {
        if amount == 0 {
            return Err(Error::ZeroAmount);
        }
        if destination.is_empty() {
            return Err(Error::Build("empty destination".into()));
        }
        self.build_spending(owner, &[], amount, OpTag::Unshield, destination)
    }

    fn build_spending(
        &self,
        owner: &str,
        outputs: &[(Address, u64)],
        withdraw: u64,
        op: OpTag,
        external: &str,
    ) -> Result<Bundle<Authorized, i64>, Error> {
        let keys = self.keystore.get(owner)?;
        let total = outputs
            .iter()
            .map(|(_, v)| *v)
            .try_fold(withdraw, |acc, v| acc.checked_add(v))
            .ok_or(Error::AmountOverflow)?;
        debug_assert!(
            total > 0,
            "callers reject zero amounts before note selection"
        );

        // Greedy largest-first note selection.
        let mut candidates: Vec<&TrackedNote> = self
            .notes
            .iter()
            .filter(|n| n.owner == owner && !n.spent)
            .collect();
        candidates.sort_by_key(|n| std::cmp::Reverse(n.note.value().inner()));
        let available: u64 = candidates.iter().map(|n| n.note.value().inner()).sum();
        if available < total {
            return Err(Error::InsufficientFunds {
                available,
                requested: total,
            });
        }
        let mut selected = Vec::new();
        let mut sum = 0u64;
        for n in candidates {
            if sum >= total {
                break;
            }
            sum += n.note.value().inner();
            selected.push(n);
        }

        let anchor = self.tree.current_anchor();
        let version = self.params.version;
        let flags = version.default_flags();
        let mut builder = Builder::new(BundleType::DEFAULT, version, flags, anchor)?;
        for n in &selected {
            let path = self.tree.witness(n.position)?;
            builder.add_spend(keys.full_viewing_key().clone(), n.note, path)?;
        }
        let ovk = Some(keys.ovk(Scope::External));
        for (addr, value) in outputs {
            builder.add_output(ovk.clone(), *addr, NoteValue::from_raw(*value), MEMO)?;
        }
        let change = sum - total;
        if change > 0 {
            let ovk = Some(keys.ovk(Scope::Internal));
            let addr = keys.change_address();
            builder.add_output(ovk, addr, NoteValue::from_raw(change), MEMO)?;
        }

        let (unauthorized, _meta) = builder
            .build::<i64>(OsRng)?
            .ok_or(Error::Internal("empty bundle"))?;
        let vb = *unauthorized.value_balance();
        debug_assert_eq!(vb, withdraw as i64);
        let commitment: [u8; 32] = unauthorized.commitment(TxVersion::V5)?.into();
        let sh = sighash(&commitment, op, &external_ctx(op, external, vb));
        let proven = unauthorized.create_proof(&self.params.pk, OsRng)?;
        proven
            .apply_signatures(OsRng, sh, &[keys.spend_authorizing_key()])
            .map_err(Error::from)
    }

    // ----- the common tail -----------------------------------------------------------------

    /// Pure verification of a bundle against this pool's current state. Returns the sighash
    /// the signatures were checked against. Mutates nothing.
    ///
    /// Order, cheapest first: value-balance sign per op → vouched amount == value balance →
    /// anchor ∈ history → no nullifier spent or repeated → proof → spend-auth signatures →
    /// binding signature. Replays and double-spends are therefore refused before the SNARK
    /// verification is paid for.
    ///
    /// `external` is the deposit tx hash (shield), the XRPL destination (unshield) or empty
    /// (transfer). `expected_amount`, when given, must equal the deposit (shield) or payout
    /// (unshield) the caller vouches for; the payout amount bound into the sighash is always
    /// taken from the bundle's own value balance.
    pub fn verify(
        &self,
        bundle: &Bundle<Authorized, i64>,
        op: OpTag,
        external: &str,
        expected_amount: Option<u64>,
    ) -> Result<[u8; 32], Error> {
        let vb = *bundle.value_balance();
        let sign_ok = match op {
            OpTag::Shield => vb < 0,
            OpTag::Transfer => vb == 0,
            OpTag::Unshield => vb > 0,
        };
        if !sign_ok {
            return Err(Error::ValueBalanceSign { op, actual: vb });
        }
        if let Some(amount) = expected_amount {
            let expected = match op {
                OpTag::Shield => -(amount as i64),
                // A transfer has no external leg; vouching a non-zero amount for one is a
                // caller error, not something to silently accept.
                OpTag::Transfer if amount != 0 => {
                    return Err(Error::VouchedAmountNotApplicable { op, amount });
                }
                OpTag::Transfer => 0,
                OpTag::Unshield => amount as i64,
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
        // Nullifiers are checked BEFORE the proof so a replay or double-spend costs a set lookup,
        // not a SNARK verification. This makes `DoubleSpend` vs `InvalidProof` an oracle for
        // "is this nullifier spent?" that needs no valid proof. It leaks nothing TODAY because
        // the spent-nullifier set is public anyway (every nullifier is in the exported bundle
        // log). CAVEAT (audit ruling on PR #2): if a future variant makes the bundle log
        // private, this ordering becomes a real spent-nullifier probe — then either move this
        // block back after the signature checks (so only a party who can produce a valid proof
        // can probe) or collapse the two errors so they are indistinguishable at the wire.
        let mut seen = HashSet::with_capacity(bundle.actions().len());
        for action in bundle.actions() {
            let nf = action.nullifier().to_bytes();
            if self.nullifiers.contains(&nf) || !seen.insert(nf) {
                return Err(Error::DoubleSpend);
            }
        }
        bundle
            .verify_proof(&self.params.vk)
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

    /// Verifies a bundle and, only if every check passes, applies it: nullifiers recorded,
    /// commitments appended, root advanced, bundle logged. Any error leaves the pool unchanged.
    pub fn submit(
        &mut self,
        bundle: Bundle<Authorized, i64>,
        op: OpTag,
        external: &str,
        expected_amount: Option<u64>,
    ) -> Result<Accepted, Error> {
        self.submit_at(bundle, op, external, expected_amount, now())
    }

    fn submit_at(
        &mut self,
        bundle: Bundle<Authorized, i64>,
        op: OpTag,
        external: &str,
        expected_amount: Option<u64>,
        timestamp: u64,
    ) -> Result<Accepted, Error> {
        let sh = self.verify(&bundle, op, external, expected_amount)?;

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

        // First mutation. The in-memory tree cannot fail here after a successful verify.
        let first_leaf = self.tree.leaf_count();
        let new_root = self.tree.append_batch(&leaves)?;

        for nf in &nullifiers {
            self.nullifiers.insert(*nf);
        }
        for tracked in &mut self.notes {
            if nullifiers.contains(&tracked.nullifier) {
                tracked.spent = true;
            }
        }
        for (owner, keys) in self.keystore.iter() {
            for (i, action) in bundle.actions().iter().enumerate() {
                let domain = OrchardDomain::for_action(action);
                if let Some((note, _, _)) = keys
                    .prepared_ivks()
                    .iter()
                    .find_map(|ivk| try_note_decryption(&domain, *ivk, action))
                {
                    self.notes.push(TrackedNote {
                        owner: owner.clone(),
                        nullifier: note.nullifier(keys.full_viewing_key()).to_bytes(),
                        note,
                        position: first_leaf + i as u64,
                        spent: false,
                    });
                }
            }
        }

        let bundle_index = self.bundles.len();
        self.records.push(record_from_bundle(
            &bundle,
            bundle_index,
            op,
            external,
            first_leaf,
            timestamp,
        ));
        self.bundles.push(bundle);
        Ok(Accepted {
            bundle_index,
            first_leaf,
            new_root,
            sighash: sh,
            nullifiers,
            commitments,
        })
    }

    // ----- persistence ---------------------------------------------------------------------

    /// Operator-side snapshot: spending keys + the public log. Never commit it.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            format: 1,
            owners: self
                .keystore
                .iter()
                .map(|(id, keys)| OwnerSecret {
                    owner: id.clone(),
                    spending_key: hex::encode(keys.spending_key().to_bytes()),
                })
                .collect(),
            anchored_root: self.anchored_root.map(hex::encode),
            records: self.records.clone(),
        }
    }

    /// Rebuilds a pool by replaying the log through the same verify-and-apply tail, so a
    /// restored pool refuses exactly what the original would have.
    pub fn restore(snapshot: &Snapshot) -> Result<Self, Error> {
        if snapshot.format != 1 {
            return Err(Error::Snapshot(format!(
                "unsupported format {}",
                snapshot.format
            )));
        }
        let mut pool = Self::new();
        for o in &snapshot.owners {
            let bytes: [u8; 32] = hex::decode(&o.spending_key)
                .map_err(|_| Error::InvalidEncoding)?
                .try_into()
                .map_err(|_| Error::InvalidEncoding)?;
            let sk = SpendingKey::from_bytes(bytes)
                .into_option()
                .ok_or(Error::InvalidEncoding)?;
            pool.keystore.insert_existing(&o.owner, sk)?;
        }
        for (i, record) in snapshot.records.iter().enumerate() {
            if record.index != i {
                return Err(Error::Snapshot(format!("record {i} is out of order")));
            }
            let bundle = bundle_from_record(record, pool.params.version)?;
            let acc =
                pool.submit_at(bundle, record.op, &record.external, None, record.timestamp)?;
            if acc.first_leaf != record.first_leaf {
                return Err(Error::Snapshot(format!("record {i} leaf index mismatch")));
            }
        }
        if let Some(root) = &snapshot.anchored_root {
            pool.set_anchored_root(root)?;
        }
        Ok(pool)
    }
}

/// The external context bound into the sighash, derived from the op and the bundle's own
/// value balance (the payout amount is never taken from a second source).
///
/// INVARIANT OWED BY EVERY CALLER (audit ruling on PR #3): establish the sign of `value_balance`
/// for `op` *before* calling this. Today's callers: `Pool::verify` (step 1, the sign-per-op
/// check, runs before the sighash is computed) and `Pool::build_spending` (an unshield is built
/// with `withdraw: u64 > 0`, so its balance is positive by construction). A new caller that skips
/// the check makes the `expect` below reachable. The `expect` is deliberate and stays in release:
/// in the verify path a sighash must be computed correctly or not at all, so an impossible
/// negative unshield balance aborts rather than yielding a `Result` every caller has to thread.
fn external_ctx(op: OpTag, external: &str, value_balance: i64) -> Vec<u8> {
    match op {
        OpTag::Shield => shield_ctx(external),
        OpTag::Transfer => Vec::new(),
        OpTag::Unshield => {
            let amount = u64::try_from(value_balance).expect("unshield balance sign checked");
            unshield_ctx(external, amount)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_ctx_maps_each_op() {
        assert_eq!(
            external_ctx(OpTag::Transfer, "ignored", 0),
            Vec::<u8>::new()
        );
        assert_eq!(external_ctx(OpTag::Shield, "D", -5), shield_ctx("D"));
        assert_eq!(
            external_ctx(OpTag::Unshield, "rX", 7),
            unshield_ctx("rX", 7)
        );
    }

    #[test]
    #[should_panic(expected = "unshield balance sign checked")]
    fn external_ctx_treats_a_non_positive_unshield_balance_as_an_invariant_violation() {
        // Unreachable through `verify` (the sign check runs first) and through the builders
        // (withdraw > 0); the helper refuses to build a context for it rather than guess.
        let _ = external_ctx(OpTag::Unshield, "rX", -1);
    }

    #[test]
    #[should_panic(expected = "callers reject zero amounts")]
    fn build_spending_asserts_a_positive_total() {
        let mut pool = Pool::new();
        pool.register_owner("a").unwrap();
        let _ = pool.build_spending("a", &[], 0, OpTag::Unshield, "rX");
    }

    #[test]
    fn build_spending_checks_the_owner_first() {
        let pool = Pool::new();
        assert!(matches!(
            pool.build_spending("nobody", &[], 1, OpTag::Unshield, "rX"),
            Err(Error::UnknownOwner(_))
        ));
    }
}
