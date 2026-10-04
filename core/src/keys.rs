//! Owner keys and the operator-held keystore.
//!
//! Every registered owner gets a fresh random Orchard [`SpendingKey`]; the pool derives the
//! [`FullViewingKey`], the payment address (external scope, index 0) and the change address
//! (internal scope, index 0). The operator holds all of it — see the trust model in the crate docs.

use std::collections::BTreeMap;

use orchard::keys::{
    FullViewingKey, OutgoingViewingKey, PreparedIncomingViewingKey, Scope, SpendAuthorizingKey,
    SpendingKey,
};
use orchard::Address;
use rand::{CryptoRng, RngCore};

use crate::error::Error;

/// Opaque owner identifier chosen by the API layer (e.g. `"alice"`).
pub type OwnerId = String;

/// One owner's keys, with the prepared incoming viewing keys cached for trial decryption.
pub struct OwnerKeys {
    sk: SpendingKey,
    fvk: FullViewingKey,
    ivk_external: PreparedIncomingViewingKey,
    ivk_internal: PreparedIncomingViewingKey,
}

impl OwnerKeys {
    /// Generates a fresh random spending key.
    pub fn random(rng: &mut (impl RngCore + CryptoRng)) -> Self {
        // `from_bytes` rejects the negligible fraction of byte strings whose derived spend
        // authorizing key would be zero; keep drawing until one is accepted.
        let sk = std::iter::repeat_with(|| {
            let mut bytes = [0u8; 32];
            rng.fill_bytes(&mut bytes);
            SpendingKey::from_bytes(bytes).into_option()
        })
        .flatten()
        .next()
        .expect("an infinite iterator yields a valid spending key");
        Self::from_spending_key(sk)
    }

    /// Wraps an existing spending key.
    pub fn from_spending_key(sk: SpendingKey) -> Self {
        let fvk = FullViewingKey::from(&sk);
        let ivk_external = PreparedIncomingViewingKey::new(&fvk.to_ivk(Scope::External));
        let ivk_internal = PreparedIncomingViewingKey::new(&fvk.to_ivk(Scope::Internal));
        Self {
            sk,
            fvk,
            ivk_external,
            ivk_internal,
        }
    }

    pub fn spending_key(&self) -> &SpendingKey {
        &self.sk
    }

    pub fn full_viewing_key(&self) -> &FullViewingKey {
        &self.fvk
    }

    /// The key that signs this owner's spends.
    pub fn spend_authorizing_key(&self) -> SpendAuthorizingKey {
        SpendAuthorizingKey::from(&self.sk)
    }

    /// Payment address others send to (external scope, diversifier index 0).
    pub fn address(&self) -> Address {
        self.fvk.address_at(0u32, Scope::External)
    }

    /// Address this owner's own change goes to (internal scope, diversifier index 0).
    pub fn change_address(&self) -> Address {
        self.fvk.address_at(0u32, Scope::Internal)
    }

    pub fn ovk(&self, scope: Scope) -> OutgoingViewingKey {
        self.fvk.to_ovk(scope)
    }

    /// Prepared incoming viewing keys for both scopes, for trial decryption.
    pub fn prepared_ivks(&self) -> [&PreparedIncomingViewingKey; 2] {
        [&self.ivk_external, &self.ivk_internal]
    }
}

/// Encodes a full viewing key as 96 hex bytes — the "auditor key" handed over at onboarding.
pub fn fvk_to_hex(fvk: &FullViewingKey) -> String {
    hex::encode(fvk.to_bytes())
}

/// Parses a 96-byte hex full viewing key.
pub fn fvk_from_hex(s: &str) -> Result<FullViewingKey, Error> {
    let bytes = hex::decode(s.trim()).map_err(|_| Error::InvalidEncoding)?;
    let arr: [u8; 96] = bytes.try_into().map_err(|_| Error::InvalidEncoding)?;
    FullViewingKey::from_bytes(&arr).ok_or(Error::InvalidEncoding)
}

/// Encodes a raw Orchard address (43 bytes) as hex.
pub fn address_to_hex(addr: &Address) -> String {
    hex::encode(addr.to_raw_address_bytes())
}

/// The operator-held map of owners to keys.
#[derive(Default)]
pub struct Keystore {
    owners: BTreeMap<OwnerId, OwnerKeys>,
}

impl Keystore {
    pub fn register(
        &mut self,
        id: &str,
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<&OwnerKeys, Error> {
        if self.owners.contains_key(id) {
            return Err(Error::OwnerExists(id.to_owned()));
        }
        self.owners.insert(id.to_owned(), OwnerKeys::random(rng));
        Ok(&self.owners[id])
    }

    pub fn get(&self, id: &str) -> Result<&OwnerKeys, Error> {
        self.owners
            .get(id)
            .ok_or_else(|| Error::UnknownOwner(id.to_owned()))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&OwnerId, &OwnerKeys)> {
        self.owners.iter()
    }
}

impl Keystore {
    /// Re-inserts an owner from a persisted spending key (snapshot restore).
    pub fn insert_existing(&mut self, id: &str, sk: SpendingKey) -> Result<&OwnerKeys, Error> {
        if self.owners.contains_key(id) {
            return Err(Error::OwnerExists(id.to_owned()));
        }
        self.owners
            .insert(id.to_owned(), OwnerKeys::from_spending_key(sk));
        Ok(&self.owners[id])
    }
}
