// One-off: fund a pool account and an anchor sink on testnet, generate N signer keys, install the
// SignerList (quorum k) and disable the pool's master key. Writes data/quorum/config.json and
// data/quorum/signerN.key.json (0600, unencrypted seeds — test deployment only).
//
//   SIGNERS=3 QUORUM=2 DIR=../../data/quorum XRPL_WS=wss://s.altnet.rippletest.net:51233 node setup-pool.mjs

import { Client, setupPool } from "./lib.mjs";

const client = new Client(process.env.XRPL_WS ?? "wss://s.altnet.rippletest.net:51233");
await client.connect();
const config = await setupPool(client, {
  signers: Number(process.env.SIGNERS ?? 3), quorum: Number(process.env.QUORUM ?? 2), dir: process.env.DIR ?? "../../data/quorum",
});
console.log(JSON.stringify(config, null, 2));
await client.disconnect();
