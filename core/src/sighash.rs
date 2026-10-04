//! The message every spend-authorization and binding signature of a bundle signs.
//!
//! `sighash = BLAKE2b-256(personal = "ConfidantSigHash")(bundle_commitment ‖ op_tag ‖ external_ctx)`
//!
//! `bundle_commitment` is Orchard's own `Bundle::commitment(TxVersion::V5)`, which already covers
//! the anchor, flags, value balance and every action's public parts. The suffix binds a shield to
//! its XRPL deposit and an unshield to its XRPL payout, so a published unshield proof cannot be
//! re-pointed at another destination or amount (REQ §4).

use serde::{Deserialize, Serialize};

/// BLAKE2b personalization (exactly 16 bytes, as BLAKE2b requires).
pub const PERSONALIZATION: &[u8; 16] = b"ConfidantSigHash";

/// Which pool operation a bundle performs. One byte inside the sighash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum OpTag {
    Shield = 1,
    Transfer = 2,
    Unshield = 3,
}

/// Computes the sighash for a bundle commitment, operation and external context.
pub fn sighash(bundle_commitment: &[u8; 32], op: OpTag, external_ctx: &[u8]) -> [u8; 32] {
    let hash = blake2b_simd::Params::new()
        .hash_length(32)
        .personal(PERSONALIZATION)
        .to_state()
        .update(bundle_commitment)
        .update(&[op as u8])
        .update(external_ctx)
        .finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(hash.as_bytes());
    out
}

fn len_prefixed(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut v = Vec::with_capacity(4 + bytes.len());
    v.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    v.extend_from_slice(bytes);
    v
}

/// External context of a shield: the XRPL deposit transaction hash (as the FE reports it).
pub fn shield_ctx(deposit_tx_hash: &str) -> Vec<u8> {
    len_prefixed(deposit_tx_hash)
}

/// External context of an unshield: the XRPL destination and the payout amount.
pub fn unshield_ctx(destination: &str, amount: u64) -> Vec<u8> {
    let mut v = len_prefixed(destination);
    v.extend_from_slice(&amount.to_le_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sighash_depends_on_every_input() {
        let c = [7u8; 32];
        let base = sighash(&c, OpTag::Unshield, &unshield_ctx("rDest", 5));
        assert_ne!(
            base,
            sighash(&[8u8; 32], OpTag::Unshield, &unshield_ctx("rDest", 5))
        );
        assert_ne!(
            base,
            sighash(&c, OpTag::Transfer, &unshield_ctx("rDest", 5))
        );
        assert_ne!(
            base,
            sighash(&c, OpTag::Unshield, &unshield_ctx("rOther", 5))
        );
        assert_ne!(
            base,
            sighash(&c, OpTag::Unshield, &unshield_ctx("rDest", 6))
        );
    }
}
