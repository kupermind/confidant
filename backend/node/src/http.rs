//! The HTTP surface: thin handlers over [`Node`]. Errors map to statuses the way
//! `docs/CORE-API.md` describes.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use confidant_core::{BundleRecord, Error as CoreError, Submission};
use serde::{Deserialize, Serialize};

use crate::node::{Node, NodeError};
use crate::xrpl::{Ledger, LedgerError};

pub struct ApiError(pub NodeError);

impl From<NodeError> for ApiError {
    fn from(e: NodeError) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            NodeError::Core(e) => match e {
                CoreError::ZeroAmount
                | CoreError::AmountOverflow
                | CoreError::InsufficientFunds { .. }
                | CoreError::TooManyOutputs(_)
                | CoreError::VouchedAmountNotApplicable { .. }
                | CoreError::InvalidEncoding
                | CoreError::DepositRequired
                | CoreError::MemoMalformed(_)
                | CoreError::MemoTooLarge(_) => StatusCode::BAD_REQUEST,
                CoreError::Build(_)
                | CoreError::Tree(_)
                | CoreError::NoWitness(_)
                | CoreError::Snapshot(_)
                | CoreError::MirrorMissing(_)
                | CoreError::LogRewound
                | CoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::CONFLICT,
            },
            NodeError::Ledger(
                LedgerError::Deposit(_) | LedgerError::Anchor(_) | LedgerError::Sweep(_),
            ) => StatusCode::UNPROCESSABLE_ENTITY,
            NodeError::Fee(_) => StatusCode::CONFLICT,
            NodeError::Ledger(_) => StatusCode::BAD_GATEWAY,
            NodeError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            NodeError::Screened(_) | NodeError::Credential(_) => StatusCode::FORBIDDEN,
        };
        (
            status,
            Json(serde_json::json!({ "error": self.0.to_string() })),
        )
            .into_response()
    }
}

type Result<T> = std::result::Result<Json<T>, ApiError>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogQuery {
    #[serde(default)]
    pub from: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogResponse {
    pub records: Vec<BundleRecord>,
    #[serde(flatten)]
    pub state: confidant_core::PoolState,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitShield {
    pub record: BundleRecord,
    pub deposit_tx_hash: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitTransfer {
    pub record: BundleRecord,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitUnshield {
    pub record: BundleRecord,
    pub destination: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endorse {
    pub submission: Submission,
    pub proposed_memo_hex: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchored {
    pub root: String,
    /// Hash of the pool's anchor Payment; the node reads it from the ledger and checks the memo.
    pub anchor_tx_hash: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reveal {
    pub viewing_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Swept {
    /// Hash of the pool → treasury Payment; the node reads it from the ledger.
    pub tx_hash: String,
}

pub fn router<L: Ledger + 'static>(node: Arc<Node<L>>) -> Router {
    Router::new()
        .route("/state", get(state::<L>))
        .route("/log", get(log::<L>))
        .route("/submit/shield", post(submit_shield::<L>))
        .route("/submit/transfer", post(submit_transfer::<L>))
        .route("/submit/unshield", post(submit_unshield::<L>))
        .route("/endorse", post(endorse::<L>))
        .route("/anchored", post(anchored::<L>))
        .route("/audit/reveal", post(reveal::<L>))
        .route("/fees", get(fees::<L>))
        .route("/policy", get(policy::<L>))
        .route("/fees/swept", post(fee_swept::<L>))
        .with_state(node)
}

async fn state<L: Ledger>(State(n): State<Arc<Node<L>>>) -> Json<confidant_core::PoolState> {
    Json(n.state())
}

/// `GET /policy`: the deposit gates this node enforces, for the coordinator's `/config` and the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyReport {
    pub credential: Option<crate::credential::CredentialPolicy>,
    pub screening: ScreeningReport,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreeningReport {
    pub active: bool,
    pub standby_ledgers: u64,
}

async fn policy<L: Ledger>(State(n): State<Arc<Node<L>>>) -> Json<PolicyReport> {
    Json(PolicyReport {
        credential: n.credential_policy().cloned(),
        screening: ScreeningReport {
            active: n.screening().is_active(),
            standby_ledgers: n.screening().standby_ledgers(),
        },
    })
}

async fn log<L: Ledger>(
    State(n): State<Arc<Node<L>>>,
    Query(q): Query<LogQuery>,
) -> Json<LogResponse> {
    Json(LogResponse {
        records: n.log(q.from),
        state: n.state(),
    })
}

async fn submit_shield<L: Ledger>(
    State(n): State<Arc<Node<L>>>,
    Json(b): Json<SubmitShield>,
) -> Result<crate::AcceptedWithMemo> {
    Ok(Json(n.submit_shield(&b.record, &b.deposit_tx_hash).await?))
}

async fn submit_transfer<L: Ledger>(
    State(n): State<Arc<Node<L>>>,
    Json(b): Json<SubmitTransfer>,
) -> Result<crate::AcceptedWithMemo> {
    Ok(Json(n.submit_transfer(&b.record)?))
}

async fn submit_unshield<L: Ledger>(
    State(n): State<Arc<Node<L>>>,
    Json(b): Json<SubmitUnshield>,
) -> Result<crate::AcceptedWithMemo> {
    Ok(Json(n.submit_unshield(&b.record, &b.destination)?))
}

async fn endorse<L: Ledger>(
    State(n): State<Arc<Node<L>>>,
    Json(b): Json<Endorse>,
) -> Result<crate::AcceptedWithMemo> {
    Ok(Json(n.endorse(&b.submission, &b.proposed_memo_hex).await?))
}

async fn anchored<L: Ledger>(
    State(n): State<Arc<Node<L>>>,
    Json(b): Json<Anchored>,
) -> Result<confidant_core::PoolState> {
    Ok(Json(n.anchored(&b.root, &b.anchor_tx_hash).await?))
}

async fn fees<L: Ledger>(State(n): State<Arc<Node<L>>>) -> Json<crate::FeeReport> {
    Json(n.fees().await)
}

async fn fee_swept<L: Ledger>(
    State(n): State<Arc<Node<L>>>,
    Json(b): Json<Swept>,
) -> Result<crate::FeeReport> {
    Ok(Json(n.fee_swept(&b.tx_hash).await?))
}

async fn reveal<L: Ledger>(
    State(n): State<Arc<Node<L>>>,
    Json(b): Json<Reveal>,
) -> Result<confidant_core::RevealReport> {
    Ok(Json(n.reveal(&b.viewing_key)?))
}
