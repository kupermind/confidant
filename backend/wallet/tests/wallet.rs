//! The demo wallet service against an in-process verifier as the log source: create, sync, build
//! every op, persistence across restarts, error mapping on the HTTP surface.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use confidant_core::{BundleRecord, DepositRef, Verifier};
use confidant_wallet::{http, LogSource, Store, StoreError};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

/// The coordinator's log, served from a verifier the test drives directly.
#[derive(Clone, Default)]
struct FakeLog {
    verifier: Arc<Mutex<Verifier>>,
    fail: Arc<Mutex<bool>>,
}

impl LogSource for FakeLog {
    async fn log(&self) -> Result<Vec<BundleRecord>, String> {
        if *self.fail.lock().unwrap() {
            return Err("coordinator unreachable".into());
        }
        Ok(self.verifier.lock().unwrap().records().to_vec())
    }
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("confidant-wallet-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&d).ok();
    d
}

#[tokio::test]
async fn wallets_sync_build_and_persist() {
    let log = FakeLog::default();
    let dir = tmp("store");
    let store = Store::open(dir.join("data").join("wallets"), log.clone()).unwrap();
    assert!(store.is_empty());
    let alice = store.create().unwrap();
    let bob = store.create().unwrap();
    assert_ne!(alice.id, bob.id);
    assert_eq!(alice.id.len(), 32, "128-bit bearer id");
    assert_eq!(alice.address.len(), 86, "43-byte raw address, hex");
    assert_eq!(alice.viewing_key.len(), 192);
    assert_eq!(store.len(), 2);
    // Files are the holder's secrets: present, 0600.
    let file = dir
        .join("data")
        .join("wallets")
        .join(format!("{}.wallet.json", alice.id));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // Shield: the service builds, the test plays the deposit + the coordinator.
    let built = store.build_shield(&alice.id, 100).await.unwrap();
    assert_eq!(built.bundle_hash.len(), 64);
    log.verifier
        .lock()
        .unwrap()
        .submit_shield(
            &built.record,
            &DepositRef {
                tx_hash: "D1".into(),
                delivered: 100,
                memo_hash: built.bundle_hash.clone(),
            },
        )
        .unwrap();
    let s = store.sync(&alice.id).await.unwrap();
    assert_eq!((s.balance, s.synced), (100, 1));
    // Transfer to bob; both wallets see it after syncing.
    let t = store
        .build_transfer(&alice.id, &bob.address, 40)
        .await
        .unwrap();
    log.verifier
        .lock()
        .unwrap()
        .submit_transfer(&t.record)
        .unwrap();
    assert_eq!(store.sync(&alice.id).await.unwrap().balance, 60);
    assert_eq!(store.sync(&bob.id).await.unwrap().balance, 40);
    // Unshield from bob: the record binds the destination.
    let u = store.build_unshield(&bob.id, 25, "rDest").await.unwrap();
    assert_eq!(u.record.op, confidant_core::OpTag::Unshield);
    let acc = log
        .verifier
        .lock()
        .unwrap()
        .submit_unshield(&u.record, "rDest")
        .unwrap();
    assert_eq!(acc.payout.unwrap().amount, 25);
    assert_eq!(store.sync(&bob.id).await.unwrap().balance, 15);
    assert_eq!(store.summary(&bob.id).unwrap().balance, 15);

    // Errors: unknown wallet, insufficient funds, zero amount, unreachable log.
    assert!(matches!(
        store.sync("nope").await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(store.summary("nope"), Err(StoreError::NotFound)));
    assert!(matches!(
        store.build_shield("nope", 1).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.build_transfer(&bob.id, &alice.address, 1_000).await,
        Err(StoreError::Core(
            confidant_core::Error::InsufficientFunds { .. }
        ))
    ));
    assert!(matches!(
        store.build_unshield(&bob.id, 0, "rDest").await,
        Err(StoreError::Core(confidant_core::Error::ZeroAmount))
    ));
    *log.fail.lock().unwrap() = true;
    assert!(matches!(
        store.sync(&alice.id).await,
        Err(StoreError::Log(_))
    ));
    *log.fail.lock().unwrap() = false;

    // Restart: the same directory loads both wallets and resyncs to the same balances.
    drop(store);
    let again = Store::open(dir.join("data").join("wallets"), log.clone()).unwrap();
    assert_eq!(again.len(), 2);
    assert_eq!(again.sync(&alice.id).await.unwrap().balance, 60);
    assert_eq!(again.sync(&bob.id).await.unwrap().balance, 15);
    // A stray file that is not a wallet is ignored; a corrupt wallet file refuses to open.
    std::fs::write(dir.join("data").join("wallets").join("notes.txt"), "x").unwrap();
    assert_eq!(
        Store::open(dir.join("data").join("wallets"), log.clone())
            .unwrap()
            .len(),
        2
    );
    std::fs::write(
        dir.join("data").join("wallets").join("bad.wallet.json"),
        "{",
    )
    .unwrap();
    assert!(matches!(
        Store::open(dir.join("data").join("wallets"), log.clone()),
        Err(StoreError::Io(_))
    ));
    std::fs::remove_dir_all(&dir).ok();
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(
            body.map(|b| Body::from(b.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(&bytes) }));
    (status, v)
}

#[tokio::test]
async fn http_surface_round_trips_and_maps_errors() {
    use axum::response::IntoResponse;
    let log = FakeLog::default();
    let dir = tmp("http");
    let app = http::router(Arc::new(
        Store::open(dir.join("data").join("wallets"), log.clone()).unwrap(),
    ));
    let (s, v) = call(&app, "GET", "/health", None).await;
    assert_eq!((s, v["wallets"].as_u64()), (StatusCode::OK, Some(0)));
    let (s, w) = call(&app, "POST", "/wallets", None).await;
    assert_eq!(s, StatusCode::OK);
    let id = w["id"].as_str().unwrap().to_owned();
    assert_eq!(w["balance"], json!(0));
    let (s, v) = call(&app, "GET", &format!("/wallets/{id}"), None).await;
    assert_eq!((s, v["synced"].as_u64()), (StatusCode::OK, Some(0)));
    let (s, v) = call(
        &app,
        "POST",
        &format!("/wallets/{id}/shield"),
        Some(json!({ "amount": 50 })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["bundleHash"].as_str().unwrap().len(), 64);
    assert_eq!(v["record"]["op"], json!("shield"));
    let record: BundleRecord = serde_json::from_value(v["record"].clone()).unwrap();
    log.verifier
        .lock()
        .unwrap()
        .submit_shield(
            &record,
            &DepositRef {
                tx_hash: "D".into(),
                delivered: 50,
                memo_hash: v["bundleHash"].as_str().unwrap().into(),
            },
        )
        .unwrap();
    let (s, v) = call(&app, "POST", &format!("/wallets/{id}/sync"), None).await;
    assert_eq!((s, v["balance"].as_u64()), (StatusCode::OK, Some(50)));
    let (s, v) = call(
        &app,
        "POST",
        &format!("/wallets/{id}/unshield"),
        Some(json!({ "amount": 20, "destination": "rOut" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["record"]["external"], json!("rOut"));
    let (s, _) = call(
        &app,
        "POST",
        &format!("/wallets/{id}/transfer"),
        Some(json!({ "recipient": w["address"], "amount": 10 })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "a transfer to oneself is a valid bundle");
    // 404 unknown wallet, 400 bad amounts, 502 unreachable coordinator, 500 io.
    let (s, _) = call(&app, "GET", "/wallets/deadbeef", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = call(
        &app,
        "POST",
        &format!("/wallets/{id}/shield"),
        Some(json!({ "amount": 0 })),
    )
    .await;
    assert_eq!((s, v["error"].is_string()), (StatusCode::BAD_REQUEST, true));
    let (s, _) = call(
        &app,
        "POST",
        &format!("/wallets/{id}/transfer"),
        Some(json!({ "recipient": w["address"], "amount": 1_000_000 })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    *log.fail.lock().unwrap() = true;
    let (s, _) = call(&app, "GET", &format!("/wallets/{id}"), None).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    *log.fail.lock().unwrap() = false;
    assert_eq!(
        http::ApiError(StoreError::Io("disk".into()))
            .into_response()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        http::ApiError(StoreError::Core(confidant_core::Error::Internal("x")))
            .into_response()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    std::fs::remove_dir_all(&dir).ok();
}
