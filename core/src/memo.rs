//! The per-operation anchor memo (`confidant/anchor`): what the quorum writes into an XRPL
//! Payment memo after every accepted bundle, so anyone can rebuild the verifier state from the
//! ledger plus a mirror of the bundle bytes (SPEC §v2).
//!
//! ```text
//! version(1) ‖ op(1) ‖ root(32) ‖ bundleHash(32) ‖ extLen(1) ‖ ext ‖ n(1) ‖ n × (nullifier(32) ‖ cmx(32))
//! ```
//!
//! `ext` is the record's external context as UTF-8: the deposit tx hash for a shield, the XRPL
//! destination for an unshield, empty for a transfer. XRPL caps a transaction's memos at 1 KB.

use crate::error::Error;
use crate::record::bundle_hash;
use crate::sighash::OpTag;
use crate::types::BundleRecord;

/// Current memo format version.
pub const VERSION: u8 = 1;
/// XRPL's limit on the serialized memos of one transaction.
pub const MAX_BYTES: usize = 1024;
/// Memo type string, hex-encoded by the XRPL side.
pub const MEMO_TYPE: &str = "confidant/anchor";

/// Decoded anchor memo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorMemo {
    pub op: OpTag,
    /// Root of the commitment tree after this bundle was applied.
    pub root: [u8; 32],
    pub bundle_hash: [u8; 32],
    pub external: String,
    /// `(nullifier, cmx)` per action, in leaf order.
    pub actions: Vec<([u8; 32], [u8; 32])>,
}

fn arr32(s: &str) -> Result<[u8; 32], Error> {
    let v = hex::decode(s).map_err(|_| Error::InvalidEncoding)?;
    v.try_into().map_err(|_| Error::InvalidEncoding)
}

impl AnchorMemo {
    /// Builds the memo for an accepted record and the root it produced.
    pub fn from_record(record: &BundleRecord, root_after: [u8; 32]) -> Result<Self, Error> {
        // A proof of funds is never anchored; refusing here stops endorse before any signature.
        if record.op == OpTag::ProofOfFunds {
            return Err(Error::MemoMalformed("not a pool operation"));
        }
        let mut actions = Vec::with_capacity(record.actions.len());
        for a in &record.actions {
            actions.push((arr32(&a.nullifier)?, arr32(&a.cmx)?));
        }
        Ok(Self {
            op: record.op,
            root: root_after,
            bundle_hash: bundle_hash(record)?,
            external: record.external.clone(),
            actions,
        })
    }

    /// Serialises the memo. Refuses anything over XRPL's 1 KB memo limit.
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let ext = self.external.as_bytes();
        if ext.len() > u8::MAX as usize {
            return Err(Error::MemoMalformed(
                "external context longer than 255 bytes",
            ));
        }
        if self.actions.len() > u8::MAX as usize {
            return Err(Error::MemoMalformed("more than 255 actions"));
        }
        let mut v = Vec::with_capacity(68 + ext.len() + 64 * self.actions.len());
        v.push(VERSION);
        v.push(self.op as u8);
        v.extend_from_slice(&self.root);
        v.extend_from_slice(&self.bundle_hash);
        v.push(ext.len() as u8);
        v.extend_from_slice(ext);
        v.push(self.actions.len() as u8);
        for (nf, cmx) in &self.actions {
            v.extend_from_slice(nf);
            v.extend_from_slice(cmx);
        }
        if v.len() > MAX_BYTES {
            return Err(Error::MemoTooLarge(v.len()));
        }
        Ok(v)
    }

    /// Parses a memo; every length is checked, trailing bytes are refused.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Result<&[u8], Error> {
            let end = at
                .checked_add(n)
                .ok_or(Error::MemoMalformed("length overflow"))?;
            let slice = bytes
                .get(at..end)
                .ok_or(Error::MemoMalformed("truncated"))?;
            at = end;
            Ok(slice)
        };
        if take(1)?[0] != VERSION {
            return Err(Error::MemoMalformed("unknown version"));
        }
        let op = OpTag::from_byte(take(1)?[0]).ok_or(Error::MemoMalformed("unknown op"))?;
        if op == OpTag::ProofOfFunds {
            return Err(Error::MemoMalformed("not a pool operation"));
        }
        let root: [u8; 32] = take(32)?.try_into().expect("32 bytes");
        let bundle_hash: [u8; 32] = take(32)?.try_into().expect("32 bytes");
        let ext_len = take(1)?[0] as usize;
        let external = std::str::from_utf8(take(ext_len)?)
            .map_err(|_| Error::MemoMalformed("external context is not UTF-8"))?
            .to_owned();
        let n = take(1)?[0] as usize;
        let mut actions = Vec::with_capacity(n);
        for _ in 0..n {
            let nf: [u8; 32] = take(32)?.try_into().expect("32 bytes");
            let cmx: [u8; 32] = take(32)?.try_into().expect("32 bytes");
            actions.push((nf, cmx));
        }
        if at != bytes.len() {
            return Err(Error::MemoMalformed("trailing bytes"));
        }
        Ok(Self {
            op,
            root,
            bundle_hash,
            external,
            actions,
        })
    }

    pub fn encode_hex(&self) -> Result<String, Error> {
        Ok(hex::encode(self.encode()?))
    }

    pub fn decode_hex(s: &str) -> Result<Self, Error> {
        Self::decode(&hex::decode(s.trim()).map_err(|_| Error::InvalidEncoding)?)
    }
}
