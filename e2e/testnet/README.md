# Confidant e2e on XRPL testnet (v2)

The SPEC demo script with real testnet Payments and the v2 trust split: wallets build and prove
locally, a coordinator and three signers each run their own verifier state, the pool account is a
2-of-3 SignerList with the master key disabled, every operation is anchored in its own memo, and a
new node rebuilds from the ledger with every deposit re-validated. See
[`docs/E2E.md`](../../docs/E2E.md) for what is asserted and
[`docs/NODE-OPERATOR.md`](../../docs/NODE-OPERATOR.md) for the signer's duties.

```bash
cargo build --release --example cli          # from the repo root
npm install                                   # xrpl.js 5.x + local Node 22
CLI=../../target/release/examples/cli npm run e2e
```

Testnet only. Writes `data/e2e-<timestamp>/` at the repo root (gitignored — the wallet files contain
spending keys, mode 0600).
