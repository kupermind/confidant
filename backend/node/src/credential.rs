//! Credential gate (XLS-70, hackathon feature A): a shield is accepted only if the depositing
//! account holds a Credential on the ledger from the pool's trusted issuer, of the configured type,
//! **accepted** by the subject (that signature binds the account) and not expired. Every node reads
//! the entry from the ledger itself (`ledger_entry` `credential`) at submit and again at endorse;
//! nothing is taken from the client. Honest label: this is gatekeeping on a trusted issuer's say-so
//! at the public boundary — it says nothing about the holder's notes, and an issuer vouches for
//! whoever it chooses to.
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::address::canonical;

/// `lsfAccepted` on a Credential ledger entry: the subject ran `CredentialAccept`.
pub const LSF_ACCEPTED: u64 = 0x0001_0000;
/// Seconds between the Unix epoch and the Ripple epoch (2000-01-01).
pub const RIPPLE_EPOCH: u64 = 946_684_800;

/// What the gate requires, as the operator configured it and as `GET /policy` reports it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialPolicy {
    /// Canonical classic address of the trusted issuer.
    pub issuer: String,
    /// `CredentialType` as upper-case hex (1–64 bytes), exactly as it is on the ledger.
    #[serde(rename = "type")]
    pub credential_type: String,
    /// The type decoded as text when it is printable (for messages and the UI), else `None`.
    pub type_text: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    /// The depositor holds no usable credential; the message names issuer, type and account.
    #[error("no accepted credential from {issuer} of type {type_text} for {account}: {why}")]
    Missing {
        issuer: String,
        type_text: String,
        account: String,
        why: &'static str,
    },
    /// The configured policy itself is invalid.
    #[error("credential policy: {0}")]
    Policy(String),
}

#[derive(Clone, Debug, Default)]
pub struct CredentialGate {
    policy: Option<CredentialPolicy>,
}

impl CredentialGate {
    /// No gate: every deposit the core accepts is accepted.
    pub fn off() -> Self {
        Self { policy: None }
    }

    /// A gate for `issuer` (classic or X-address, canonicalized) and `credential_type` (hex).
    pub fn new(
        issuer: &str,
        credential_type: &str,
        type_text: Option<&str>,
    ) -> Result<Self, CredentialError> {
        let issuer = canonical(issuer)
            .ok_or_else(|| CredentialError::Policy(format!("issuer {issuer} is not an address")))?;
        let t = credential_type.trim().to_uppercase();
        if t.is_empty()
            || !t.len().is_multiple_of(2)
            || t.len() > 128
            || !t.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(CredentialError::Policy(
                "credential type must be 1–64 bytes as hex".into(),
            ));
        }
        let type_text = type_text.map(str::to_owned).or_else(|| {
            hex::decode(&t)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .filter(|s| !s.is_empty() && s.chars().all(|c| !c.is_control()))
        });
        Ok(Self {
            policy: Some(CredentialPolicy {
                issuer,
                credential_type: t,
                type_text,
            }),
        })
    }

    pub fn is_active(&self) -> bool {
        self.policy.is_some()
    }

    pub fn policy(&self) -> Option<&CredentialPolicy> {
        self.policy.as_ref()
    }

    /// Judges the ledger's answer for `account`'s credential (`None` = no such entry) at `now`
    /// (seconds since the Ripple epoch). With the gate off, everything passes.
    pub fn check(
        &self,
        account: &str,
        entry: Option<&Value>,
        now: u64,
    ) -> Result<(), CredentialError> {
        let Some(p) = &self.policy else {
            return Ok(());
        };
        let refuse = |why: &'static str| {
            Err(CredentialError::Missing {
                issuer: p.issuer.clone(),
                type_text: p
                    .type_text
                    .clone()
                    .unwrap_or_else(|| p.credential_type.clone()),
                account: account.to_owned(),
                why,
            })
        };
        let Some(e) = entry else {
            return refuse("none on the ledger");
        };
        // The lookup is keyed by (subject, issuer, type); an answer for anything else is refused,
        // whatever the RPC did.
        let field = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("");
        if field("Subject") != account
            || field("Issuer") != p.issuer
            || !field("CredentialType").eq_ignore_ascii_case(&p.credential_type)
        {
            return refuse("the ledger answered for another credential");
        }
        let flags = e.get("Flags").and_then(Value::as_u64).unwrap_or(0);
        if flags & LSF_ACCEPTED == 0 {
            return refuse("issued but not accepted by the subject");
        }
        if let Some(exp) = e.get("Expiration").and_then(Value::as_u64) {
            if exp <= now {
                return refuse("expired");
            }
        }
        Ok(())
    }
}

/// Whether a `ledger_entry` error means "no such entry" (an ordinary answer: the account holds no
/// such credential) rather than a failure of the lookup. rippled reports it as the error code
/// `entryNotFound` with the message "Entry not found."; the RPC layer may surface either.
pub fn is_entry_not_found(rpc_error: &str) -> bool {
    rpc_error.contains("entryNotFound")
        || rpc_error.to_ascii_lowercase().contains("entry not found")
}

/// This host's clock in seconds since the Ripple epoch. **Not** what the gate judges `Expiration`
/// against — the node uses the validated ledger's `close_time` for that; this is for logs, tooling
/// and tests that need a plausible "now".
pub fn ripple_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .saturating_sub(RIPPLE_EPOCH)
}
