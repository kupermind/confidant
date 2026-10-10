//! `confidant-node --listen 127.0.0.1:7001 --state data/n1.verifier-state.json --xrpl-rpc URL --pool rPOOL --signers r1,r2,r3 --quorum 2 [--screening-list FILE] [--screening-standby-ledgers N] [--mirror http://host/log] [--rebuild]`
//!
//! Holds no keys. Refuses to start unless the pool account's on-ledger SignerList is exactly
//! `--signers` with `SignerQuorum == --quorum` and the master key disabled. With `--rebuild` the node ignores its state file and rebuilds from the pool
//! account's anchor memos on the ledger plus the mirror's log, re-validating every shield's
//! deposit (`rebuild_checked`); otherwise it restores its state file (or starts empty).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use confidant_core::{BundleRecord, Verifier, VerifierSnapshot};
use confidant_node::{http, JsonRpc, Node, Screening};

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
    let signers: Vec<String> = arg(&args, "--signers")
        .unwrap_or_else(|| die("--signers r1,r2,... (the pool's SignerList) is required"))
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    let quorum: u64 = arg(&args, "--quorum")
        .and_then(|q| q.parse().ok())
        .unwrap_or_else(|| die("--quorum <k> is required"));
    let mirror = arg(&args, "--mirror");
    let rebuild = args.iter().any(|a| a == "--rebuild");
    let standby: u64 = arg(&args, "--screening-standby-ledgers")
        .map(|n| {
            n.parse()
                .unwrap_or_else(|_| die("--screening-standby-ledgers must be a number"))
        })
        .unwrap_or(0);
    let screening = match arg(&args, "--screening-list") {
        Some(p) => Screening::from_file(Path::new(&p), standby)
            .unwrap_or_else(|e| die(format!("--screening-list {p}: {e}"))),
        None => Screening::new(Vec::<String>::new(), standby),
    };
    if screening.is_active() {
        eprintln!(
            "screening: {} listed addresses ({} canonical, {} undecodable and inert), standby {} ledgers (shields from listed or too-fresh deposits and unshields to listed addresses are refused with 403)",
            screening.listed_count(),
            screening.listed_count() - screening.undecodable_count(),
            screening.undecodable_count(),
            screening.standby_ledgers()
        );
        if screening.undecodable_count() > 0 {
            eprintln!(
                "WARNING: screening list has {} entries that are not XRPL addresses — they block nobody; check the list file",
                screening.undecodable_count()
            );
        }
    } else {
        eprintln!("screening: OFF (no --screening-list, no standby)");
    }

    if let Some(p) = &state {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let under_data = p.components().any(|c| c.as_os_str() == "data");
        if !(name.ends_with(".verifier-state.json") || under_data) {
            die("--state must end in .verifier-state.json or live under data/");
        }
    }

    let ledger = JsonRpc::new(rpc.clone());
    // Custody gate: serve only against the quorum this node believes in.
    Node::<JsonRpc>::check_custody(&ledger, &pool, &signers, quorum)
        .await
        .unwrap_or_else(|e| die(e));
    eprintln!(
        "pool custody verified on the ledger: {quorum}-of-{} SignerList, master key disabled",
        signers.len()
    );
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

    let node = Arc::new(Node::new(verifier, ledger, pool, state).with_screening(screening));
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|e| die(e));
    axum::serve(listener, http::router(node))
        .await
        .unwrap_or_else(|e| die(e));
}
