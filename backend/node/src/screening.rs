//! Boundary screening policy: the cheap form of "proof of innocence" that fits this design. The
//! pool's entry and exit are public XRPL transactions, and every verifier node already reads the
//! deposit (sender account) and the unshield destination from the ledger itself, so each node can
//! refuse — independently, fail-closed — any shield whose depositor, and any unshield whose
//! destination, is on its configured list, and any shield whose deposit is younger than a standby
//! period (so list providers have time to publish). With the gate on since a pool's genesis,
//! "no note descends from a listed address" holds pool-wide and anyone can re-check it from the
//! public log (`backend/quorum/screening/audit.mjs`). It is gatekeeping, not a per-note ZK proof
//! (ROADMAP §4.5.4 is the expensive variant); lists are public data and can be wrong or late.

use std::collections::HashSet;
use std::path::Path;

use crate::address::canonical;

/// The form every comparison uses: the classic address for anything that decodes (X-addresses
/// lose their tag), the raw trimmed string otherwise (an operator's typo never matches anything).
fn canon(s: &str) -> String {
    let t = s.trim();
    canonical(t).unwrap_or_else(|| t.to_owned())
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ScreenError {
    #[error("account {0} is on the screening list")]
    Blocked(String),
    #[error("deposit in ledger {deposit_ledger} is younger than the standby of {standby} ledgers (validated ledger {validated}); retry at ledger {}", deposit_ledger + standby)]
    Standby {
        deposit_ledger: u64,
        validated: u64,
        standby: u64,
    },
}

/// The policy one node enforces. `off()` = no list, no standby: the gate is inert.
#[derive(Clone, Debug, Default)]
pub struct Screening {
    blocked: HashSet<String>,
    standby_ledgers: u64,
    /// Entries that did not decode as an XRPL address: kept (they match only themselves) but
    /// inert against any real account, so the operator must be told about them at startup.
    undecodable: usize,
}

impl Screening {
    pub fn off() -> Self {
        Self::default()
    }

    pub fn new(blocked: impl IntoIterator<Item = String>, standby_ledgers: u64) -> Self {
        let mut set = HashSet::new();
        let mut undecodable = 0;
        for raw in blocked {
            let t = raw.trim();
            if t.is_empty() {
                continue;
            }
            if canonical(t).is_none() {
                undecodable += 1;
            }
            set.insert(canon(t));
        }
        Self {
            blocked: set,
            standby_ledgers,
            undecodable,
        }
    }

    /// One XRPL address per line (`#` comments allowed) or a JSON array of strings.
    pub fn from_file(path: &Path, standby_ledgers: u64) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let entries: Vec<String> = if text.trim_start().starts_with('[') {
            serde_json::from_str(&text).map_err(std::io::Error::other)?
        } else {
            text.lines()
                .map(|l| l.split('#').next().unwrap_or("").trim().to_owned())
                .collect()
        };
        Ok(Self::new(entries, standby_ledgers))
    }

    pub fn is_active(&self) -> bool {
        !self.blocked.is_empty() || self.standby_ledgers > 0
    }

    pub fn listed_count(&self) -> usize {
        self.blocked.len()
    }

    /// Entries that are not valid XRPL addresses and therefore block nobody.
    pub fn undecodable_count(&self) -> usize {
        self.undecodable
    }

    pub fn standby_ledgers(&self) -> u64 {
        self.standby_ledgers
    }

    /// Listed in any spelling: the account is canonicalized before the lookup, so a listed
    /// classic address is caught when supplied as an X-address (with or without a tag), and vice
    /// versa.
    pub fn is_listed(&self, account: &str) -> bool {
        self.blocked.contains(&canon(account))
    }

    /// A shield's deposit: the sender must not be listed and the deposit must be old enough.
    pub fn check_deposit(
        &self,
        account: &str,
        deposit_ledger: u64,
        validated: u64,
    ) -> Result<(), ScreenError> {
        if self.is_listed(account) {
            return Err(ScreenError::Blocked(account.to_owned()));
        }
        if validated < deposit_ledger.saturating_add(self.standby_ledgers) {
            return Err(ScreenError::Standby {
                deposit_ledger,
                validated,
                standby: self.standby_ledgers,
            });
        }
        Ok(())
    }

    /// An unshield's payout destination must not be listed.
    pub fn check_destination(&self, destination: &str) -> Result<(), ScreenError> {
        if self.is_listed(destination) {
            return Err(ScreenError::Blocked(destination.to_owned()));
        }
        Ok(())
    }
}
