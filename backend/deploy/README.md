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

**HTTPS on a domain** (`nginx/confidant-ui-tls.conf`, replaces `confidant-ui.conf`). Same page and
proxies, served over TLS, so every visitor's browser is a secure context and gets the thread pool.
`<domain>` below is the public demo domain.

1. **DNS** at the registrar: `A` records for `<domain>` and `www.<domain>` → the host's public IPv4
   (plus `AAAA` → its IPv6, if it has one). Low TTL (300) until the demo is over. Check:
   `dig +short <domain>` and `dig +short www.<domain>` both print the host's address.
2. **Firewall:** `sudo ufw allow 443/tcp`. Keep 80 open (ACME renewals and the redirect).
3. **Certificate** (Let's Encrypt, free, 90 days, auto-renewed). With the HTTP site still up on port 80:
   ```bash
   sudo apt install -y certbot python3-certbot-nginx
   sudo mkdir -p /var/www/letsencrypt
   sudo certbot certonly --nginx -d <domain> -d www.<domain> -m <ops e-mail> --agree-tos -n
   ```
   `certonly` leaves the nginx config untouched; the cert lands in `/etc/letsencrypt/live/<domain>/`.
4. **Site:** render the TLS config and swap it in for the HTTP one:
   ```bash
   sed 's/__DOMAIN__/<domain>/g' /srv/confidant/repo/backend/deploy/nginx/confidant-ui-tls.conf \
     | sudo tee /etc/nginx/sites-available/confidant-ui >/dev/null
   sudo ln -sf /etc/nginx/sites-available/confidant-ui /etc/nginx/sites-enabled/confidant-ui
   sudo nginx -t && sudo systemctl reload nginx
   ```
   (`/etc/nginx/sites-enabled/` must hold only one `default_server` per port — remove the old UI
   site's link if it has another name, and the distribution's `default` site.)
5. **Renewals:** switch them to the webroot the new port-80 block serves and reload nginx after each
   one:
   ```bash
   sudo certbot reconfigure --cert-name <domain> --webroot -w /var/www/letsencrypt \
     --deploy-hook 'systemctl reload nginx'
   sudo certbot renew --dry-run
   ```
6. **Verify:**
   - `curl -sI https://<domain>/ | grep -i cross-origin` shows both COOP and COEP;
   - `curl -sI http://<domain>/` is a 301 to `https://<domain>/`, and `https://www.<domain>/` is a 301 to the apex;
   - `curl -s https://<domain>/api/config` answers with the pool, `quorum`, `signers` and `fees`;
   - in a browser on another machine, the DevTools console prints `true` for `crossOriginIsolated`,
     and the wallet card shows "N threads" with N > 1.
   - after the swap the port-80 block is `default_server`, so `http://<ip>/` also redirects to
     `https://<domain>/` (intended); the coordinator's own site on 8787 stays plain HTTP.
7. **Clients:** the coordinator is reachable as `https://<domain>/api` (smoke, pre-population:
   `COORDINATOR_URL=https://<domain>/api`). The plain `http://<host>:8787` port can stay for the CLI
   harnesses, or be closed (`sudo ufw delete allow 8787/tcp`) once nothing uses it.

The UI needs no change: it calls `/api/` and `/wallet/` on its own origin, so HTTPS covers them too
(no mixed content) and the isolated page can use them without CORP. No HSTS on purpose: a testnet
demo domain should be able to fall back to HTTP without browsers refusing it for months.

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
