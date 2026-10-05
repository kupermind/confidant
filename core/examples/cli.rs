//! A thin command-line front over `confidant-core` for end-to-end harnesses — v2 shape.
//!
//! TEST TOOL, not the product API. Two roles, two state files:
//!
//! ```text
//! cli wallet   --file data/alice.wallet.json         init | info | balance | sync <log.json>
//!                                                    build-shield <amount> <out.json>
//!                                                    build-transfer <recipientAddrHex> <amount> <out.json>
//!                                                    build-unshield <amount> <destination> <out.json>
//! cli verifier --state data/v1.verifier-state.json   submit-shield <bundle.json> <depositTxHash> <delivered> <memoHashHex>
//!                                                    submit-transfer <bundle.json>
//!                                                    submit-unshield <bundle.json> <destination>
//!                                                    state | export-log <log.json> | anchored <rootHex>
//!                                                    anchor-memo <index> | check-memo <index> <memoHex>
//!                                                    rebuild <memos.json> <log.json> | reveal <viewingKeyHex>
//! ```
//!
//! The wallet file holds the spending key **unencrypted**: it must be gitignored
//! (`*.wallet.json` or under `data/`), is written 0600, and every run says so. The verifier
//! state is public (no keys). The XRPL side (deposit validation, memo Payments, payouts, the
//! SignerList quorum) lives in `e2e/testnet/run.mjs`.

use std::path::{Path, PathBuf};
use std::process::exit;

use confidant_core::memo::AnchorMemo;
use confidant_core::{
    BundleRecord, DepositRef, TransferOutput, Verifier, VerifierSnapshot, Wallet, WalletSecret,
};

const WARNING: &str = "the wallet file holds the spending key UNENCRYPTED; test tool only, never \
production — real key custody (threshold / TEE) is the roadmap";

const USAGE: &str = "usage: cli wallet --file <*.wallet.json> <cmd> | cli verifier --state <*.verifier-state.json> <cmd>
wallet:   init | info | balance | sync <log.json>
          build-shield <amount> <out.json>
          build-transfer <recipientAddressHex> <amount> <out.json>
          build-unshield <amount> <destination> <out.json>
verifier: submit-shield <bundle.json> <depositTxHash> <deliveredAmount> <memoHashHex>
          submit-transfer <bundle.json> | submit-unshield <bundle.json> <destination>
          state | export-log <log.json> | anchored <rootHex> | reveal <viewingKeyHex>
          anchor-memo <index> | check-memo <index> <memoHex> | rebuild <memos.json> <log.json>
files must be gitignored: *.wallet.json / *.verifier-state.json or under data/";

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    exit(1)
}

fn arg<'a>(args: &'a [String], i: usize, name: &str) -> &'a str {
    args.get(i)
        .map(String::as_str)
        .unwrap_or_else(|| die(format!("missing argument <{name}>")))
}

fn num(s: &str) -> u64 {
    s.parse()
        .unwrap_or_else(|_| die(format!("`{s}` is not a u64")))
}

fn check_path(p: &Path, suffix: &str) {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let under_data = p.components().any(|c| c.as_os_str() == "data");
    if !(name.ends_with(suffix) || under_data) {
        die(format!(
            "path must end in {suffix} or live under data/ (gitignored)"
        ));
    }
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> T {
    let text = std::fs::read_to_string(p).unwrap_or_else(|e| die(format!("{}: {e}", p.display())));
    serde_json::from_str(&text).unwrap_or_else(|e| die(format!("{}: {e}", p.display())))
}

/// Writes a file owner-read/write only (0600), replacing any existing file.
fn write_private(p: &Path, text: &str) {
    if let Some(dir) = p.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).unwrap_or_else(|e| die(e));
        }
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(p).unwrap_or_else(|e| die(e));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))
            .unwrap_or_else(|e| die(e));
    }
    use std::io::Write;
    f.write_all(text.as_bytes()).unwrap_or_else(|e| die(e));
}

fn write_public(p: &Path, text: &str) {
    if let Some(dir) = p.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).unwrap_or_else(|e| die(e));
        }
    }
    std::fs::write(p, text).unwrap_or_else(|e| die(e));
}

fn out<T: serde::Serialize>(v: &T) {
    println!(
        "{}",
        serde_json::to_string_pretty(v).unwrap_or_else(|e| die(e))
    );
}

fn wallet_cmd(file: PathBuf, args: &[String]) {
    check_path(&file, ".wallet.json");
    eprintln!("WARNING: {WARNING} ({})", file.display());
    let cmd = arg(args, 0, "command");
    let a = &args[1..];
    let load = || -> Wallet {
        let secret: WalletSecret = read_json(&file);
        Wallet::from_secret(&secret).unwrap_or_else(|e| die(e))
    };
    // A wallet needs the public log to know its notes; every command but init/info takes one.
    let synced = |log: &str| -> Wallet {
        let mut w = load();
        let log: Vec<BundleRecord> = read_json(Path::new(log));
        w.sync(&log).unwrap_or_else(|e| die(e));
        w
    };
    match cmd {
        "init" => {
            if file.exists() {
                die("wallet file exists; refusing to overwrite a spending key");
            }
            let w = Wallet::new();
            write_private(
                &file,
                &serde_json::to_string_pretty(&w.secret()).unwrap_or_else(|e| die(e)),
            );
            out(&w.info());
        }
        "info" => out(&load().info()),
        "balance" => {
            let w = synced(arg(a, 0, "log.json"));
            out(&serde_json::json!({ "balance": w.balance(), "synced": w.synced() }));
        }
        "sync" => {
            let w = synced(arg(a, 0, "log.json"));
            out(
                &serde_json::json!({ "balance": w.balance(), "synced": w.synced(), "root": w.replica().state().root }),
            );
        }
        "build-shield" => {
            // `<log.json>` is optional for shields (no witnesses needed) — but syncing keeps the
            // anchor current; the harness always passes it.
            let w = match a.get(2) {
                Some(log) => synced(log),
                None => load(),
            };
            let p = w
                .build_shield(num(arg(a, 0, "amount")))
                .unwrap_or_else(|e| die(e));
            write_public(
                Path::new(arg(a, 1, "out.json")),
                &serde_json::to_string_pretty(&p.record).unwrap_or_else(|e| die(e)),
            );
            out(&serde_json::json!({ "bundleHash": p.hash_hex(), "file": arg(a, 1, "out.json") }));
        }
        "build-transfer" => {
            let w = synced(arg(a, 3, "log.json"));
            let outputs = [TransferOutput {
                recipient: arg(a, 0, "recipientAddressHex").to_owned(),
                amount: num(arg(a, 1, "amount")),
            }];
            let p = w.build_transfer(&outputs).unwrap_or_else(|e| die(e));
            write_public(
                Path::new(arg(a, 2, "out.json")),
                &serde_json::to_string_pretty(&p.record).unwrap_or_else(|e| die(e)),
            );
            out(&serde_json::json!({ "bundleHash": p.hash_hex(), "file": arg(a, 2, "out.json") }));
        }
        "build-unshield" => {
            let w = synced(arg(a, 3, "log.json"));
            let p = w
                .build_unshield(num(arg(a, 0, "amount")), arg(a, 1, "destination"))
                .unwrap_or_else(|e| die(e));
            write_public(
                Path::new(arg(a, 2, "out.json")),
                &serde_json::to_string_pretty(&p.record).unwrap_or_else(|e| die(e)),
            );
            out(&serde_json::json!({ "bundleHash": p.hash_hex(), "file": arg(a, 2, "out.json") }));
        }
        other => die(format!("unknown wallet command `{other}`")),
    }
}

fn verifier_cmd(state: PathBuf, args: &[String]) {
    check_path(&state, ".verifier-state.json");
    let cmd = arg(args, 0, "command");
    let a = &args[1..];
    let load = || -> Verifier {
        if !state.exists() {
            return Verifier::new();
        }
        let snap: VerifierSnapshot = read_json(&state);
        Verifier::restore(&snap).unwrap_or_else(|e| die(e))
    };
    let save = |v: &Verifier| {
        write_public(
            &state,
            &serde_json::to_string_pretty(&v.snapshot()).unwrap_or_else(|e| die(e)),
        );
    };
    let bundle = |p: &str| -> BundleRecord { read_json(Path::new(p)) };
    match cmd {
        "submit-shield" => {
            let mut v = load();
            let dep = DepositRef {
                tx_hash: arg(a, 1, "depositTxHash").to_owned(),
                delivered: num(arg(a, 2, "deliveredAmount")),
                memo_hash: arg(a, 3, "memoHashHex").to_owned(),
            };
            let acc = v
                .submit_shield(&bundle(arg(a, 0, "bundle.json")), &dep)
                .unwrap_or_else(|e| die(e));
            save(&v);
            out(&acc);
        }
        "submit-transfer" => {
            let mut v = load();
            let acc = v
                .submit_transfer(&bundle(arg(a, 0, "bundle.json")))
                .unwrap_or_else(|e| die(e));
            save(&v);
            out(&acc);
        }
        "submit-unshield" => {
            let mut v = load();
            let acc = v
                .submit_unshield(&bundle(arg(a, 0, "bundle.json")), arg(a, 1, "destination"))
                .unwrap_or_else(|e| die(e));
            save(&v);
            out(&acc);
        }
        "state" => out(&load().state()),
        "export-log" => {
            let v = load();
            write_public(
                Path::new(arg(a, 0, "log.json")),
                &serde_json::to_string_pretty(v.records()).unwrap_or_else(|e| die(e)),
            );
            out(&v.state());
        }
        "anchored" => {
            let mut v = load();
            v.set_anchored_root(arg(a, 0, "rootHex"))
                .unwrap_or_else(|e| die(e));
            save(&v);
            out(&v.state());
        }
        "anchor-memo" => {
            let v = load();
            let m = v
                .anchor_memo(num(arg(a, 0, "index")) as usize)
                .unwrap_or_else(|e| die(e));
            out(
                &serde_json::json!({ "memoHex": m.encode_hex().unwrap_or_else(|e| die(e)), "root": hex::encode(m.root), "bundleHash": hex::encode(m.bundle_hash), "external": m.external }),
            );
        }
        "check-memo" => {
            let v = load();
            let m = AnchorMemo::decode_hex(arg(a, 1, "memoHex")).unwrap_or_else(|e| die(e));
            v.check_memo(num(arg(a, 0, "index")) as usize, &m)
                .unwrap_or_else(|e| die(e));
            out(&serde_json::json!({ "ok": true }));
        }
        "rebuild" => {
            // A new node: memos read off the ledger (hex strings, in order) + a mirror log.
            let memo_hex: Vec<String> = read_json(Path::new(arg(a, 0, "memos.json")));
            let memos: Vec<AnchorMemo> = memo_hex
                .iter()
                .map(|h| AnchorMemo::decode_hex(h).unwrap_or_else(|e| die(e)))
                .collect();
            let log: Vec<BundleRecord> = read_json(Path::new(arg(a, 1, "log.json")));
            let v = Verifier::rebuild(&memos, |h| {
                log.iter()
                    .find(|r| confidant_core::record::bundle_hash(r).ok() == Some(*h))
                    .cloned()
            })
            .unwrap_or_else(|e| die(e));
            save(&v);
            out(&v.state());
        }
        "reveal" => out(&load()
            .reveal(arg(a, 0, "viewingKeyHex"))
            .unwrap_or_else(|e| die(e))),
        other => die(format!("unknown verifier command `{other}`")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("wallet") if args.get(1).map(String::as_str) == Some("--file") => {
            wallet_cmd(PathBuf::from(arg(&args, 2, "wallet file")), &args[3..])
        }
        Some("verifier") if args.get(1).map(String::as_str) == Some("--state") => {
            verifier_cmd(PathBuf::from(arg(&args, 2, "state file")), &args[3..])
        }
        _ => {
            eprintln!("{USAGE}\n\nWARNING: {WARNING}");
            exit(
                if args.is_empty() || matches!(args[0].as_str(), "help" | "--help" | "-h") {
                    0
                } else {
                    1
                },
            );
        }
    }
}
