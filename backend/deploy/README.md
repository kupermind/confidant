# Deploying the quorum services (systemd + nginx)

Generic manifests for one host running the whole quorum: N verifier nodes (+1 for the coordinator),
N signer sidecars, one coordinator, nginx in front of the coordinator. No host names or addresses live
in this directory; the pool, signers and ports come from `config.json` and the env files.

```
/srv/confidant/              owner: system user `confidant` (nologin), mode 750
  bin/                       confidant-node, cli            (cargo build --release --bin confidant-node --example cli)
  repo/                      this repository (backend/quorum needs `npm ci --omit=dev`, system Node ≥ 22)
  data/quorum/               config.json, signer<N>.key.json (0600), node<N>.verifier-state.json
  env/                       common.env, node-<i>.env, signer-<i>.env, coordinator.env   (render-env.mjs)
  logs/
```

```bash
# once per pool (testnet faucet): keys 0600 under data/quorum, config.json next to them
sudo -u confidant -H bash -c 'cd /srv/confidant/repo/backend/quorum && SIGNERS=3 QUORUM=2 DIR=/srv/confidant/data/quorum node setup-pool.mjs'
# env files for the units (ports: nodes 7700+i, sidecars 7800+i, coordinator 7900, all on 127.0.0.1);
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
(the custody gate). Logs: `journalctl -u 'confidant-*' -f`. Restart everything:
`systemctl restart confidant.target`. The UI's base URL is `http://<host>:8787` (`docs/API-BOUNDARY.md`).

Smoke test from any machine with the CLI built (`backend/quorum/smoke.mjs`):

```bash
POOL=<pool from config.json> COORDINATOR_URL=http://<host>:8787 CLI=../../target/release/examples/cli node smoke.mjs
```

It funds a faucet wallet, shields, transfers, unshields to a fresh address and reveals; prints explorer
links. Wallets it creates land under `data/` (gitignored).

Units run as `confidant` with `ProtectSystem=strict` (nodes may write only `data/`), `NoNewPrivileges`,
`PrivateTmp`. Sidecar key files are unencrypted seeds, 0600 — a testnet deployment; see `SPEC.md` §6
for what a sidecar refuses to sign.
