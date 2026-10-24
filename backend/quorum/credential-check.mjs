// Live check of the credential gate (XLS-70, feature A) through the UI's own path, with a verdict:
//   1. a fresh faucet account shields through the demo wallet service + coordinator → must be refused
//      with 403 naming "credential" (no credential on the ledger);
//   2. the configured issuer signs CredentialCreate for that account, the account signs CredentialAccept;
//   3. the same record + deposit is resubmitted → must be anchored (the node's submit is idempotent for
//      the deposit, so no second deposit is paid).
//
//   WALLET_URL=http://127.0.0.1:7950 COORDINATOR_URL=http://127.0.0.1:7900 ISSUER_KEY=/srv/confidant/data/quorum/issuer.key.json \
//     node credential-check.mjs
//
// Reads the issuer seed from ISSUER_KEY ({ address, seed }, 0600 on the host); never prints it. Exit 0 only
// when the refusal and the later acceptance both happened as expected.
import { readFileSync } from "node:fs";
import { Client, Wallet, xrpToDrops } from "xrpl";
import { http, memoOf, MEMO_SHIELD, resilientClient } from "./lib.mjs";

const C = (process.env.COORDINATOR_URL ?? "http://127.0.0.1:7200").replace(/\/$/, "");
const W = (process.env.WALLET_URL ?? "http://127.0.0.1:7250").replace(/\/$/, "");
const DROPS = xrpToDrops(Number(process.env.DEPOSIT_XRP ?? "2"));
const RIPPLE_EPOCH = 946684800;
const t0 = Date.now();
const log = (m) => console.log(`[${String(Date.now() - t0).padStart(6)} ms] ${m}`);
const must = (c, m) => { if (!c) throw new Error(`CREDENTIAL-CHECK FAILED: ${m}`); };

const cfg = await http("GET", `${C}/config`);
must(cfg.pool && cfg.xrplWs, "coordinator /config must expose pool and xrplWs");
must(cfg.credential?.issuer && cfg.credential?.type, "the gate is off: /config.credential is null — nothing to check");
const issuerKey = JSON.parse(readFileSync(process.env.ISSUER_KEY ?? "../../data/quorum/issuer.key.json", "utf8"));
must(issuerKey.address === cfg.credential.issuer, `ISSUER_KEY is ${issuerKey.address}, the nodes trust ${cfg.credential.issuer}`);
const issuer = Wallet.fromSeed(issuerKey.seed);
const feeOf = (drops, bps) => Math.floor((Number(drops) * (bps ?? 0)) / 10000);
const NET = Number(DROPS) - feeOf(DROPS, cfg.fees?.shieldBps);
const client = resilientClient(new Client(cfg.xrplWs)); await client.connect();
try {
  const { wallet: holder } = await client.fundWallet();
  const alice = await http("POST", `${W}/wallets`);
  log(`holder ${holder.address} (faucet), pool wallet ${alice.id.slice(0, 6)}…, gate: issuer ${cfg.credential.issuer} type ${cfg.credential.typeText ?? cfg.credential.type}`);

  const built = await http("POST", `${W}/wallets/${alice.id}/shield`, { amount: NET });
  const tx = await client.autofill({ TransactionType: "Payment", Account: holder.address, Destination: cfg.pool, Amount: DROPS, Memos: [memoOf(MEMO_SHIELD, built.bundleHash)] });
  const signed = holder.sign(tx); const dep = await client.submitAndWait(signed.tx_blob);
  must(dep.result.meta?.TransactionResult === "tesSUCCESS", "deposit");
  log(`deposit ${cfg.explorer}/transactions/${signed.hash}`);

  // 1. Without a credential: the standby may answer first (403 "standby"); the gate must answer 403 "credential".
  const submit = async () => { for (let t = 0; t < 45; t++) { try { return await http("POST", `${C}/submit/shield`, { record: built.record, depositTxHash: signed.hash }); } catch (e) { if (e.status === 403 && /standby/.test(e.message)) { await new Promise((r) => setTimeout(r, 4000)); continue; } throw e; } } throw new Error("standby never cleared"); };
  let refused = null;
  try { await submit(); } catch (e) { refused = e; }
  must(refused && refused.status === 403 && /credential/i.test(refused.message), `expected a 403 naming the credential, got ${refused ? `${refused.status} ${refused.message}` : "an acceptance"}`);
  log(`refused as expected: ${refused.message}`);

  // 2. Issue and accept.
  const exp = Math.floor(Date.now() / 1000) - RIPPLE_EPOCH + 7 * 86400;
  let r = await client.submitAndWait(issuer.sign(await client.autofill({ TransactionType: "CredentialCreate", Account: issuer.address, Subject: holder.address, CredentialType: cfg.credential.type, Expiration: exp })).tx_blob);
  must(r.result.meta?.TransactionResult === "tesSUCCESS", "CredentialCreate");
  r = await client.submitAndWait(holder.sign(await client.autofill({ TransactionType: "CredentialAccept", Account: holder.address, Issuer: issuer.address, CredentialType: cfg.credential.type })).tx_blob);
  must(r.result.meta?.TransactionResult === "tesSUCCESS", "CredentialAccept");
  log("credential issued and accepted on the ledger");

  // 3. The same shield now passes (same record, same deposit — nothing is paid twice).
  const acc = await submit();
  must(acc?.anchorTx, "shield anchored after the credential");
  log(`shield anchored ${cfg.explorer}/transactions/${acc.anchorTx}, signed by ${acc.signers?.length ?? "?"} of ${cfg.signers}`);
  console.log("\nOK — the credential gate refuses without, accepts with; every node read the credential from the ledger");
} finally { await client.disconnect(); }
