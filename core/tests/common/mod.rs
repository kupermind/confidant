//! Shared helpers for the integration suites: a wallet that syncs from a verifier, a deposit
//! reference the way the API layer would build it, and the three operations end to end.

#![allow(dead_code)]

use confidant_core::{Accepted, DepositRef, Prepared, TransferOutput, Verifier, Wallet};

/// What the API layer hands the verifier after it found the deposit on-ledger: the tx hash, the
/// delivered amount, and the memo (the bundle hash the wallet published).
pub fn deposit_for(prepared: &Prepared, tx_hash: &str, delivered: u64) -> DepositRef {
    DepositRef {
        tx_hash: tx_hash.to_owned(),
        delivered,
        memo_hash: prepared.hash_hex(),
    }
}

pub fn out(recipient: &str, amount: u64) -> TransferOutput {
    TransferOutput {
        recipient: recipient.to_owned(),
        amount,
    }
}

/// Wallet-side build + verifier-side accept for a shield, then sync the wallet.
pub fn shield(v: &mut Verifier, w: &mut Wallet, amount: u64, tx_hash: &str) -> Accepted {
    w.sync(v.records()).unwrap();
    let p = w.build_shield(amount).unwrap();
    let acc = v
        .submit_shield(&p.record, &deposit_for(&p, tx_hash, amount))
        .unwrap();
    w.sync(v.records()).unwrap();
    acc
}

/// Wallet-side build + verifier-side accept for a transfer, then sync both wallets.
pub fn transfer(v: &mut Verifier, from: &mut Wallet, to: &mut Wallet, amount: u64) -> Accepted {
    from.sync(v.records()).unwrap();
    let p = from
        .build_transfer(&[out(&to.info().address, amount)])
        .unwrap();
    let acc = v.submit_transfer(&p.record).unwrap();
    from.sync(v.records()).unwrap();
    to.sync(v.records()).unwrap();
    acc
}

/// Wallet-side build + verifier-side accept for an unshield, then sync the wallet.
pub fn unshield(v: &mut Verifier, w: &mut Wallet, amount: u64, destination: &str) -> Accepted {
    w.sync(v.records()).unwrap();
    let p = w.build_unshield(amount, destination).unwrap();
    let acc = v.submit_unshield(&p.record, destination).unwrap();
    w.sync(v.records()).unwrap();
    acc
}
