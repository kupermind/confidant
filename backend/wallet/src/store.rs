//! Wallet files, syncing from a log source and building bundles.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use confidant_core::{BundleRecord, Error as CoreError, TransferOutput, Wallet, WalletSecret};
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Where the public log comes from: a coordinator's `GET /log`, or a test double.
pub trait LogSource: Send + Sync {
    fn log(&self) -> impl Future<Output = Result<Vec<BundleRecord>, String>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("no such wallet")]
    NotFound,
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("log source: {0}")]
    Log(String),
    #[error("wallet store: {0}")]
    Io(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WalletSummary {
    pub id: String,
    pub address: String,
    pub viewing_key: String,
    pub balance: u64,
    pub synced: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Built {
    pub record: BundleRecord,
    pub bundle_hash: String,
}

/// All wallets this service holds, keyed by their bearer id.
pub struct Store<L: LogSource> {
    dir: PathBuf,
    log: L,
    wallets: Mutex<HashMap<String, Wallet>>,
}

fn io(e: impl std::fmt::Display) -> StoreError {
    StoreError::Io(e.to_string())
}

impl<L: LogSource> Store<L> {
    /// Opens (creating) `dir` and loads every `*.wallet.json` in it.
    pub fn open(dir: impl Into<PathBuf>, log: L) -> Result<Self, StoreError> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).map_err(io)?;
        let mut wallets = HashMap::new();
        for entry in std::fs::read_dir(&dir).map_err(io)? {
            let path = entry.map_err(io)?.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(id) = name.strip_suffix(".wallet.json") else {
                continue;
            };
            let text = std::fs::read_to_string(&path).map_err(io)?;
            let secret: WalletSecret = serde_json::from_str(&text).map_err(io)?;
            wallets.insert(id.to_owned(), Wallet::from_secret(&secret)?);
        }
        Ok(Self {
            dir,
            log,
            wallets: Mutex::new(wallets),
        })
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Wallet>> {
        self.wallets.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.wallet.json"))
    }

    /// A new wallet with a fresh spending key, persisted 0600 under a random bearer id.
    pub fn create(&self) -> Result<WalletSummary, StoreError> {
        let mut raw = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut raw);
        let id = hex::encode(raw);
        let wallet = Wallet::new();
        write_private(&self.path(&id), &wallet.secret())?;
        let summary = summarize(&id, &wallet);
        self.lock().insert(id, wallet);
        Ok(summary)
    }

    /// Syncs `id` from the log source and returns its summary.
    pub async fn sync(&self, id: &str) -> Result<WalletSummary, StoreError> {
        if !self.lock().contains_key(id) {
            return Err(StoreError::NotFound);
        }
        let log = self.log.log().await.map_err(StoreError::Log)?;
        let mut wallets = self.lock();
        let wallet = wallets.get_mut(id).ok_or(StoreError::NotFound)?;
        wallet.sync(&log)?;
        Ok(summarize(id, wallet))
    }

    pub fn summary(&self, id: &str) -> Result<WalletSummary, StoreError> {
        let wallets = self.lock();
        let wallet = wallets.get(id).ok_or(StoreError::NotFound)?;
        Ok(summarize(id, wallet))
    }

    pub async fn build_shield(&self, id: &str, amount: u64) -> Result<Built, StoreError> {
        self.sync(id).await?;
        let wallets = self.lock();
        let wallet = wallets.get(id).ok_or(StoreError::NotFound)?;
        let p = wallet.build_shield(amount)?;
        Ok(built(p))
    }

    pub async fn build_transfer(
        &self,
        id: &str,
        recipient: &str,
        amount: u64,
    ) -> Result<Built, StoreError> {
        self.sync(id).await?;
        let wallets = self.lock();
        let wallet = wallets.get(id).ok_or(StoreError::NotFound)?;
        let p = wallet.build_transfer(&[TransferOutput {
            recipient: recipient.to_owned(),
            amount,
        }])?;
        Ok(built(p))
    }

    pub async fn build_unshield(
        &self,
        id: &str,
        amount: u64,
        destination: &str,
    ) -> Result<Built, StoreError> {
        self.sync(id).await?;
        let wallets = self.lock();
        let wallet = wallets.get(id).ok_or(StoreError::NotFound)?;
        let p = wallet.build_unshield(amount, destination)?;
        Ok(built(p))
    }

    /// A proof of funds for a lender's challenge (the canonical challenge string): never submitted,
    /// checked read-only by a node at `POST /pof/verify`.
    pub async fn build_proof_of_funds(
        &self,
        id: &str,
        amount: u64,
        challenge: &str,
    ) -> Result<Built, StoreError> {
        self.sync(id).await?;
        let wallets = self.lock();
        let wallet = wallets.get(id).ok_or(StoreError::NotFound)?;
        let p = wallet.build_proof_of_funds(amount, challenge.as_bytes())?;
        Ok(built(p))
    }
}

fn built(p: confidant_core::Prepared) -> Built {
    let bundle_hash = p.hash_hex();
    Built {
        record: p.record,
        bundle_hash,
    }
}

fn summarize(id: &str, wallet: &Wallet) -> WalletSummary {
    let info = wallet.info();
    WalletSummary {
        id: id.to_owned(),
        address: info.address,
        viewing_key: info.viewing_key,
        balance: wallet.balance(),
        synced: wallet.synced(),
    }
}

fn write_private(path: &Path, secret: &WalletSecret) -> Result<(), StoreError> {
    let text = serde_json::to_string_pretty(secret).map_err(io)?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(io)?;
        f.write_all(text.as_bytes()).map_err(io)?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    std::fs::write(path, text).map_err(io)
}
