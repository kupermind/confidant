//! `confidant-node --listen 127.0.0.1:7001 --state data/n1.verifier-state.json --xrpl-rpc URL --pool rPOOL --signers r1,r2,r3 --quorum 2 [--fee-shield-bps N --fee-unshield-bps N --fee-treasury rT] [--screening-list FILE] [--screening-standby-ledgers N] [--mirror http://host/log] [--rebuild] [--pof-revealing-enabled]`
//!
//! Holds no keys. Refuses to start unless the pool account's on-ledger SignerList is exactly
//! `--signers` with `SignerQuorum == --quorum` and the master key disabled. With `--rebuild` the node ignores its state file and rebuilds from the pool
//! account's anchor memos on the ledger plus the mirror's log, re-validating every shield's
//! deposit (`rebuild_checked`); otherwise it restores its state file (or starts empty).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use confidant_core::{BundleRecord, Verifier, VerifierSnapshot};
use confidant_node::{http, CredentialGate, FeeLedger, FeePolicy, JsonRpc, Node, Screening};

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
    // The nullifier-revealing proof-of-funds path is rejected at the design level (it regresses
    // graph-hiding; see docs/design/TIER4-PROOF-OF-FUNDS-REWRITE.md). OFF unless a demo is run with
    // this flag deliberately, until the Tier-4 circuit (no nullifier reveal) replaces the proof.
    let pof_revealing = args.iter().any(|a| a == "--pof-revealing-enabled");
    let standby: u64 = arg(&args, "--screening-standby-ledgers")
        .map(|n| {
            n.parse()
                .unwrap_or_else(|_| die("--screening-standby-ledgers must be a number"))
        })
        .unwrap_or(0);
    let bps = |name: &str| -> u64 {
        arg(&args, name)
            .map(|n| {
                n.parse()
                    .unwrap_or_else(|_| die(format!("{name} must be a number")))
            })
            .unwrap_or(0)
    };
    let fees = FeePolicy::new(
        bps("--fee-shield-bps"),
        bps("--fee-unshield-bps"),
        arg(&args, "--fee-treasury").as_deref(),
    )
    .unwrap_or_else(|e| die(e));
    let screening = match arg(&args, "--screening-list") {
        Some(p) => Screening::from_file(Path::new(&p), standby)
            .unwrap_or_else(|e| die(format!("--screening-list {p}: {e}"))),
        None => Screening::new(Vec::<String>::new(), standby),
    };
    let credential = match (
        arg(&args, "--credential-issuer"),
        arg(&args, "--credential-type"),
    ) {
        (Some(issuer), Some(t)) => {
            CredentialGate::new(&issuer, &t, arg(&args, "--credential-type-text").as_deref())
                .unwrap_or_else(|e| die(e))
        }
        (None, None) => CredentialGate::off(),
        _ => die("--credential-issuer and --credential-type go together"),
    };
    match credential.policy() {
        Some(p) => eprintln!(
            "credential gate: deposits only from accounts holding an accepted, unexpired credential of type {} ({}) from issuer {}",
            p.credential_type,
            p.type_text.as_deref().unwrap_or("non-text"),
            p.issuer
        ),
        None => eprintln!("credential gate: OFF (no --credential-issuer / --credential-type)"),
    }
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
        Node::<JsonRpc>::rebuild_from_ledger(&ledger, &pool, &records, &fees)
            .await
            .unwrap_or_else(|e| die(e))
    } else if let Some(p) = state.as_ref().filter(|p| p.exists()) {
        let text = std::fs::read_to_string(p).unwrap_or_else(|e| die(e));
        let snap: VerifierSnapshot = serde_json::from_str(&text).unwrap_or_else(|e| die(e));
        let fee_file = Node::<JsonRpc>::fee_ledger_path(p);
        let fee_ledger: FeeLedger = if fee_file.exists() {
            let t = std::fs::read_to_string(&fee_file).unwrap_or_else(|e| die(e));
            serde_json::from_str(&t).unwrap_or_else(|e| die(format!("{}: {e}", fee_file.display())))
        } else {
            FeeLedger::default()
        };
        (
            Verifier::restore(&snap).unwrap_or_else(|e| die(e)),
            fee_ledger,
        )
    } else {
        (Verifier::new(), FeeLedger::default())
    };
    let (verifier, fee_ledger) = verifier;
    // The verifying key is built lazily by the core; build it now so the first submission does
    // not pay for it.
    confidant_core::Params::get().vk();
    if fees.is_active() {
        eprintln!(
            "fees: shield {} bps, unshield {} bps, treasury {}; accrued {} swept {} available {} drops",
            fees.shield_bps,
            fees.unshield_bps,
            fees.treasury.as_deref().unwrap_or("-"),
            fee_ledger.accrued,
            fee_ledger.swept,
            fee_ledger.available()
        );
    } else {
        eprintln!("fees: OFF (no --fee-shield-bps / --fee-unshield-bps)");
    }
    eprintln!(
        "confidant-node: pool {pool}, {} bundles, root {}, listening on {listen} (holds no keys; reads the ledger at {rpc})",
        verifier.state().bundle_count,
        &verifier.state().root[..16]
    );

    let node = Arc::new(
        Node::new(verifier, ledger, pool, state)
            .with_screening(screening)
            .with_credential(credential)
            .with_pof_revealing(pof_revealing)
            .with_fees(fees, fee_ledger),
    );
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|e| die(e));
    axum::serve(listener, http::router(node))
        .await
        .unwrap_or_else(|e| die(e));
}
