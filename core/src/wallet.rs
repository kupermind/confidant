//! The holder's side: keys, own notes, and building / proving / signing of the three operations.
//! Keys and plaintext never leave this type. It needs the public bundle log (from any verifier)
//! to find its notes and to compute Merkle witnesses; it never talks to XRPL itself.

use orchard::builder::{Builder, BundleType};
use orchard::bundle::{Flags, TxVersion};
use orchard::keys::Scope;
use orchard::note::Note;
use orchard::note_encryption::OrchardDomain;
use orchard::value::NoteValue;
use orchard::Address;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use zcash_note_encryption::try_note_decryption;

use crate::error::Error;
use crate::keys::{address_from_hex, address_to_hex, fvk_to_hex, spending_key_from_hex, OwnerKeys};
use crate::record::{bundle_hash, record_from_bundle};
use crate::sighash::{external_ctx, sighash, OpTag};
use crate::types::{BundleRecord, TransferOutput, WalletInfo};
use crate::verifier::Verifier;

/// Upper bound on recipients per private transfer (each action proves in ~0.5 s).
pub const MAX_OUTPUTS: usize = 8;
const MEMO: [u8; 512] = [0u8; 512];

/// The wallet's persisted secret. **Contains the spending key** — never commit, mode 0600.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletSecret {
    pub format: u32,
    pub spending_key: String,
}

/// A built, proven and signed bundle ready for submission, plus its hash (what a shield's
/// deposit memo must carry).
#[derive(Clone, Debug)]
pub struct Prepared {
    /// `index`, `timestamp` and `first_leaf` are zero; the verifier fills them on acceptance.
    pub record: BundleRecord,
    pub hash: [u8; 32],
}

impl Prepared {
    pub fn hash_hex(&self) -> String {
        hex::encode(self.hash)
    }
}

struct TrackedNote {
    note: Note,
    position: u64,
    nullifier: [u8; 32],
    spent: bool,
}

pub struct Wallet {
    keys: OwnerKeys,
    replica: Verifier,
    notes: Vec<TrackedNote>,
    synced: usize,
}

impl Default for Wallet {
    /// A wallet with a fresh random spending key.
    fn default() -> Self {
        Self::from_keys(OwnerKeys::random(&mut OsRng))
    }
}

impl Wallet {
    /// A wallet with a fresh random spending key (same as `Default`).
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_spending_key(sk: orchard::keys::SpendingKey) -> Self {
        Self::from_keys(OwnerKeys::from_spending_key(sk))
    }

    fn from_keys(keys: OwnerKeys) -> Self {
        Self {
            keys,
            replica: Verifier::new(),
            notes: Vec::new(),
            synced: 0,
        }
    }

    pub fn from_secret(secret: &WalletSecret) -> Result<Self, Error> {
        if secret.format != 1 {
            return Err(Error::Snapshot(format!(
                "unsupported wallet format {}",
                secret.format
            )));
        }
        Ok(Self::from_spending_key(spending_key_from_hex(
            &secret.spending_key,
        )?))
    }

    pub fn secret(&self) -> WalletSecret {
        WalletSecret {
            format: 1,
            spending_key: hex::encode(self.keys.spending_key().to_bytes()),
        }
    }

    pub fn info(&self) -> WalletInfo {
        WalletInfo {
            address: address_to_hex(&self.keys.address()),
            viewing_key: fvk_to_hex(self.keys.full_viewing_key()),
        }
    }

    pub fn address(&self) -> Address {
        self.keys.address()
    }

    /// The replica of the public state this wallet has synced.
    pub fn replica(&self) -> &Verifier {
        &self.replica
    }

    /// Number of log records already synced.
    pub fn synced(&self) -> usize {
        self.synced
    }

    /// Sum of unspent notes this wallet owns.
    pub fn balance(&self) -> u64 {
        self.notes
            .iter()
            .filter(|n| !n.spent)
            .map(|n| n.note.value().inner())
            .sum()
    }

    /// Catches up with the public log: every new record is re-verified into the replica, spent
    /// notes are marked, and new notes to this wallet are found by trial decryption. Returns the
    /// number of records applied. The log must extend what was synced before.
    pub fn sync(&mut self, log: &[BundleRecord]) -> Result<usize, Error> {
        if log.len() < self.synced {
            return Err(Error::LogRewound);
        }
        let mut applied = 0;
        for record in &log[self.synced..] {
            let acc = self.replica.replay(record)?;
            for n in &mut self.notes {
                if acc.nullifiers.contains(&hex::encode(n.nullifier)) {
                    n.spent = true;
                }
            }
            let bundle = self
                .replica
                .bundle(acc.bundle_index)
                .expect("just accepted");
            for (i, action) in bundle.actions().iter().enumerate() {
                let domain = OrchardDomain::for_action(action);
                if let Some((note, _, _)) = self
                    .keys
                    .prepared_ivks()
                    .iter()
                    .find_map(|ivk| try_note_decryption(&domain, *ivk, action))
                {
                    self.notes.push(TrackedNote {
                        nullifier: note.nullifier(self.keys.full_viewing_key()).to_bytes(),
                        note,
                        position: acc.first_leaf + i as u64,
                        spent: false,
                    });
                }
            }
            self.synced += 1;
            applied += 1;
        }
        Ok(applied)
    }

    // ----- builders ------------------------------------------------------------------------

    /// Builds and proves a shield of `amount` to this wallet. Send the deposit Payment with
    /// memo `confidant/shield` = `hash_hex()` afterwards, then submit `{record, depositTxHash}`.
    pub fn build_shield(&self, amount: u64) -> Result<Prepared, Error> {
        if amount == 0 {
            return Err(Error::ZeroAmount);
        }
        let version = self.replica.params().version;
        let anchor = self.replica.current_anchor();
        let mut builder =
            Builder::new(BundleType::DEFAULT, version, Flags::SPENDS_DISABLED, anchor)?;
        let ovk = Some(self.keys.ovk(Scope::External));
        builder.add_output(ovk, self.keys.address(), NoteValue::from_raw(amount), MEMO)?;
        let (unauthorized, _meta) = builder
            .build::<i64>(OsRng)?
            .ok_or(Error::Internal("empty bundle"))?;
        let vb = *unauthorized.value_balance();
        let commitment: [u8; 32] = unauthorized.commitment(TxVersion::V5)?.into();
        let sh = sighash(
            &commitment,
            OpTag::Shield,
            &external_ctx(OpTag::Shield, "", vb),
        );
        let proven = unauthorized.create_proof(self.replica.params().pk(), OsRng)?;
        let bundle = proven.apply_signatures(OsRng, sh, &[])?;
        let record = record_from_bundle(&bundle, 0, OpTag::Shield, "", 0, 0);
        let hash = bundle_hash(&record)?;
        Ok(Prepared { record, hash })
    }

    /// Builds and proves a private transfer (value balance 0). Recipients are raw Orchard
    /// addresses (43 bytes, hex).
    pub fn build_transfer(&self, outputs: &[TransferOutput]) -> Result<Prepared, Error> {
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
            resolved.push((address_from_hex(&o.recipient)?, o.amount));
        }
        self.build_spending(&resolved, 0, OpTag::Transfer, "")
    }

    /// Builds and proves an unshield of `amount` to the XRPL address `destination`.
    pub fn build_unshield(&self, amount: u64, destination: &str) -> Result<Prepared, Error> {
        if amount == 0 {
            return Err(Error::ZeroAmount);
        }
        if destination.is_empty() {
            return Err(Error::Build("empty destination".into()));
        }
        self.build_spending(&[], amount, OpTag::Unshield, destination)
    }

    fn build_spending(
        &self,
        outputs: &[(Address, u64)],
        withdraw: u64,
        op: OpTag,
        external: &str,
    ) -> Result<Prepared, Error> {
        let total = outputs
            .iter()
            .map(|(_, v)| *v)
            .try_fold(withdraw, |acc, v| acc.checked_add(v))
            .ok_or(Error::AmountOverflow)?;
        // Callers reject zero amounts first; a zero total here selects no notes and ends in
        // `Error::Internal("empty bundle")` below, in debug and release alike.

        // Greedy largest-first note selection.
        let mut candidates: Vec<&TrackedNote> = self.notes.iter().filter(|n| !n.spent).collect();
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

        let version = self.replica.params().version;
        let anchor = self.replica.current_anchor();
        let flags = version.default_flags();
        let mut builder = Builder::new(BundleType::DEFAULT, version, flags, anchor)?;
        for n in &selected {
            let path = self.replica.witness(n.position)?;
            builder.add_spend(self.keys.full_viewing_key().clone(), n.note, path)?;
        }
        let ovk = Some(self.keys.ovk(Scope::External));
        for (addr, value) in outputs {
            builder.add_output(ovk.clone(), *addr, NoteValue::from_raw(*value), MEMO)?;
        }
        let change = sum - total;
        if change > 0 {
            let ovk = Some(self.keys.ovk(Scope::Internal));
            let addr = self.keys.change_address();
            builder.add_output(ovk, addr, NoteValue::from_raw(change), MEMO)?;
        }

        let (unauthorized, _meta) = builder
            .build::<i64>(OsRng)?
            .ok_or(Error::Internal("empty bundle"))?;
        let vb = *unauthorized.value_balance();
        debug_assert_eq!(vb, withdraw as i64);
        let commitment: [u8; 32] = unauthorized.commitment(TxVersion::V5)?.into();
        let sh = sighash(&commitment, op, &external_ctx(op, external, vb));
        let proven = unauthorized.create_proof(self.replica.params().pk(), OsRng)?;
        let bundle = proven.apply_signatures(OsRng, sh, &[self.keys.spend_authorizing_key()])?;
        let record = record_from_bundle(&bundle, 0, op, external, 0, 0);
        let hash = bundle_hash(&record)?;
        Ok(Prepared { record, hash })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-crate, so the runner's coverage attributes the trivial accessors to this file whatever
    /// its inlining decisions across the integration-test crates.
    #[test]
    fn accessors_of_a_fresh_wallet() {
        let w = Wallet::new();
        assert_eq!(w.replica().state().bundle_count, 0);
        assert_eq!(w.synced(), 0);
        assert_eq!(w.balance(), 0);
        assert_eq!(w.info().address.len(), 86);
        let _ = w.address();
    }
}
