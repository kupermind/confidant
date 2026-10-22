//! The protocol fee (Railgun's model, parameters on the server): a configurable share, in basis
//! points, of every **shield** and every **unshield** — the two operations whose amounts are public.
//! Private transfers carry no fee (their amounts are hidden; Railgun does not fee them either).
//!
//! The fee never leaves the pool account by itself: a shield's deposit is credited to the note
//! *net* of the fee, an unshield's payout is the note *net* of the fee, and every node keeps a
//! ledger of what accrued. The quorum moves accrued fees to the treasury account with a
//! multisigned `fee-sweep` Payment that each signer sidecar checks against **its own** node's
//! ledger (destination = the configured treasury, amount ≤ available). Both the accrual and the
//! sweeps are replayable from the public log and the ledger, so a rebuilt node lands on the same
//! numbers.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::address::canonical;

/// A sanity cap: no deployment of this code takes more than 10 % of an operation.
pub const MAX_BPS: u64 = 1_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeePolicy {
    pub shield_bps: u64,
    pub unshield_bps: u64,
    /// Canonical classic address of the treasury; required as soon as any fee is non-zero.
    pub treasury: Option<String>,
}

fn share(gross: u64, bps: u64) -> u64 {
    ((gross as u128 * bps as u128) / 10_000) as u64
}

impl FeePolicy {
    pub fn off() -> Self {
        Self::default()
    }

    pub fn new(shield_bps: u64, unshield_bps: u64, treasury: Option<&str>) -> Result<Self, String> {
        if shield_bps > MAX_BPS || unshield_bps > MAX_BPS {
            return Err(format!("a fee above {MAX_BPS} bps (10 %) is refused"));
        }
        let treasury = treasury.map(str::trim).filter(|t| !t.is_empty());
        let treasury = match treasury {
            Some(t) => {
                Some(canonical(t).ok_or_else(|| format!("treasury {t} is not an XRPL address"))?)
            }
            None => None,
        };
        if (shield_bps > 0 || unshield_bps > 0) && treasury.is_none() {
            return Err("a non-zero fee needs --fee-treasury <rAddress>".into());
        }
        Ok(Self {
            shield_bps,
            unshield_bps,
            treasury,
        })
    }

    pub fn is_active(&self) -> bool {
        self.shield_bps > 0 || self.unshield_bps > 0
    }

    /// Fee on a deposit of `gross` drops; the note is credited with `gross - fee`.
    pub fn shield_fee(&self, gross: u64) -> u64 {
        share(gross, self.shield_bps)
    }

    /// Fee on an unshield of `gross` drops (the note's value); the payout is `gross - fee`.
    pub fn unshield_fee(&self, gross: u64) -> u64 {
        share(gross, self.unshield_bps)
    }
}

/// What this node has accounted: fees accrued by accepted operations and fees already moved to
/// the treasury (by tx hash, so a sweep is counted once).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeeLedger {
    pub accrued: u64,
    pub swept: u64,
    #[serde(default)]
    pub sweeps: BTreeSet<String>,
}

impl FeeLedger {
    pub fn available(&self) -> u64 {
        self.accrued.saturating_sub(self.swept)
    }
}

/// `GET /fees`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeeReport {
    pub shield_bps: u64,
    pub unshield_bps: u64,
    /// The pool account's validated XRP balance minus the value of every note it backs, read from
    /// the ledger — an upper bound on what may leave the pool that does not depend on this fee
    /// ledger. `None` when the ledger could not be read (then nothing is sweepable).
    #[serde(default)]
    pub pool_excess: Option<u64>,
    /// The value of every note the pool still backs, in drops: shields in minus unshields out,
    /// from the public value balances of the accepted bundles. Public by construction (every
    /// shield and unshield is a Payment on the ledger); only how it is split among holders is
    /// private. Shown by the UI as the pool total.
    #[serde(default)]
    pub outstanding_backing: u64,
    pub treasury: Option<String>,
    pub accrued: u64,
    pub swept: u64,
    pub available: u64,
}
