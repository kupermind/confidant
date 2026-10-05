//! The message every spend-authorization and binding signature of a bundle signs.
//!
//! `sighash = BLAKE2b-256(personal = "ConfidantSigHash")(bundle_commitment ‖ op_tag ‖ external_ctx)`
//!
//! `bundle_commitment` is Orchard's own `Bundle::commitment(TxVersion::V5)`, which already covers
//! the anchor, flags, value balance and every action's public parts. The op tag keeps a bundle of
//! one operation from being presented as another. The external context binds an unshield to its
//! XRPL destination and to its own value balance, so a published unshield proof cannot be
//! re-pointed. Shields carry no external context: the deposit ↔ bundle binding lives in the deposit
//! memo (the bundle hash), the consumed-deposit set and the anchor memo (SPEC §v2).

use serde::{Deserialize, Serialize};

/// BLAKE2b personalization (exactly 16 bytes, as BLAKE2b requires).
pub const PERSONALIZATION: &[u8; 16] = b"ConfidantSigHash";

/// Which pool operation a bundle performs. One byte inside the sighash and the anchor memo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum OpTag {
    Shield = 1,
    Transfer = 2,
    Unshield = 3,
}

impl OpTag {
    /// Inverse of `op as u8`.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(OpTag::Shield),
            2 => Some(OpTag::Transfer),
            3 => Some(OpTag::Unshield),
            _ => None,
        }
    }
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

/// External context of an unshield: the XRPL destination and the payout amount (which the
/// verifier always derives from the bundle's own value balance).
pub fn unshield_ctx(destination: &str, amount: u64) -> Vec<u8> {
    let bytes = destination.as_bytes();
    let mut v = Vec::with_capacity(12 + bytes.len());
    v.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    v.extend_from_slice(bytes);
    v.extend_from_slice(&amount.to_le_bytes());
    v
}

/// The external context bound into the sighash for `op`, derived from the bundle's own value
/// balance (the payout amount is never taken from a second source).
///
/// INVARIANT OWED BY EVERY CALLER (audit ruling on PR #3): establish the sign of `value_balance`
/// for `op` *before* calling this. Callers today: `Verifier::verify` (step 1, the sign-per-op
/// check) and `Wallet` builders (an unshield is built with `withdraw: u64 > 0`). The `expect` is
/// deliberate and stays in release: a sighash must be computed correctly or not at all.
pub fn external_ctx(op: OpTag, external: &str, value_balance: i64) -> Vec<u8> {
    match op {
        OpTag::Shield | OpTag::Transfer => Vec::new(),
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

    #[test]
    fn op_tag_round_trips_and_rejects_unknown_bytes() {
        for op in [OpTag::Shield, OpTag::Transfer, OpTag::Unshield] {
            assert_eq!(OpTag::from_byte(op as u8), Some(op));
        }
        assert_eq!(OpTag::from_byte(0), None);
        assert_eq!(OpTag::from_byte(4), None);
    }

    #[test]
    fn external_ctx_maps_each_op() {
        assert_eq!(external_ctx(OpTag::Shield, "ignored", -5), Vec::<u8>::new());
        assert_eq!(
            external_ctx(OpTag::Transfer, "ignored", 0),
            Vec::<u8>::new()
        );
        assert_eq!(
            external_ctx(OpTag::Unshield, "rX", 7),
            unshield_ctx("rX", 7)
        );
    }

    #[test]
    #[should_panic(expected = "unshield balance sign checked")]
    fn external_ctx_treats_a_non_positive_unshield_balance_as_an_invariant_violation() {
        let _ = external_ctx(OpTag::Unshield, "rX", -1);
    }
}
