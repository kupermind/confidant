//! Selective disclosure: what one holder's full viewing key reveals, and nothing more.

use orchard::bundle::Authorized;
use orchard::keys::{FullViewingKey, PreparedIncomingViewingKey, Scope};
use orchard::note_encryption::OrchardDomain;
use orchard::Bundle;
use zcash_note_encryption::{try_note_decryption, try_output_recovery_with_ovk};

use crate::keys::address_to_hex;
use crate::types::{BundleRecord, Direction, RevealReport, RevealedNote};

/// Trial-decrypts every recorded action with the holder's incoming viewing keys (both scopes)
/// and recovers their outgoing notes with their outgoing viewing keys. `is_spent` answers
/// whether a nullifier is in the pool's spent set.
pub fn reveal<'a>(
    fvk: &FullViewingKey,
    bundles: impl Iterator<Item = (&'a BundleRecord, &'a Bundle<Authorized, i64>)>,
    is_spent: impl Fn(&[u8; 32]) -> bool,
) -> RevealReport {
    let ivks = [
        PreparedIncomingViewingKey::new(&fvk.to_ivk(Scope::External)),
        PreparedIncomingViewingKey::new(&fvk.to_ivk(Scope::Internal)),
    ];
    let ovks = [fvk.to_ovk(Scope::External), fvk.to_ovk(Scope::Internal)];

    let mut notes = Vec::new();
    let mut balance = 0u64;
    for (record, bundle) in bundles {
        for (i, action) in bundle.actions().iter().enumerate() {
            let domain = OrchardDomain::for_action(action);
            let leaf_index = record.first_leaf + i as u64;
            if let Some((note, _, _)) = ivks
                .iter()
                .find_map(|ivk| try_note_decryption(&domain, ivk, action))
            {
                let spent = is_spent(&note.nullifier(fvk).to_bytes());
                if !spent {
                    balance += note.value().inner();
                }
                notes.push(RevealedNote {
                    direction: Direction::Incoming,
                    amount: note.value().inner(),
                    leaf_index,
                    bundle_index: record.index,
                    timestamp: record.timestamp,
                    spent,
                    recipient: None,
                });
            } else if let Some((note, recipient, _)) = ovks.iter().find_map(|ovk| {
                try_output_recovery_with_ovk(
                    &domain,
                    ovk,
                    action,
                    action.cv_net(),
                    &action.encrypted_note().out_ciphertext,
                )
            }) {
                notes.push(RevealedNote {
                    direction: Direction::Outgoing,
                    amount: note.value().inner(),
                    leaf_index,
                    bundle_index: record.index,
                    timestamp: record.timestamp,
                    spent: false,
                    recipient: Some(address_to_hex(&recipient)),
                });
            }
        }
    }
    RevealReport { balance, notes }
}
