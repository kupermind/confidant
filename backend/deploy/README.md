# Deploying the quorum services (systemd + nginx)

Generic manifests for one host running the whole quorum: N verifier nodes (+1 for the coordinator),
N signer sidecars, one coordinator, nginx in front of the coordinator. No host names or addresses live
in this directory; the pool, signers and ports come from `config.json` and the env files.

```
/srv/confidant/              owner: system user `confidant` (nologin), mode 750
  bin/                       confidant-node, cli            (cargo build --release --bin confidant-node --example cli)
  repo/                      this repository (backend/quorum needs `npm ci --omit=dev`, system Node ≥ 22)
  data/quorum/               config.json, signer<N>.key.json + treasury.key.json (0600), node<N>.verifier-state.json (+ .fees.json)
  env/                       common.env, node-<i>.env, signer-<i>.env, coordinator.env   (render-env.mjs)
  logs/
```

```bash
# once per pool (testnet faucet): keys 0600 under data/quorum, config.json next to them; the protocol
# fee (Railgun's model: a share of every shield and unshield, transfers free) is set here, server-side —
# FEE_SHIELD_BPS / FEE_UNSHIELD_BPS (default 25 = 0.25 %, max 1000) — and a treasury account is funded
sudo -u confidant -H bash -c 'cd /srv/confidant/repo/backend/quorum && SIGNERS=3 QUORUM=2 FEE_SHIELD_BPS=25 FEE_UNSHIELD_BPS=25 DIR=/srv/confidant/data/quorum node setup-pool.mjs'
# env files for the units (ports: nodes 7700+i, sidecars 7800+i, coordinator 7900, all on 127.0.0.1);
# FEE_ARGS come from config.json (fees + treasury) — every node of the quorum gets the same policy;
# optional boundary screening for every node: SCREENING_LIST=/srv/confidant/data/screening/ofac-xrp.txt SCREENING_STANDBY_LEDGERS=2
sudo -u confidant -H node /srv/confidant/repo/backend/deploy/render-env.mjs /srv/confidant/data/quorum/config.json /srv/confidant/env /srv/confidant/data/quorum 7700
# units
sudo install -m 0644 /srv/confidant/repo/backend/deploy/systemd/* /etc/systemd/system/ && sudo systemctl daemon-reload
for i in 0 1 2 3; do sudo systemctl enable confidant-node@$i; done; for i in 1 2 3; do sudo systemctl enable confidant-signer@$i; done
sudo systemctl enable confidant-coordinator confidant.target && sudo systemctl start confidant.target
# nginx (CORS for a browser UI, long proxy timeouts because a submit waits for the anchor) + firewall
sudo install -m 0644 /srv/confidant/repo/backend/deploy/nginx/confidant.conf /etc/nginx/sites-available/confidant
sudo ln -sf /etc/nginx/sites-available/confidant /etc/nginx/sites-enabled/ && sudo nginx -t && sudo systemctl reload nginx
sudo ufw allow 8787/tcp
```

Every node logs `pool custody verified on the ledger: k-of-N SignerList, master key disabled` or exits
(the custody gate), then `fees: shield S bps, unshield U bps, treasury rT…; accrued/swept/available`
(or `fees: OFF`). **Sweep accrued fees to the treasury** (k-of-N, verified on the ledger by every node):
`curl -sX POST http://127.0.0.1:7900/fees/sweep -H 'content-type: application/json' -d '{}'`; check with
`curl -s http://127.0.0.1:7900/fees`. The rates are fixed at the pool's genesis: a pool whose shields were
bound with one policy cannot be replayed under another, so new rates mean a new pool (`setup-pool.mjs` again,
fresh state files). Logs: `journalctl -u 'confidant-*' -f`. Restart everything:
`systemctl restart confidant.target`. The UI's base URL is `http://<host>:8787` (`docs/API-BOUNDARY.md`).

**Browser wallet:** `cargo install wasm-pack` once, then `wasm-pack build frontend/wasm --target web --release --out-dir ../pkg`
produces `frontend/pkg/` (≈ 3 MB wasm; gitignored) — ship it next to `index.html` and `wallet-worker.js`
(nginx serves `application/wasm`). Keys, notes and proofs then stay in the visitor's browser.
**Multithreaded proving** (`frontend/pkg-threads/`, also gitignored): a nightly build with atomics —
`rustup toolchain install nightly && rustup target add wasm32-unknown-unknown --toolchain nightly && rustup component add rust-src --toolchain nightly`, then
`frontend/wasm/build-threads.sh` (the exact RUSTFLAGS live there: atomics + shared, imported memory + the TLS
exports; without the shared-memory flags the thread-pool workers cannot receive the module's memory).
The worker uses it only when the page is cross-origin isolated, which `nginx/confidant-ui.conf` provides
(`Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Embedder-Policy: require-corp`; every
cross-origin resource the page loads must carry CORP — the xrpl.js CDN does); otherwise it falls back
to `pkg/`. Ship both directories. **Browsers honour those headers only in a secure context**: over
plain `http://<ip>/` Chrome logs "COOP header has been ignored, because the URL's origin was
untrustworthy" and the wallet runs single-threaded. Threads therefore need HTTPS (a domain + TLS on
this site), or `http://localhost` — e.g. on the demo laptop `ssh -N -L 8080:127.0.0.1:80 <host>` and
open `http://localhost:8080/`, which is a secure context and gets the full thread pool.

**UI prototype** (`frontend/index.html`, static) + **demo wallet service** (fallback when WebAssembly is unavailable) (`confidant-wallet`, holds the
demo wallets' spending keys under `data/wallets/` — a custodial convenience on the demo host, see
`README.md`): `cargo build --release --bin confidant-wallet`, install the binary next to the node,
`systemctl enable --now confidant-wallet`, then the nginx site `nginx/confidant-ui.conf` on port 80
(static page, `/api/` → coordinator, `/wallet/` → wallet service) and `ufw allow 80/tcp`. The UI needs
no configuration: it reads `/api/config` for the pool account and the explorer.

Smoke test from any machine with the CLI built (`backend/quorum/smoke.mjs`):

```bash
POOL=<pool from config.json> COORDINATOR_URL=http://<host>:8787 CLI=../../target/release/examples/cli node smoke.mjs
```

It funds a faucet wallet, shields, transfers, unshields to a fresh address and reveals; prints explorer
links. Wallets it creates land under `data/` (gitignored).

Units run as `confidant` with `ProtectSystem=strict` (nodes may write only `data/`), `NoNewPrivileges`,
`PrivateTmp`. Sidecar key files are unencrypted seeds, 0600 — a testnet deployment; see `SPEC.md` §6
for what a sidecar refuses to sign.
