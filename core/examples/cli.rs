//! A thin command-line front over `confidant-core` for end-to-end harnesses.
//!
//! This is a TEST TOOL, not the product API: it does what the backend will do (hold one `Pool`,
//! persist a snapshot, expose the five operations) so an XRPL-side script can drive the whole
//! demo on testnet before the HTTP layer exists. One command per invocation; JSON on stdout.
//!
//! ```text
//! cli --state data/e2e.pool-state.json register <owner>
//! cli --state ... shield <owner> <amount> <depositTxHash>
//! cli --state ... transfer <owner> <recipient> <amount>
//! cli --state ... unshield <owner> <amount> <destination>
//! cli --state ... reveal <viewingKeyHex>
//! cli --state ... anchored <rootHex>          # the FE confirms a root it wrote into a memo
//! cli --state ... state | owner <id> | export-log <file> | verify-log <file>
//! ```
//!
//! The state file holds spending keys **unencrypted**. The tool refuses any path that is not
//! covered by the repo's `.gitignore` patterns (`*.pool-state.json` or under `data/`), writes the
//! file with mode 0600, and says so loudly on every run. Real key custody (threshold / TEE) is
//! the backend's problem and a roadmap item — never pick this tool up as a backend.

use std::path::{Path, PathBuf};
use std::process::exit;

use confidant_core::record::bundle_from_record;
use confidant_core::{BundleRecord, Pool, Snapshot, TransferOutput};

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    exit(1)
}

fn arg<'a>(args: &'a [String], i: usize, name: &str) -> &'a str {
    args.get(i)
        .map(String::as_str)
        .unwrap_or_else(|| die(format!("missing argument <{name}>")))
}

fn amount(s: &str) -> u64 {
    s.parse()
        .unwrap_or_else(|_| die(format!("amount `{s}` is not a u64")))
}

fn check_state_path(p: &Path) {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let under_data = p.components().any(|c| c.as_os_str() == "data");
    if !(name.ends_with(".pool-state.json") || under_data) {
        die("state path must end in .pool-state.json or live under data/ (gitignored)");
    }
}

fn load(p: &Path) -> Pool {
    if !p.exists() {
        return Pool::new();
    }
    let text = std::fs::read_to_string(p).unwrap_or_else(|e| die(e));
    let snap: Snapshot = serde_json::from_str(&text).unwrap_or_else(|e| die(e));
    Pool::restore(&snap).unwrap_or_else(|e| die(e))
}

const WARNING: &str = "writes spending keys UNENCRYPTED to the state file; test tool only, never \
production — the backend owns real key custody (threshold / TEE, roadmap)";

const USAGE: &str = "usage: cli [--state <file>] <command> [args]
  register <owner>                         new owner (random spending key, held in the state file)
  owner <owner>                            address + viewing key of an owner
  shield <owner> <amount> <depositTxHash>  mint after the XRPL deposit was validated by the caller
  transfer <owner> <recipient> <amount>    one private hop
  unshield <owner> <amount> <destination>  authorize a payout to an XRPL address
  reveal <viewingKeyHex>                   selective disclosure for one holder
  anchored <rootHex>                       record the root the caller wrote into an XRPL memo
  state | export-log <file> | verify-log <file>
state file: must end in .pool-state.json or live under data/ (both gitignored); written 0600";

/// Writes the snapshot owner-read/write only (0600), replacing any existing file.
fn save(p: &Path, pool: &Pool) {
    if let Some(dir) = p.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).unwrap_or_else(|e| die(e));
        }
    }
    let text = serde_json::to_string_pretty(&pool.snapshot()).unwrap_or_else(|e| die(e));
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
        // An existing file keeps its old mode on open; force it.
        f.set_permissions(std::fs::Permissions::from_mode(0o600))
            .unwrap_or_else(|e| die(e));
    }
    use std::io::Write;
    f.write_all(text.as_bytes()).unwrap_or_else(|e| die(e));
}

fn out<T: serde::Serialize>(v: &T) {
    println!(
        "{}",
        serde_json::to_string_pretty(v).unwrap_or_else(|e| die(e))
    );
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut state = PathBuf::from("data/e2e.pool-state.json");
    if args.first().map(String::as_str) == Some("--state") {
        state = PathBuf::from(arg(&args, 1, "state file"));
        args.drain(..2);
    }
    check_state_path(&state);
    let cmd = args.first().cloned().unwrap_or_else(|| "help".into());
    if matches!(cmd.as_str(), "help" | "--help" | "-h") {
        eprintln!("{USAGE}\n\nWARNING: {WARNING}");
        return;
    }
    eprintln!("WARNING: cli {WARNING} ({})", state.display());
    let a = &args[1..];

    match cmd.as_str() {
        "register" => {
            let mut pool = load(&state);
            let info = pool
                .register_owner(arg(a, 0, "owner"))
                .unwrap_or_else(|e| die(e));
            save(&state, &pool);
            out(&info);
        }
        "owner" => out(&load(&state)
            .owner_info(arg(a, 0, "owner"))
            .unwrap_or_else(|e| die(e))),
        "shield" => {
            let mut pool = load(&state);
            let r = pool
                .shield(
                    arg(a, 0, "owner"),
                    amount(arg(a, 1, "amount")),
                    arg(a, 2, "depositTxHash"),
                )
                .unwrap_or_else(|e| die(e));
            save(&state, &pool);
            out(&r);
        }
        "transfer" => {
            let mut pool = load(&state);
            let outputs = [TransferOutput {
                recipient: arg(a, 1, "recipient").to_owned(),
                amount: amount(arg(a, 2, "amount")),
            }];
            let r = pool
                .transfer(arg(a, 0, "owner"), &outputs)
                .unwrap_or_else(|e| die(e));
            save(&state, &pool);
            out(&r);
        }
        "unshield" => {
            let mut pool = load(&state);
            let r = pool
                .unshield(
                    arg(a, 0, "owner"),
                    amount(arg(a, 1, "amount")),
                    arg(a, 2, "destination"),
                )
                .unwrap_or_else(|e| die(e));
            save(&state, &pool);
            out(&r);
        }
        "reveal" => out(&load(&state)
            .reveal(arg(a, 0, "viewingKeyHex"))
            .unwrap_or_else(|e| die(e))),
        "anchored" => {
            let mut pool = load(&state);
            pool.set_anchored_root(arg(a, 0, "rootHex"))
                .unwrap_or_else(|e| die(e));
            save(&state, &pool);
            out(&pool.state());
        }
        "state" => out(&load(&state).state()),
        "export-log" => {
            let pool = load(&state);
            let text = serde_json::to_string_pretty(pool.records()).unwrap_or_else(|e| die(e));
            std::fs::write(arg(a, 0, "file"), text).unwrap_or_else(|e| die(e));
            out(&pool.state());
        }
        "verify-log" => {
            // A third party: no keys, no snapshot — only the public log.
            let text = std::fs::read_to_string(arg(a, 0, "file")).unwrap_or_else(|e| die(e));
            let log: Vec<BundleRecord> = serde_json::from_str(&text).unwrap_or_else(|e| die(e));
            let mut verifier = Pool::new();
            for record in &log {
                let bundle = bundle_from_record(record, verifier.params().version)
                    .unwrap_or_else(|e| die(format!("record {}: {e}", record.index)));
                verifier
                    .submit(bundle, record.op, &record.external, None)
                    .unwrap_or_else(|e| die(format!("record {} refused: {e}", record.index)));
            }
            out(&verifier.state());
        }
        other => die(format!("unknown command `{other}`")),
    }
}
