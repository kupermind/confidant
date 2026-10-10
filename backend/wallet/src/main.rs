//! `confidant-wallet --listen 127.0.0.1:7950 --dir data/wallets --coordinator http://127.0.0.1:7900`
//!
//! Demo wallet service for the UI prototype. Holds the demo wallets' spending keys in `--dir`
//! (0600) — run it where the holder is; on the demo host it is a custodial convenience.

use std::sync::Arc;

use confidant_wallet::{http, HttpLog, Store};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let listen = arg(&args, "--listen").unwrap_or_else(|| "127.0.0.1:7950".into());
    let dir = arg(&args, "--dir").unwrap_or_else(|| "data/wallets".into());
    let coordinator = arg(&args, "--coordinator").unwrap_or_else(|| "http://127.0.0.1:7900".into());
    if !dir.split('/').any(|c| c == "data") {
        die("--dir must live under a data/ directory (wallet files hold spending keys)");
    }
    let store = Arc::new(Store::open(&dir, HttpLog::new(&coordinator)).unwrap_or_else(|e| die(e)));
    eprintln!(
        "confidant-wallet: {} wallets in {dir}, syncing from {coordinator}, listening on {listen} — WARNING: this host holds the demo wallets' spending keys (demo service; in production run it on the holder's machine)",
        store.len()
    );
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|e| die(e));
    axum::serve(listener, http::router(store))
        .await
        .unwrap_or_else(|e| die(e));
}
