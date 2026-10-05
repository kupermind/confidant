//! Conversion between a live [`Bundle`] and its public [`BundleRecord`], both ways, plus the
//! **bundle hash**: `BLAKE2b-256(personal "ConfidantBundle")` over the canonical bundle bytes.
//!
//! The record is what wallets submit and verifiers log; rebuilding a bundle from it is what a
//! verifier (or any third party) does before re-verifying proof and signatures. The hash is what
//! the deposit memo and the anchor memo commit to; it covers the bundle itself (anchor, flags,
//! value balance, every action's public parts, proof, binding signature) and none of the
//! verifier's metadata (`index`, `timestamp`, `first_leaf`, `external`).

use nonempty::NonEmpty;
use orchard::bundle::{Authorized, BundleVersion, Flags};
use orchard::circuit::Proof;
use orchard::note::{ExtractedNoteCommitment, Nullifier, TransmittedNoteCiphertext};
use orchard::primitives::redpallas;
use orchard::value::ValueCommitment;
use orchard::{Action, Anchor, Bundle};

use crate::error::Error;
use crate::sighash::OpTag;
use crate::types::{ActionRecord, BundleRecord};

/// BLAKE2b personalization of the bundle hash (≤ 16 bytes).
pub const HASH_PERSONALIZATION: &[u8; 15] = b"ConfidantBundle";

fn arr<const N: usize>(s: &str) -> Result<[u8; N], Error> {
    let v = hex::decode(s).map_err(|_| Error::InvalidEncoding)?;
    v.try_into().map_err(|_| Error::InvalidEncoding)
}

fn ct<T>(o: subtle::CtOption<T>) -> Result<T, Error> {
    o.into_option().ok_or(Error::InvalidEncoding)
}

/// Serialises a bundle's public effects. `external` is the verifier's external context
/// (deposit tx hash / destination / empty); wallets pass what they know.
pub fn record_from_bundle(
    bundle: &Bundle<Authorized, i64>,
    index: usize,
    op: OpTag,
    external: &str,
    first_leaf: u64,
    timestamp: u64,
) -> BundleRecord {
    let actions = bundle
        .actions()
        .iter()
        .map(|a| ActionRecord {
            nullifier: hex::encode(a.nullifier().to_bytes()),
            rk: hex::encode(<[u8; 32]>::from(a.rk())),
            cmx: hex::encode(a.cmx().to_bytes()),
            cv_net: hex::encode(a.cv_net().to_bytes()),
            epk: hex::encode(a.encrypted_note().epk_bytes),
            enc_ciphertext: hex::encode(a.encrypted_note().enc_ciphertext),
            out_ciphertext: hex::encode(a.encrypted_note().out_ciphertext),
            spend_auth_sig: hex::encode(<[u8; 64]>::from(a.authorization())),
        })
        .collect();
    BundleRecord {
        index,
        op,
        timestamp,
        external: external.to_owned(),
        anchor: hex::encode(bundle.anchor().to_bytes()),
        flags: bundle.flag_byte(),
        value_balance: *bundle.value_balance(),
        first_leaf,
        actions,
        proof: hex::encode(bundle.authorization().proof().as_ref()),
        binding_signature: hex::encode(<[u8; 64]>::from(
            bundle.authorization().binding_signature(),
        )),
    }
}

/// Rebuilds a bundle from its record. Only well-formedness is checked here; the verifier's tail
/// does the proof and signature checks.
pub fn bundle_from_record(
    record: &BundleRecord,
    version: BundleVersion,
) -> Result<Bundle<Authorized, i64>, Error> {
    let mut actions = Vec::with_capacity(record.actions.len());
    for a in &record.actions {
        let nf = ct(Nullifier::from_bytes(&arr::<32>(&a.nullifier)?))?;
        let rk = redpallas::VerificationKey::try_from(arr::<32>(&a.rk)?)
            .map_err(|_| Error::InvalidEncoding)?;
        let cmx = ct(ExtractedNoteCommitment::from_bytes(&arr::<32>(&a.cmx)?))?;
        let cv_net = ct(ValueCommitment::from_bytes(&arr::<32>(&a.cv_net)?))?;
        let encrypted_note = TransmittedNoteCiphertext {
            epk_bytes: arr::<32>(&a.epk)?,
            enc_ciphertext: arr::<580>(&a.enc_ciphertext)?,
            out_ciphertext: arr::<80>(&a.out_ciphertext)?,
        };
        let sig = redpallas::Signature::from(arr::<64>(&a.spend_auth_sig)?);
        actions.push(
            Action::from_parts(nf, rk, cmx, encrypted_note, cv_net, sig)
                .map_err(|e| Error::Build(e.to_string()))?,
        );
    }
    let actions = NonEmpty::from_vec(actions).ok_or(Error::InvalidEncoding)?;
    let flags = Flags::from_byte(record.flags, version).ok_or(Error::InvalidEncoding)?;
    let anchor = ct(Anchor::from_bytes(arr::<32>(&record.anchor)?))?;
    let proof = Proof::new(hex::decode(&record.proof).map_err(|_| Error::InvalidEncoding)?);
    let binding = redpallas::Signature::from(arr::<64>(&record.binding_signature)?);
    Bundle::try_from_parts(
        actions,
        flags,
        record.value_balance,
        anchor,
        Authorized::from_parts(proof, binding),
        version,
    )
    .map_err(|e| Error::Build(e.to_string()))
}

/// Canonical bytes of a record's bundle part (what the bundle hash covers):
/// `anchor(32) ‖ flags(1) ‖ valueBalance(i64 LE) ‖ n(u32 LE) ‖ n × action ‖ proofLen(u32 LE) ‖ proof ‖ bindingSig(64)`,
/// each action being `nf ‖ rk ‖ cmx ‖ cvNet ‖ epk ‖ enc(580) ‖ out(80) ‖ sig(64)`.
pub fn canonical_bytes(record: &BundleRecord) -> Result<Vec<u8>, Error> {
    let mut v = Vec::with_capacity(512 + record.actions.len() * 912 + record.proof.len() / 2);
    v.extend_from_slice(&arr::<32>(&record.anchor)?);
    v.push(record.flags);
    v.extend_from_slice(&record.value_balance.to_le_bytes());
    v.extend_from_slice(&(record.actions.len() as u32).to_le_bytes());
    for a in &record.actions {
        v.extend_from_slice(&arr::<32>(&a.nullifier)?);
        v.extend_from_slice(&arr::<32>(&a.rk)?);
        v.extend_from_slice(&arr::<32>(&a.cmx)?);
        v.extend_from_slice(&arr::<32>(&a.cv_net)?);
        v.extend_from_slice(&arr::<32>(&a.epk)?);
        v.extend_from_slice(&arr::<580>(&a.enc_ciphertext)?);
        v.extend_from_slice(&arr::<80>(&a.out_ciphertext)?);
        v.extend_from_slice(&arr::<64>(&a.spend_auth_sig)?);
    }
    let proof = hex::decode(&record.proof).map_err(|_| Error::InvalidEncoding)?;
    v.extend_from_slice(&(proof.len() as u32).to_le_bytes());
    v.extend_from_slice(&proof);
    v.extend_from_slice(&arr::<64>(&record.binding_signature)?);
    Ok(v)
}

/// The bundle hash: what the deposit memo and the anchor memo commit to.
pub fn bundle_hash(record: &BundleRecord) -> Result<[u8; 32], Error> {
    let bytes = canonical_bytes(record)?;
    let hash = blake2b_simd::Params::new()
        .hash_length(32)
        .personal(HASH_PERSONALIZATION)
        .hash(&bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(hash.as_bytes());
    Ok(out)
}
