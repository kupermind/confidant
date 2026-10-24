//! The node's logic, independent of HTTP: a [`Verifier`] behind a mutex, the ledger it reads, and
//! the operations the endpoints expose.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use confidant_core::memo::AnchorMemo;
use confidant_core::record::bundle_hash;
use confidant_core::{
    Accepted, BundleRecord, DepositRef, Error as CoreError, Payout, PoolState, RevealReport,
    Submission, Verifier, VerifierSnapshot,
};
use serde::{Deserialize, Serialize};

use crate::credential::{ripple_now, CredentialGate, CredentialPolicy};
use crate::fees::{FeeLedger, FeePolicy, FeeReport};
use crate::pof::{PofChallenge, PofVerdict, MEANING};
use crate::screening::Screening;
use crate::xrpl::{
    account_balance, anchor_fact, anchor_memos, check_custody_info, deposit_facts, deposit_source,
    ensure_full_history, sweep_facts, sweeps_in, Ledger, LedgerError,
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
    /// Refused by this node's credential gate (XLS-70, feature A); HTTP 403.
    #[error("refused by credential policy: {0}")]
    Credential(String),
    /// A proof-of-funds challenge this node will not judge (fields, freshness); HTTP 400.
    #[error("proof of funds: {0}")]
    Pof(String),
    /// Too many proof-of-funds verifications in flight on this node; HTTP 429.
    #[error("proof of funds: this node is busy verifying, retry shortly")]
    Busy,
    /// A fee sweep that does not match this node's fee ledger; HTTP 409.
    #[error("fee sweep refused: {0}")]
    Fee(String),
}

/// What `submit_*` and `endorse` return: the core's `Accepted` plus the anchor memo the quorum
/// must now write (hex), so a coordinator needs no second call.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedWithMemo {
    #[serde(flatten)]
    pub accepted: Accepted,
    pub memo_hex: String,
    /// Protocol fee taken by this operation, in drops (0 for transfers and when fees are off).
    /// For a shield it is `deposit − note`; for an unshield `note − payout`.
    #[serde(default)]
    pub fee: u64,
}

pub struct Node<L: Ledger> {
    verifier: Mutex<Verifier>,
    ledger: L,
    pool: String,
    state_path: Option<PathBuf>,
    screening: Screening,
    credential: CredentialGate,
    fees: FeePolicy,
    fee_ledger: Mutex<FeeLedger>,
    /// Bounds concurrent proof-of-funds verifications: an unauthenticated ingress that costs a
    /// SNARK verification per request.
    pof_permits: tokio::sync::Semaphore,
}

/// How many proof-of-funds verifications a node runs at once; beyond that, 429.
pub const POF_PERMITS: usize = 4;

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
            credential: CredentialGate::off(),
            fees: FeePolicy::off(),
            fee_ledger: Mutex::new(FeeLedger::default()),
            pof_permits: tokio::sync::Semaphore::new(POF_PERMITS),
        }
    }

    /// Installs the protocol fee policy and the ledger of what already accrued/swept.
    pub fn with_fees(mut self, policy: FeePolicy, ledger: FeeLedger) -> Self {
        self.fees = policy;
        self.fee_ledger = Mutex::new(ledger);
        self
    }

    pub fn fee_policy(&self) -> &FeePolicy {
        &self.fees
    }

    /// Where the fee ledger lives: next to the state file.
    pub fn fee_ledger_path(state_path: &Path) -> PathBuf {
        state_path.with_extension("fees.json")
    }

    /// Lock-order invariant: a caller that needs both mutexes acquires the verifier lock
    /// (`lock()`) FIRST and the fee ledger (`lock_fees()`) second — `submit_*` / `endorse` hold `v`
    /// and then `accrue()`. Never call anything that takes `lock()` while holding this guard.
    fn lock_fees(&self) -> std::sync::MutexGuard<'_, FeeLedger> {
        self.fee_ledger.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist_fees(&self, l: &FeeLedger) -> Result<(), NodeError> {
        if let Some(p) = &self.state_path {
            let text = serde_json::to_string(l).map_err(|e| NodeError::Io(e.to_string()))?;
            std::fs::write(Self::fee_ledger_path(p), text)
                .map_err(|e| NodeError::Io(e.to_string()))?;
        }
        Ok(())
    }

    fn accrue(&self, fee: u64) -> Result<(), NodeError> {
        if fee == 0 {
            return Ok(());
        }
        let mut l = self.lock_fees();
        l.accrued = l.accrued.saturating_add(fee);
        self.persist_fees(&l)
    }

    /// The fee an unshield takes from `acc.payout`, applied in place: the payout becomes net.
    fn net_payout(&self, acc: &mut Accepted) -> u64 {
        match acc.payout.as_mut() {
            Some(p) => {
                let fee = self.fees.unshield_fee(p.amount);
                p.amount -= fee;
                fee
            }
            None => 0,
        }
    }

    /// The value of every note the pool still backs: shields in minus unshields out, from the
    /// public value balances of the accepted bundles (transfers are zero).
    pub fn outstanding_backing(&self) -> u64 {
        let sum: i128 = self
            .lock()
            .records()
            .iter()
            .map(|r| -(r.value_balance as i128))
            .sum();
        u64::try_from(sum.max(0)).unwrap_or(u64::MAX)
    }

    /// What may leave the pool account without touching user backing: its validated balance on
    /// the ledger minus `outstanding_backing`. Independent of the fee ledger by construction —
    /// the belt the auditor asked for (2026-10-13): a conservation check must not be computed in
    /// the accounting it guards.
    async fn pool_excess(&self) -> Result<u64, LedgerError> {
        let info = self.ledger.account_info(&self.pool).await?;
        Ok(account_balance(&info)?.saturating_sub(self.outstanding_backing()))
    }

    /// `GET /fees`: the policy, this node's ledger, the value the pool backs (the public pool
    /// total), and the sweepable amount — the fee ledger's `available` capped by the pool's excess
    /// over outstanding notes; nothing when the ledger cannot be read.
    pub async fn fees(&self) -> FeeReport {
        let excess = self.pool_excess().await.ok();
        // Read the backing (verifier lock, released inside the call) BEFORE taking the fee-ledger
        // lock: the apply paths hold the verifier lock and then take the fee ledger, so taking them
        // in the other order here could deadlock a node under a concurrent submit (lock-order
        // invariant on `lock` / `lock_fees`).
        let backing = self.outstanding_backing();
        let l = self.lock_fees();
        FeeReport {
            shield_bps: self.fees.shield_bps,
            unshield_bps: self.fees.unshield_bps,
            pool_excess: excess,
            outstanding_backing: backing,
            treasury: self.fees.treasury.clone(),
            accrued: l.accrued,
            swept: l.swept,
            available: match excess {
                Some(e) => l.available().min(e),
                None => 0,
            },
        }
    }

    /// Records a treasury sweep after reading the Payment from the ledger itself: validated,
    /// from the pool, to the treasury, not above what is available. Counted once per tx hash.
    pub async fn fee_swept(&self, tx_hash: &str) -> Result<FeeReport, NodeError> {
        let treasury = self
            .fees
            .treasury
            .clone()
            .ok_or_else(|| NodeError::Fee("no treasury configured".into()))?;
        let tx = self.ledger.tx(tx_hash).await?;
        let amount = sweep_facts(&tx, &self.pool, &treasury)?;
        // Independent of the fee ledger: after this sweep the pool must still hold every note's
        // value (read from the ledger; a sweep that broke that is never recorded, and is an alarm).
        let balance = account_balance(&self.ledger.account_info(&self.pool).await?)?;
        let outstanding = self.outstanding_backing();
        if balance < outstanding {
            return Err(NodeError::Fee(format!(
                "after sweep {tx_hash} the pool account holds {balance} drops, below the {outstanding} drops its notes are backed by"
            )));
        }
        {
            // Scoped: the std mutex guard must not live across the await below.
            let mut l = self.lock_fees();
            if !l.sweeps.contains(tx_hash) {
                if amount > l.available() {
                    return Err(NodeError::Fee(format!(
                        "{amount} drops exceed the {} available",
                        l.available()
                    )));
                }
                l.swept = l.swept.saturating_add(amount);
                l.sweeps.insert(tx_hash.to_owned());
                self.persist_fees(&l)?;
            }
        }
        Ok(self.fees().await)
    }

    /// Installs the boundary screening policy this node enforces (default: off).
    pub fn with_screening(mut self, screening: Screening) -> Self {
        self.screening = screening;
        self
    }

    pub fn screening(&self) -> &Screening {
        &self.screening
    }

    /// Installs the credential gate this node enforces on deposits (default: off).
    pub fn with_credential(mut self, gate: CredentialGate) -> Self {
        self.credential = gate;
        self
    }

    pub fn credential_policy(&self) -> Option<&CredentialPolicy> {
        self.credential.policy()
    }

    /// The concurrency bound on proof-of-funds verifications (tests hold its permits).
    pub fn pof_permits(&self) -> &tokio::sync::Semaphore {
        &self.pof_permits
    }

    /// `POST /pof/verify` (feature C, Tier 1, read side): verifies a holder's proof of funds
    /// against this node's own state and the lender's challenge. Read-only — never applied,
    /// persisted or logged; the challenge window is judged against the validated ledger this node
    /// reads itself; at most `POF_PERMITS` verifications run at a time.
    pub async fn verify_proof_of_funds(
        &self,
        record: &BundleRecord,
        amount: u64,
        challenge: &PofChallenge,
    ) -> Result<PofVerdict, NodeError> {
        let _permit = self
            .pof_permits
            .try_acquire()
            .map_err(|_| NodeError::Busy)?;
        challenge
            .validate()
            .map_err(|e| NodeError::Pof(e.to_string()))?;
        let validated = self.ledger.validated_ledger_index().await?;
        challenge
            .check_window(validated)
            .map_err(|e| NodeError::Pof(e.to_string()))?;
        let bytes = challenge.bytes(amount);
        let (root, hash) = {
            let v = self.lock();
            v.verify_proof_of_funds(record, amount, &bytes)?;
            (v.state().root, hex::encode(bundle_hash(record)?))
        };
        Ok(PofVerdict {
            ok: true,
            amount,
            challenge: challenge.clone(),
            validated_ledger: validated,
            root,
            bundle_hash: hash,
            meaning: MEANING.to_owned(),
        })
    }

    pub fn pool(&self) -> &str {
        &self.pool
    }

    /// Lock-order invariant: verifier lock before fee-ledger lock (see `lock_fees`). Holding this
    /// guard and then calling `lock_fees()` is fine; the reverse order can deadlock the node.
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

    fn with_memo(
        v: &Verifier,
        accepted: Accepted,
        fee: u64,
    ) -> Result<AcceptedWithMemo, NodeError> {
        let memo_hex = v.anchor_memo(accepted.bundle_index)?.encode_hex()?;
        Ok(AcceptedWithMemo {
            accepted,
            memo_hex,
            fee,
        })
    }

    /// The deposit the core may vouch for: the ledger's facts (screened) with the protocol fee
    /// taken off `delivered`, plus that fee. The wallet must have built the note for the net.
    async fn vouched_deposit(&self, tx_hash: &str) -> Result<(DepositRef, u64), NodeError> {
        let gross = self.screened_deposit(tx_hash).await?;
        let fee = self.fees.shield_fee(gross.delivered);
        Ok((
            DepositRef {
                delivered: gross.delivered - fee,
                ..gross
            },
            fee,
        ))
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
        // Credential gate: the depositor's XLS-70 credential, read from the ledger by this node
        // (never from the client), judged by issuer, type, acceptance and expiry. Runs on every
        // path that vouches for a deposit — submit and endorse alike.
        if let Some(p) = self.credential.policy() {
            let (account, _) = deposit_source(&tx)?;
            let entry = self
                .ledger
                .credential(&account, &p.issuer, &p.credential_type)
                .await?;
            self.credential
                .check(&account, entry.as_ref(), ripple_now())
                .map_err(|e| NodeError::Credential(e.to_string()))?;
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
        let (deposit, fee) = self.vouched_deposit(deposit_tx_hash).await?;
        let mut v = self.lock();
        let acc = v.submit_shield(record, &deposit)?;
        self.persist(&v)?;
        self.accrue(fee)?;
        Self::with_memo(&v, acc, fee)
    }

    pub fn submit_transfer(&self, record: &BundleRecord) -> Result<AcceptedWithMemo, NodeError> {
        let mut v = self.lock();
        let acc = v.submit_transfer(record)?;
        self.persist(&v)?;
        Self::with_memo(&v, acc, 0)
    }

    pub fn submit_unshield(
        &self,
        record: &BundleRecord,
        destination: &str,
    ) -> Result<AcceptedWithMemo, NodeError> {
        self.screened_destination(destination)?;
        let mut v = self.lock();
        let mut acc = v.submit_unshield(record, destination)?;
        self.persist(&v)?;
        let fee = self.net_payout(&mut acc);
        self.accrue(fee)?;
        Self::with_memo(&v, acc, fee)
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
        let mut shield_fee = 0;
        let submission = match submission {
            Submission::Shield { record, deposit } => {
                let (deposit, fee) = self.vouched_deposit(&deposit.tx_hash).await?;
                shield_fee = fee;
                Submission::Shield {
                    record: record.clone(),
                    deposit,
                }
            }
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
        if let Some(mut already) = Self::already_applied(&v, record, op, external, &proposed)? {
            // Already accrued when it was first applied: report the fee, do not count it twice.
            let fee = if already.payout.is_some() {
                self.net_payout(&mut already)
            } else {
                shield_fee
            };
            return Self::with_memo(&v, already, fee);
        }
        let mut acc = v.endorse(&submission, &proposed)?;
        self.persist(&v)?;
        let fee = if acc.payout.is_some() {
            self.net_payout(&mut acc)
        } else {
            shield_fee
        };
        self.accrue(fee)?;
        Self::with_memo(&v, acc, fee)
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
        // Gross here; the caller nets the fee off exactly as it did on first acceptance.
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
        fees: &FeePolicy,
    ) -> Result<(Verifier, FeeLedger), NodeError> {
        let txs = ledger.account_tx(pool).await?;
        ensure_full_history(&txs, pool)?;
        let memos = anchor_memos(&txs, pool)?;
        let mut deposits = std::collections::BTreeMap::new();
        let mut fee_ledger = FeeLedger::default();
        for memo in &memos {
            if memo.op == confidant_core::OpTag::Shield {
                let tx = ledger.tx(&memo.external).await?;
                let gross = deposit_facts(&tx, &memo.external, pool)?;
                // The same fee the accepting nodes took: the note was credited net of it.
                let fee = fees.shield_fee(gross.delivered);
                fee_ledger.accrued = fee_ledger.accrued.saturating_add(fee);
                deposits.insert(
                    memo.external.clone(),
                    DepositRef {
                        delivered: gross.delivered - fee,
                        ..gross
                    },
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
        // Unshield fees from the replayed records; sweeps from the pool's own Payments to the treasury.
        for r in verifier.records() {
            if r.op == confidant_core::OpTag::Unshield {
                fee_ledger.accrued = fee_ledger
                    .accrued
                    .saturating_add(fees.unshield_fee(r.value_balance as u64));
            }
        }
        if let Some(treasury) = &fees.treasury {
            for (hash, drops) in sweeps_in(&txs, pool, treasury) {
                if fee_ledger.sweeps.insert(hash) {
                    fee_ledger.swept = fee_ledger.swept.saturating_add(drops);
                }
            }
        }
        Ok((verifier, fee_ledger))
    }
}
