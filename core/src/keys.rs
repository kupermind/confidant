//! A holder's keys: a random Orchard spending key and what the wallet derives from it.

use orchard::keys::{
    FullViewingKey, OutgoingViewingKey, PreparedIncomingViewingKey, Scope, SpendAuthorizingKey,
    SpendingKey,
};
use orchard::Address;
use rand::{CryptoRng, RngCore};

use crate::error::Error;

/// One holder's keys, with the prepared incoming viewing keys cached for trial decryption.
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

    /// The key that signs this holder's spends.
    pub fn spend_authorizing_key(&self) -> SpendAuthorizingKey {
        SpendAuthorizingKey::from(&self.sk)
    }

    /// Payment address others send to (external scope, diversifier index 0).
    pub fn address(&self) -> Address {
        self.fvk.address_at(0u32, Scope::External)
    }

    /// Address this holder's own change goes to (internal scope, diversifier index 0).
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

/// Encodes a full viewing key as 96 hex bytes — the "auditor key" a holder hands over.
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

/// Parses a 43-byte hex raw Orchard address.
pub fn address_from_hex(s: &str) -> Result<Address, Error> {
    let bytes = hex::decode(s.trim()).map_err(|_| Error::InvalidEncoding)?;
    let arr: [u8; 43] = bytes.try_into().map_err(|_| Error::InvalidEncoding)?;
    Address::from_raw_address_bytes(&arr)
        .into_option()
        .ok_or(Error::InvalidEncoding)
}

/// Parses a 32-byte hex spending key.
pub fn spending_key_from_hex(s: &str) -> Result<SpendingKey, Error> {
    let bytes = hex::decode(s.trim()).map_err(|_| Error::InvalidEncoding)?;
    let arr: [u8; 32] = bytes.try_into().map_err(|_| Error::InvalidEncoding)?;
    SpendingKey::from_bytes(arr)
        .into_option()
        .ok_or(Error::InvalidEncoding)
}
