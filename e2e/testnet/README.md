# Confidant e2e on XRPL testnet

The SPEC demo script with real testnet Payments, driving the Rust core through
`core/examples/cli.rs`. See [`docs/E2E.md`](../../docs/E2E.md) for what is asserted.

```bash
cargo build --release --example cli          # from the repo root
npm install                                   # xrpl.js 5.x + local Node 22
CLI=../../target/release/examples/cli npm run e2e
```

Testnet only. Writes `data/e2e-<timestamp>.*.json` at the repo root (gitignored — the state file
contains spending keys).
