//! Apply-path signature safety and the hash-domain guard — the step the audit session asked to
//! land before any proof-of-funds work (2026-10-14), because it protects the pool as it is today.
//!
//! 1. Every entrypoint that can change a verifier's state re-verifies BOTH the spend-auth
//!    signatures and the binding signature under the POOL sighash before it consumes a nullifier
//!    or appends a leaf. A structurally valid Orchard bundle whose signatures are over a FOREIGN
//!    sighash (another BLAKE2b personalization — the shape a proof-of-funds record would have) is
//!    refused at every entrypoint, a genuine bundle with a swapped binding signature likewise, and
//!    the verifier is byte-identical afterwards. The honest counterparts are then accepted, so the
//!    refusals are due to the signatures and nothing else.
//! 2. Every BLAKE2b personalization and every sighash context in `core/src` is enumerated and
//!    unique. A new hash domain must be added to the allow-list below on purpose, so two domains
//!    can never silently share a personalization.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use confidant_core::keys::{spending_key_from_hex, OwnerKeys};
use confidant_core::orchard::builder::{Builder, BundleType};
use confidant_core::orchard::bundle::TxVersion;
use confidant_core::orchard::keys::Scope;
use confidant_core::orchard::note::Note;
use confidant_core::orchard::note_encryption::OrchardDomain;
use confidant_core::orchard::value::NoteValue;
use confidant_core::orchard::Address;
use confidant_core::record::{bundle_from_record, bundle_hash, record_from_bundle};
use confidant_core::{
    AnchorMemo, BundleRecord, DepositRef, Error, OpTag, Submission, Verifier, Wallet,
};
use rand::rngs::OsRng;
use zcash_note_encryption::try_note_decryption;

const MEMO: [u8; 512] = [0u8; 512];
/// A hash domain no pool operation computes — what a proof-of-funds (or any other non-operation)
/// record would be signed under.
const FOREIGN_PERSONALIZATION: &[u8; 16] = b"ForeignDomain_01";

fn foreign_sighash(commitment: &[u8; 32], ctx: &[u8]) -> [u8; 32] {
    let hash = blake2b_simd::Params::new()
        .hash_length(32)
        .personal(FOREIGN_PERSONALIZATION)
        .to_state()
        .update(commitment)
        .update(ctx)
        .finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(hash.as_bytes());
    out
}

struct Owned {
    note: Note,
    position: u64,
}

/// The holder's unspent notes, found exactly the way a wallet finds them: trial decryption of
/// the public log, minus the notes whose nullifiers the log already shows spent.
fn owned_notes(v: &Verifier, keys: &OwnerKeys) -> Vec<Owned> {
    let spent: HashSet<String> = v
        .records()
        .iter()
        .flat_map(|r| r.actions.iter().map(|a| a.nullifier.to_lowercase()))
        .collect();
    let mut out = Vec::new();
    for r in v.records() {
        let bundle = bundle_from_record(r, v.params().version).unwrap();
        for (i, action) in bundle.actions().iter().enumerate() {
            let domain = OrchardDomain::for_action(action);
            let found = keys
                .prepared_ivks()
                .iter()
                .find_map(|ivk| try_note_decryption(&domain, *ivk, action));
            if let Some((note, _addr, _memo)) = found {
                let nf = hex::encode(note.nullifier(keys.full_viewing_key()).to_bytes());
                if !spent.contains(&nf) {
                    out.push(Owned {
                        note,
                        position: r.first_leaf + i as u64,
                    });
                }
            }
        }
    }
    out
}

/// A genuine Orchard bundle over the verifier's current anchor — spends of the holder's notes,
/// the given outputs, value balance = `expected_vb` — proven with the pool's proving key but SIGNED
/// over the foreign sighash. Everything but the signature domain is exactly what the pool accepts.
fn foreign_record(
    v: &Verifier,
    keys: &OwnerKeys,
    spends: &[&Owned],
    outputs: &[(Address, u64, Scope)],
    expected_vb: i64,
    op: OpTag,
    external: &str,
) -> BundleRecord {
    let version = v.params().version;
    let mut builder = Builder::new(
        BundleType::DEFAULT,
        version,
        version.default_flags(),
        v.current_anchor(),
    )
    .unwrap();
    for s in spends {
        builder
            .add_spend(
                keys.full_viewing_key().clone(),
                s.note,
                v.witness(s.position).unwrap(),
            )
            .unwrap();
    }
    for (addr, value, scope) in outputs {
        builder
            .add_output(
                Some(keys.ovk(*scope)),
                *addr,
                NoteValue::from_raw(*value),
                MEMO,
            )
            .unwrap();
    }
    let (unauthorized, _meta) = builder.build::<i64>(OsRng).unwrap().unwrap();
    assert_eq!(*unauthorized.value_balance(), expected_vb);
    let commitment: [u8; 32] = unauthorized.commitment(TxVersion::V5).unwrap().into();
    let sh = foreign_sighash(&commitment, b"lender-nonce|rBorrower|ledger 1234");
    let proven = unauthorized.create_proof(v.params().pk(), OsRng).unwrap();
    let bundle = proven
        .apply_signatures(OsRng, sh, &[keys.spend_authorizing_key()])
        .unwrap();
    record_from_bundle(&bundle, 0, op, external, 0, 0)
}

/// Everything observable about a verifier: the public state and the whole log.
fn fingerprint(v: &Verifier) -> String {
    format!(
        "{}|{}",
        serde_json::to_string(&v.state()).unwrap(),
        serde_json::to_string(&v.snapshot()).unwrap()
    )
}

fn deposit(record: &BundleRecord, tx_hash: &str, delivered: u64) -> DepositRef {
    DepositRef {
        tx_hash: tx_hash.to_owned(),
        delivered,
        memo_hash: hex::encode(bundle_hash(record).unwrap()),
    }
}

/// Positions the record as the next log entry, the way a trusted log or a mirror would present it.
fn as_next_entry(v: &Verifier, record: &BundleRecord) -> BundleRecord {
    let mut r = record.clone();
    r.index = v.records().len();
    r.first_leaf = v.state().leaf_count;
    r
}

/// Runs one record through every apply entrypoint of the verifier and asserts the same refusal
/// everywhere, with the verifier unchanged. `shape` says which submit/endorse paths fit the
/// record (its value-balance sign); the log-driven paths (replay, restore, rebuild, rebuild_checked)
/// take every shape.
fn refused_everywhere(
    v: &mut Verifier,
    record: &BundleRecord,
    shape: OpTag,
    deposits: &BTreeMap<String, DepositRef>,
    expect: fn(&Error) -> bool,
) {
    let before = fingerprint(v);
    let dep = deposit(
        record,
        &record.external,
        (-record.value_balance).max(0) as u64,
    );
    let memo = AnchorMemo::from_record(record, [0u8; 32]).unwrap();
    fn refusal<T>(what: &str, r: Result<T, Error>, expect: fn(&Error) -> bool) {
        match r {
            Err(e) if expect(&e) => {}
            Err(e) => panic!("{what}: expected the signature refusal, got {e:?}"),
            Ok(_) => {
                panic!("{what}: ACCEPTED a bundle whose signatures are not under the pool sighash")
            }
        }
    }
    match shape {
        OpTag::ProofOfFunds => unreachable!("not an apply-path shape"),
        OpTag::Shield => {
            refusal("submit_shield", v.submit_shield(record, &dep), expect);
            refusal(
                "endorse(shield)",
                v.endorse(
                    &Submission::Shield {
                        record: record.clone(),
                        deposit: dep.clone(),
                    },
                    &memo,
                ),
                expect,
            );
        }
        OpTag::Transfer => {
            refusal("submit_transfer", v.submit_transfer(record), expect);
            refusal(
                "endorse(transfer)",
                v.endorse(
                    &Submission::Transfer {
                        record: record.clone(),
                    },
                    &memo,
                ),
                expect,
            );
        }
        OpTag::Unshield => {
            refusal(
                "submit_unshield",
                v.submit_unshield(record, &record.external),
                expect,
            );
            refusal(
                "endorse(unshield)",
                v.endorse(
                    &Submission::Unshield {
                        record: record.clone(),
                        destination: record.external.clone(),
                    },
                    &memo,
                ),
                expect,
            );
        }
    }
    // The log-driven paths: a trusted log, a snapshot, and the ledger's anchor memos + a mirror.
    let entry = as_next_entry(v, record);
    refusal("replay", v.replay(&entry), expect);
    let mut snap = v.snapshot();
    snap.records.push(entry.clone());
    refusal("restore", Verifier::restore(&snap), expect);
    let mut memos: Vec<AnchorMemo> = (0..v.records().len())
        .map(|i| v.anchor_memo(i).unwrap())
        .collect();
    memos.push(AnchorMemo::from_record(&entry, [0u8; 32]).unwrap());
    let mut all: Vec<BundleRecord> = v.records().to_vec();
    all.push(entry.clone());
    let mut mirror = |h: &[u8; 32]| {
        all.iter()
            .find(|r| bundle_hash(r).ok() == Some(*h))
            .cloned()
    };
    refusal("rebuild", Verifier::rebuild(&memos, &mut mirror), expect);
    let mut lookup = |tx: &str| {
        deposits
            .get(tx)
            .cloned()
            .or_else(|| (tx == entry.external).then(|| dep.clone()))
    };
    refusal(
        "rebuild_checked",
        Verifier::rebuild_checked(&memos, &mut mirror, &mut lookup),
        expect,
    );
    assert_eq!(
        fingerprint(v),
        before,
        "a refusal must leave the verifier untouched"
    );
}

#[test]
fn both_signatures_are_re_verified_under_the_pool_sighash_at_every_apply_path() {
    let mut v = Verifier::new();
    let mut alice = Wallet::new();
    shield(&mut v, &mut alice, 100, "D1");
    shield(&mut v, &mut alice, 60, "D2");
    let deposits: BTreeMap<String, DepositRef> = [("D1", 100u64, 0usize), ("D2", 60, 1)]
        .into_iter()
        .map(|(tx, amount, i)| (tx.to_owned(), deposit(&v.records()[i], tx, amount)))
        .collect();
    let keys =
        OwnerKeys::from_spending_key(spending_key_from_hex(&alice.secret().spending_key).unwrap());
    let notes = owned_notes(&v, &keys);
    assert_eq!(
        notes.len(),
        2,
        "both shielded notes found by trial decryption"
    );
    let big = notes
        .iter()
        .find(|n| n.note.value().inner() == 100)
        .unwrap();
    let small = notes.iter().find(|n| n.note.value().inner() == 60).unwrap();

    // 1. Foreign sighash: three shapes, each structurally a valid pool operation (sign of the
    //    value balance, known anchor, unspent nullifiers, valid proof), signed under another domain.
    let unshield_like = foreign_record(
        &v,
        &keys,
        &[big],
        &[(keys.change_address(), 60, Scope::Internal)],
        40,
        OpTag::Unshield,
        "rDest",
    );
    let transfer_like = foreign_record(
        &v,
        &keys,
        &[small],
        &[(keys.address(), 60, Scope::External)],
        0,
        OpTag::Transfer,
        "",
    );
    let shield_like = foreign_record(
        &v,
        &keys,
        &[],
        &[(keys.address(), 25, Scope::External)],
        -25,
        OpTag::Shield,
        "DX",
    );
    assert_eq!(shield_like.value_balance, -25);
    let spend_auth = |e: &Error| matches!(e, Error::InvalidSpendAuth);
    refused_everywhere(
        &mut v,
        &unshield_like,
        OpTag::Unshield,
        &deposits,
        spend_auth,
    );
    refused_everywhere(
        &mut v,
        &transfer_like,
        OpTag::Transfer,
        &deposits,
        spend_auth,
    );
    refused_everywhere(&mut v, &shield_like, OpTag::Shield, &deposits, spend_auth);

    // 2. Genuine bundles with a swapped binding signature: the spend-auth signatures pass, the
    //    binding signature is checked separately and refuses — at every entrypoint.
    let pu = alice.build_unshield(40, "rDest").unwrap();
    let pt = alice
        .build_transfer(&[out(&alice.info().address, 60)])
        .unwrap();
    let ps = alice.build_shield(25).unwrap();
    let swap = |r: &BundleRecord, other: &BundleRecord, external: &str| {
        let mut bad = r.clone();
        bad.binding_signature = other.binding_signature.clone();
        bad.external = external.to_owned();
        bad
    };
    let binding = |e: &Error| matches!(e, Error::InvalidBindingSignature);
    refused_everywhere(
        &mut v,
        &swap(&pu.record, &pt.record, "rDest"),
        OpTag::Unshield,
        &deposits,
        binding,
    );
    refused_everywhere(
        &mut v,
        &swap(&pt.record, &pu.record, ""),
        OpTag::Transfer,
        &deposits,
        binding,
    );
    refused_everywhere(
        &mut v,
        &swap(&ps.record, &pu.record, "DX"),
        OpTag::Shield,
        &deposits,
        binding,
    );

    // 3. The honest counterparts are accepted: the only thing that was wrong above was a signature.
    v.submit_unshield(&pu.record, "rDest").unwrap();
    v.submit_shield(&ps.record, &deposit_for(&ps, "DX", 25))
        .unwrap();
    assert_eq!(v.state().bundle_count, 4);
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Completeness guard on hash domains: enumerates every BLAKE2b personalization and every sighash
/// context function in `core/src`, by reading the sources, and pins them to an allow-list. Scope:
/// `core/src` is where every bundle sighash lives today; if another crate (node, sidecar, CLI) ever
/// computes one, extend the walk to it (audit note, 2026-10-14, ROADMAP §4.3b). Adding
/// a domain (e.g. a proof-of-funds sighash) means extending the list here, on purpose — a new
/// `.personal(...)` or `*_ctx` that is not listed, a personalization shared by two call sites, or a
/// BLAKE2b without a personalization fails this test.
#[test]
fn every_blake2b_personalization_and_sighash_context_in_core_is_enumerated_and_unique() {
    // The proof-of-funds domain was added on purpose (hackathon feature C, 2026-10-24): a bundle
    // signed under it verifies under no pool sighash, and every apply path refuses its op first.
    const PERSONALIZATIONS: &[&str] = &["ConfidantBundle", "ConfidantPoF_v1_", "ConfidantSigHash"];
    const SIGHASH_CONTEXTS: &[&str] = &["external_ctx", "pof_ctx", "unshield_ctx"];

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(
        files.len() >= 10,
        "unexpectedly few source files: {files:?}"
    );

    let mut consts: BTreeMap<String, (String, String)> = BTreeMap::new(); // ident → (file, literal)
    let mut personal_sites: Vec<(String, String)> = Vec::new(); // (file, ident)
    let mut blake_sites = 0usize;
    let mut ctx_fns: BTreeSet<String> = BTreeSet::new();
    for path in &files {
        let file = path.strip_prefix(&src).unwrap().display().to_string();
        let text = fs::read_to_string(path).unwrap();
        for line in text.lines().map(str::trim) {
            if line.starts_with("//") {
                continue;
            }
            // `pub const NAME: &[u8; N] = b"literal";`
            if let Some(rest) = line.strip_prefix("pub const ") {
                if let (Some(colon), Some(eq)) = (rest.find(':'), rest.find("= b\"")) {
                    if rest[colon..eq].contains("&[u8;") {
                        let ident = rest[..colon].trim().to_owned();
                        let lit = rest[eq + 4..].split("\";").next().unwrap().to_owned();
                        assert!(
                            consts.insert(ident.clone(), (file.clone(), lit)).is_none(),
                            "{ident} defined twice"
                        );
                    }
                }
            }
            blake_sites += line.matches("blake2b_simd::Params::new()").count();
            if let Some(i) = line.find(".personal(") {
                let arg = line[i + 10..].split(')').next().unwrap().trim();
                personal_sites.push((file.clone(), arg.to_owned()));
            }
            if file == "sighash.rs" {
                if let Some(rest) = line.strip_prefix("pub fn ") {
                    let name = rest.split('(').next().unwrap();
                    if name.ends_with("_ctx") {
                        ctx_fns.insert(name.to_owned());
                    }
                }
            }
        }
    }

    // Every BLAKE2b in the core is personalized, and every personalization is a named constant.
    assert_eq!(
        blake_sites,
        personal_sites.len(),
        "a BLAKE2b hash without .personal(): {personal_sites:?}"
    );
    let mut domains: BTreeMap<String, Vec<String>> = BTreeMap::new(); // literal → call sites
    for (file, ident) in &personal_sites {
        let (def_file, lit) = consts
            .get(ident)
            .unwrap_or_else(|| panic!("{file}: .personal({ident}) is not a byte-string constant"));
        assert_eq!(def_file, file, "{ident} must be defined where it is used");
        domains.entry(lit.clone()).or_default().push(file.clone());
    }
    let found: Vec<&str> = domains.keys().map(String::as_str).collect();
    assert_eq!(
        found, PERSONALIZATIONS,
        "the set of hash domains changed — extend the allow-list on purpose"
    );
    for (lit, sites) in &domains {
        assert_eq!(
            sites.len(),
            1,
            "personalization {lit:?} is shared by {sites:?}"
        );
        assert!(
            lit.len() <= 16,
            "{lit:?} exceeds BLAKE2b's 16-byte personalization"
        );
    }
    // No two domains are prefixes of each other once zero-padded to 16 bytes.
    let padded: BTreeSet<[u8; 16]> = domains
        .keys()
        .map(|l| {
            let mut p = [0u8; 16];
            p[..l.len()].copy_from_slice(l.as_bytes());
            p
        })
        .collect();
    assert_eq!(padded.len(), domains.len());
    // The sighash context functions are exactly the known ones (one per external-context shape).
    let ctx: Vec<&str> = ctx_fns.iter().map(String::as_str).collect();
    assert_eq!(ctx, SIGHASH_CONTEXTS, "sighash context shapes changed");
}
