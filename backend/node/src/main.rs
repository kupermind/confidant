//! `confidant-node --listen 127.0.0.1:7001 --state data/n1.verifier-state.json --xrpl-rpc URL --pool rPOOL [--mirror http://host/log] [--rebuild]`
//!
//! Holds no keys. With `--rebuild` the node ignores its state file and rebuilds from the pool
//! account's anchor memos on the ledger plus the mirror's log, re-validating every shield's
//! deposit (`rebuild_checked`); otherwise it restores its state file (or starts empty).

use std::path::PathBuf;
use std::sync::Arc;

use confidant_core::{BundleRecord, Verifier, VerifierSnapshot};
use confidant_node::{http, JsonRpc, Node};

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
    let listen = arg(&args, "--listen").unwrap_or_else(|| "127.0.0.1:7001".into());
    let state = arg(&args, "--state").map(PathBuf::from);
    let rpc =
        arg(&args, "--xrpl-rpc").unwrap_or_else(|| "https://s.altnet.rippletest.net:51234".into());
    let pool = arg(&args, "--pool").unwrap_or_else(|| die("--pool <poolAccount> is required"));
    let mirror = arg(&args, "--mirror");
    let rebuild = args.iter().any(|a| a == "--rebuild");

    if let Some(p) = &state {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let under_data = p.components().any(|c| c.as_os_str() == "data");
        if !(name.ends_with(".verifier-state.json") || under_data) {
            die("--state must end in .verifier-state.json or live under data/");
        }
    }

    let ledger = JsonRpc::new(rpc.clone());
    let verifier = if rebuild {
        let mirror_url =
            mirror.unwrap_or_else(|| die("--rebuild needs --mirror <url of another node's /log>"));
        let log: serde_json::Value = reqwest::get(&mirror_url)
            .await
            .unwrap_or_else(|e| die(e))
            .json()
            .await
            .unwrap_or_else(|e| die(e));
        let records: Vec<BundleRecord> =
            serde_json::from_value(log.get("records").cloned().unwrap_or(log))
                .unwrap_or_else(|e| die(format!("mirror log: {e}")));
        eprintln!(
            "rebuilding from the ledger ({rpc}) and {} mirror records…",
            records.len()
        );
        Node::<JsonRpc>::rebuild_from_ledger(&ledger, &pool, &records)
            .await
            .unwrap_or_else(|e| die(e))
    } else if let Some(p) = state.as_ref().filter(|p| p.exists()) {
        let text = std::fs::read_to_string(p).unwrap_or_else(|e| die(e));
        let snap: VerifierSnapshot = serde_json::from_str(&text).unwrap_or_else(|e| die(e));
        Verifier::restore(&snap).unwrap_or_else(|e| die(e))
    } else {
        Verifier::new()
    };
    eprintln!(
        "confidant-node: pool {pool}, {} bundles, root {}, listening on {listen} (holds no keys; reads the ledger at {rpc})",
        verifier.state().bundle_count,
        &verifier.state().root[..16]
    );

    let node = Arc::new(Node::new(verifier, ledger, pool, state));
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|e| die(e));
    axum::serve(listener, http::router(node))
        .await
        .unwrap_or_else(|e| die(e));
}
