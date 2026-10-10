//! The node's logic, independent of HTTP: a [`Verifier`] behind a mutex, the ledger it reads, and
//! the operations the endpoints expose.

use std::path::PathBuf;
use std::sync::Mutex;

use confidant_core::memo::AnchorMemo;
use confidant_core::record::bundle_hash;
use confidant_core::{
    Accepted, BundleRecord, DepositRef, Error as CoreError, Payout, PoolState, RevealReport,
    Submission, Verifier, VerifierSnapshot,
};
use serde::{Deserialize, Serialize};

use crate::screening::Screening;
use crate::xrpl::{
    anchor_fact, anchor_memos, check_custody_info, deposit_facts, deposit_source,
    ensure_full_history, Ledger, LedgerError,
};

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error("state file: {0}")]
    Io(String),
    /// Refused by this node's boundary screening policy (list or standby); HTTP 403.
    #[error("refused by screening policy: {0}")]
    Screened(String),
}

/// What `submit_*` and `endorse` return: the core's `Accepted` plus the anchor memo the quorum
/// must now write (hex), so a coordinator needs no second call.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedWithMemo {
    #[serde(flatten)]
    pub accepted: Accepted,
    pub memo_hex: String,
}

pub struct Node<L: Ledger> {
    verifier: Mutex<Verifier>,
    ledger: L,
    pool: String,
    state_path: Option<PathBuf>,
    screening: Screening,
}

impl<L: Ledger> Node<L> {
    pub fn new(
        verifier: Verifier,
        ledger: L,
        pool: impl Into<String>,
        state_path: Option<PathBuf>,
    ) -> Self {
        Self {
            verifier: Mutex::new(verifier),
            ledger,
            pool: pool.into(),
            state_path,
            screening: Screening::off(),
        }
    }

    /// Installs the boundary screening policy this node enforces (default: off).
    pub fn with_screening(mut self, screening: Screening) -> Self {
        self.screening = screening;
        self
    }

    pub fn screening(&self) -> &Screening {
        &self.screening
    }

    pub fn pool(&self) -> &str {
        &self.pool
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Verifier> {
        self.verifier.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist(&self, v: &Verifier) -> Result<(), NodeError> {
        if let Some(p) = &self.state_path {
            if let Some(dir) = p.parent() {
                if !dir.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir).map_err(|e| NodeError::Io(e.to_string()))?;
                }
            }
            let text =
                serde_json::to_string(&v.snapshot()).map_err(|e| NodeError::Io(e.to_string()))?;
            std::fs::write(p, text).map_err(|e| NodeError::Io(e.to_string()))?;
        }
        Ok(())
    }

    fn with_memo(v: &Verifier, accepted: Accepted) -> Result<AcceptedWithMemo, NodeError> {
        let memo_hex = v.anchor_memo(accepted.bundle_index)?.encode_hex()?;
        Ok(AcceptedWithMemo { accepted, memo_hex })
    }

    // ----- reads ---------------------------------------------------------------------------

    pub fn state(&self) -> PoolState {
        self.lock().state()
    }

    pub fn log(&self, from: usize) -> Vec<BundleRecord> {
        let v = self.lock();
        v.records().get(from..).unwrap_or(&[]).to_vec()
    }

    pub fn reveal(&self, viewing_key_hex: &str) -> Result<RevealReport, NodeError> {
        Ok(self.lock().reveal(viewing_key_hex)?)
    }

    /// The deposit facts for `tx_hash`, read from the ledger by this node itself.
    pub async fn deposit(&self, tx_hash: &str) -> Result<DepositRef, NodeError> {
        let tx = self.ledger.tx(tx_hash).await?;
        Ok(deposit_facts(&tx, tx_hash, &self.pool)?)
    }

    /// The deposit facts plus the screening gate: the sender account and the deposit's ledger are
    /// read from the same ledger answer, never from the client; a listed sender or a deposit
    /// younger than the standby is refused before the core sees the bundle.
    async fn screened_deposit(&self, tx_hash: &str) -> Result<DepositRef, NodeError> {
        let tx = self.ledger.tx(tx_hash).await?;
        let deposit = deposit_facts(&tx, tx_hash, &self.pool)?;
        if self.screening.is_active() {
            let (account, deposit_ledger) = deposit_source(&tx)?;
            let validated = if self.screening.standby_ledgers() > 0 {
                self.ledger.validated_ledger_index().await?
            } else {
                u64::MAX
            };
            self.screening
                .check_deposit(&account, deposit_ledger, validated)
                .map_err(|e| NodeError::Screened(e.to_string()))?;
        }
        Ok(deposit)
    }

    fn screened_destination(&self, destination: &str) -> Result<(), NodeError> {
        self.screening
            .check_destination(destination)
            .map_err(|e| NodeError::Screened(e.to_string()))
    }

    // ----- submissions ---------------------------------------------------------------------

    pub async fn submit_shield(
        &self,
        record: &BundleRecord,
        deposit_tx_hash: &str,
    ) -> Result<AcceptedWithMemo, NodeError> {
        let deposit = self.screened_deposit(deposit_tx_hash).await?;
        let mut v = self.lock();
        let acc = v.submit_shield(record, &deposit)?;
        self.persist(&v)?;
        Self::with_memo(&v, acc)
    }

    pub fn submit_transfer(&self, record: &BundleRecord) -> Result<AcceptedWithMemo, NodeError> {
        let mut v = self.lock();
        let acc = v.submit_transfer(record)?;
        self.persist(&v)?;
        Self::with_memo(&v, acc)
    }

    pub fn submit_unshield(
        &self,
        record: &BundleRecord,
        destination: &str,
    ) -> Result<AcceptedWithMemo, NodeError> {
        self.screened_destination(destination)?;
        let mut v = self.lock();
        let acc = v.submit_unshield(record, destination)?;
        self.persist(&v)?;
        Self::with_memo(&v, acc)
    }

    /// A signer's duty. For a shield the node **re-reads the deposit from the ledger itself** and
    /// ignores the facts the coordinator sent (only the tx hash is taken from the submission).
    /// Idempotent: if this exact bundle is already the node's last record and the proposed memo is
    /// the one the node produced, the same answer is returned without re-applying anything, so a
    /// coordinator may retry to collect a signature again.
    pub async fn endorse(
        &self,
        submission: &Submission,
        proposed_memo_hex: &str,
    ) -> Result<AcceptedWithMemo, NodeError> {
        let proposed = AnchorMemo::decode_hex(proposed_memo_hex)?;
        let submission = match submission {
            Submission::Shield { record, deposit } => Submission::Shield {
                record: record.clone(),
                deposit: self.screened_deposit(&deposit.tx_hash).await?,
            },
            Submission::Unshield {
                record,
                destination,
            } => {
                self.screened_destination(destination)?;
                Submission::Unshield {
                    record: record.clone(),
                    destination: destination.clone(),
                }
            }
            other => other.clone(),
        };
        let (record, op, external) = match &submission {
            Submission::Shield { record, deposit } => (
                record,
                confidant_core::OpTag::Shield,
                deposit.tx_hash.as_str(),
            ),
            Submission::Transfer { record } => (record, confidant_core::OpTag::Transfer, ""),
            Submission::Unshield {
                record,
                destination,
            } => (
                record,
                confidant_core::OpTag::Unshield,
                destination.as_str(),
            ),
        };
        let mut v = self.lock();
        if let Some(already) = Self::already_applied(&v, record, op, external, &proposed)? {
            return Self::with_memo(&v, already);
        }
        let acc = v.endorse(&submission, &proposed)?;
        self.persist(&v)?;
        Self::with_memo(&v, acc)
    }

    /// If the submission (bundle, op and external context) is exactly the verifier's last accepted
    /// bundle and `proposed` is the memo it produced, reconstructs that acceptance; otherwise
    /// `None`. A retry with a different destination or deposit is NOT the same submission.
    fn already_applied(
        v: &Verifier,
        record: &BundleRecord,
        op: confidant_core::OpTag,
        external: &str,
        proposed: &AnchorMemo,
    ) -> Result<Option<Accepted>, NodeError> {
        let Some(last) = v.records().last() else {
            return Ok(None);
        };
        if last.op != op || last.external != external || bundle_hash(last)? != bundle_hash(record)?
        {
            return Ok(None);
        }
        let index = last.index;
        if v.check_memo(index, proposed).is_err() {
            return Ok(None);
        }
        let payout = (last.op == confidant_core::OpTag::Unshield).then(|| Payout {
            amount: last.value_balance as u64,
            destination: last.external.clone(),
        });
        Ok(Some(Accepted {
            bundle_index: index,
            first_leaf: last.first_leaf,
            new_root: v.anchor_history()[index + 1].clone(),
            bundle_hash: hex::encode(bundle_hash(last)?),
            nullifiers: last.actions.iter().map(|a| a.nullifier.clone()).collect(),
            commitments: last.actions.iter().map(|a| a.cmx.clone()).collect(),
            payout,
        }))
    }

    /// Marks `root_hex` anchored only after reading the anchor transaction from the ledger itself:
    /// validated, sent by the pool, carrying a `confidant/anchor` memo whose root is `root_hex`.
    /// A coordinator's bare claim is never enough.
    pub async fn anchored(
        &self,
        root_hex: &str,
        anchor_tx_hash: &str,
    ) -> Result<PoolState, NodeError> {
        let tx = self.ledger.tx(anchor_tx_hash).await?;
        let memo = anchor_fact(&tx, &self.pool)?;
        if hex::encode(memo.root) != root_hex.to_ascii_lowercase() {
            return Err(LedgerError::Anchor("anchor memo does not carry the claimed root").into());
        }
        let mut v = self.lock();
        v.set_anchored_root(root_hex)?;
        self.persist(&v)?;
        Ok(v.state())
    }

    /// Startup gate: the pool account's on-ledger custody must be exactly the configured
    /// `signers` with quorum `quorum` and the master key disabled; otherwise the node refuses to
    /// serve (it would be trusting a custody it does not believe in).
    pub async fn check_custody(
        ledger: &L,
        pool: &str,
        signers: &[String],
        quorum: u64,
    ) -> Result<(), NodeError> {
        let info = ledger.account_info(pool).await?;
        Ok(check_custody_info(&info, signers, quorum)?)
    }

    pub fn snapshot(&self) -> VerifierSnapshot {
        self.lock().snapshot()
    }

    // ----- joining from the ledger ----------------------------------------------------------

    /// Builds a verifier from the pool account's anchor memos on the ledger, a mirror's log, and
    /// the deposit facts the node reads for every shield itself (`rebuild_checked`), which refuses
    /// any step whose root differs from its memo. The node then marks the last root anchored.
    pub async fn rebuild_from_ledger(
        ledger: &L,
        pool: &str,
        mirror: &[BundleRecord],
    ) -> Result<Verifier, NodeError> {
        let txs = ledger.account_tx(pool).await?;
        ensure_full_history(&txs, pool)?;
        let memos = anchor_memos(&txs, pool)?;
        let mut deposits = std::collections::BTreeMap::new();
        for memo in &memos {
            if memo.op == confidant_core::OpTag::Shield {
                let tx = ledger.tx(&memo.external).await?;
                deposits.insert(
                    memo.external.clone(),
                    deposit_facts(&tx, &memo.external, pool)?,
                );
            }
        }
        let mut verifier = Verifier::rebuild_checked(
            &memos,
            &mut |h| {
                mirror
                    .iter()
                    .find(|r| bundle_hash(r).ok() == Some(*h))
                    .cloned()
            },
            &mut |tx| deposits.get(tx).cloned(),
        )?;
        // `rebuild_checked` already refused any step whose root differs from its memo, so the last
        // memo's root is the verifier's root; record it as anchored.
        if let Some(last) = memos.last() {
            verifier.set_anchored_root(&hex::encode(last.root))?;
        }
        Ok(verifier)
    }
}
