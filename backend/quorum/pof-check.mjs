// Live check of the proof-of-funds read side (feature C, Tier 1) against running nodes, with a verdict:
//   1. a fresh faucet account gets the gate's credential (issue + accept), shields through the demo wallet
//      service + coordinator (anchored);
//   2. the wallet service builds a proof of funds for a lender's challenge (borrower, lender, the validated
//      ledger index, an expiry) — the canonical string the UI builds too;
//   3. POST /pof/verify on EVERY node → ok, same root as the node's /state, nothing logged or applied
//      (bundle count unchanged);
//   4. negative cases on node 0: another amount → 409; a stale ledger index → 400; an expired challenge → 400;
//      a pool record (the shield) → 409; an oversized body → 413;
//   5. the signing path: POST /pof/credential on the coordinator → a multisigned CredentialCreate from the pool
//      to the borrower whose fields equal node 0's own derivation; the holder accepts it; the ledger shows it
//      accepted with that URI. Skipped (with a note) when the coordinator has no such route yet.
//
//   WALLET_URL=http://127.0.0.1:7950 COORDINATOR_URL=http://127.0.0.1:7900 NODE_URLS=http://127.0.0.1:7700,http://127.0.0.1:7701,… \
//     ISSUER_KEY=/srv/confidant/data/quorum/issuer.key.json node pof-check.mjs
//
// NODE_URLS defaults to /config.nodes resolved against UI_URL (the public origin). Reads the issuer seed from
// ISSUER_KEY ({ address, seed }, 0600 on the host) only to issue the credential the gate requires; never prints it.
// Exit 0 only when every node verified and every negative case was refused with the expected status.
import { readFileSync } from "node:fs";
import { Client, Wallet, xrpToDrops } from "xrpl";
import { http, memoOf, MEMO_SHIELD, resilientClient } from "./lib.mjs";

const C = (process.env.COORDINATOR_URL ?? "http://127.0.0.1:7200").replace(/\/$/, "");
const W = (process.env.WALLET_URL ?? "http://127.0.0.1:7250").replace(/\/$/, "");
const DROPS = xrpToDrops(Number(process.env.DEPOSIT_XRP ?? "2"));
const RIPPLE_EPOCH = 946684800;
const t0 = Date.now();
const log = (m) => console.log(`[${String(Date.now() - t0).padStart(6)} ms] ${m}`);
const must = (c, m) => { if (!c) throw new Error(`POF-CHECK FAILED: ${m}`); };
const challengeString = (ch, amount) => `confidant/pof/v1|${ch.borrower}|${amount}|${ch.ledgerIndex}|${ch.expiry}|${ch.lender}`;

const cfg = await http("GET", `${C}/config`);
must(cfg.pool && cfg.xrplWs, "coordinator /config must expose pool and xrplWs");
const nodes = process.env.NODE_URLS
  ? process.env.NODE_URLS.split(",")
  : (cfg.nodes ?? []).map((u) => new URL(u, process.env.UI_URL ?? C).toString().replace(/\/$/, ""));
must(nodes.length > 0, "no node URLs: set NODE_URLS or have /config.nodes");
const feeOf = (drops, bps) => Math.floor((Number(drops) * (bps ?? 0)) / 10000);
const NET = Number(DROPS) - feeOf(DROPS, cfg.fees?.shieldBps);
const client = resilientClient(new Client(cfg.xrplWs)); await client.connect();
try {
  const { wallet: holder } = await client.fundWallet();
  const alice = await http("POST", `${W}/wallets`);
  log(`holder ${holder.address} (faucet), pool wallet ${alice.id.slice(0, 6)}…, ${nodes.length} nodes`);

  // 1. Credential (if the gate is on), deposit, shield through the coordinator.
  if (cfg.credential?.issuer) {
    const issuerKey = JSON.parse(readFileSync(process.env.ISSUER_KEY ?? "../../data/quorum/issuer.key.json", "utf8"));
    must(issuerKey.address === cfg.credential.issuer, `ISSUER_KEY is ${issuerKey.address}, the nodes trust ${cfg.credential.issuer}`);
    const issuer = Wallet.fromSeed(issuerKey.seed);
    const exp = Math.floor(Date.now() / 1000) - RIPPLE_EPOCH + 86400;
    let r = await client.submitAndWait(issuer.sign(await client.autofill({ TransactionType: "CredentialCreate", Account: issuer.address, Subject: holder.address, CredentialType: cfg.credential.type, Expiration: exp })).tx_blob);
    must(r.result.meta?.TransactionResult === "tesSUCCESS", "CredentialCreate");
    r = await client.submitAndWait(holder.sign(await client.autofill({ TransactionType: "CredentialAccept", Account: holder.address, Issuer: issuer.address, CredentialType: cfg.credential.type })).tx_blob);
    must(r.result.meta?.TransactionResult === "tesSUCCESS", "CredentialAccept");
    log("credential issued and accepted (the gate is on)");
  }
  const built = await http("POST", `${W}/wallets/${alice.id}/shield`, { amount: NET });
  const tx = await client.autofill({ TransactionType: "Payment", Account: holder.address, Destination: cfg.pool, Amount: DROPS, Memos: [memoOf(MEMO_SHIELD, built.bundleHash)] });
  const signed = holder.sign(tx); const dep = await client.submitAndWait(signed.tx_blob);
  must(dep.result.meta?.TransactionResult === "tesSUCCESS", "deposit");
  const submit = async () => { for (let t = 0; t < 45; t++) { try { return await http("POST", `${C}/submit/shield`, { record: built.record, depositTxHash: signed.hash }); } catch (e) { if (e.status === 403 && /standby/.test(e.message)) { await new Promise((r) => setTimeout(r, 4000)); continue; } throw e; } } throw new Error("standby never cleared"); };
  const acc = await submit();
  must(acc?.anchorTx, "shield anchored");
  log(`shield of ${NET} drops anchored ${cfg.explorer}/transactions/${acc.anchorTx}`);

  // 2. The lender's challenge and the holder's proof (X = the whole note here; any X ≤ balance works).
  const L = await client.getLedgerIndex();
  const ch = { borrower: holder.address, lender: "pof-check", ledgerIndex: L, expiry: L + 600 };
  const X = NET;
  const pof = await http("POST", `${W}/wallets/${alice.id}/pof`, { amount: X, challenge: challengeString(ch, X) });
  must(pof.record?.op === "proofoffunds", `record op is ${pof.record?.op}, expected the proof-of-funds tag`);
  log(`proof of funds built for X = ${X} drops, challenge at ledger ${L}, expiry ${ch.expiry}`);

  // 3. Every node verifies on its own state; nothing changes.
  const before = await Promise.all(nodes.map((n) => http("GET", `${n}/state`)));
  for (const [i, n] of nodes.entries()) {
    const v = await http("POST", `${n}/pof/verify`, { record: pof.record, amount: X, challenge: ch });
    must(v.ok === true && v.amount === X, `node ${i}: verdict ${JSON.stringify(v).slice(0, 120)}`);
    must(v.root === before[i].root, `node ${i}: verdict root ${v.root} ≠ its state ${before[i].root}`);
    must(typeof v.meaning === "string" && /not a lock/.test(v.meaning), `node ${i}: meaning missing`);
    must(v.validatedLedger >= L, `node ${i}: validatedLedger ${v.validatedLedger} < ${L}`);
    log(`node ${i} ok — root ${v.root.slice(0, 12)}…, judged at ledger ${v.validatedLedger}`);
  }
  const after = await Promise.all(nodes.map((n) => http("GET", `${n}/state`)));
  for (const [i] of nodes.entries()) must(after[i].bundleCount === before[i].bundleCount && after[i].root === before[i].root, `node ${i}: state changed`);

  // 4. Negative cases on node 0.
  const expect = async (status, re, body, what) => {
    try { await http("POST", `${nodes[0]}/pof/verify`, body); } catch (e) {
      must(e.status === status && (!re || re.test(e.message)), `${what}: expected ${status}, got ${e.status} ${e.message}`);
      log(`refused as expected (${what}): ${e.status} ${String(e.message).slice(0, 90)}`); return;
    }
    must(false, `${what}: was accepted`);
  };
  await expect(409, null, { record: pof.record, amount: X - 1, challenge: ch }, "another amount");
  await expect(400, /not recent/, { record: pof.record, amount: X, challenge: { ...ch, ledgerIndex: L - 400, expiry: L + 600 } }, "stale ledger index");
  await expect(400, /expired/, { record: pof.record, amount: X, challenge: { ...ch, ledgerIndex: L - 100, expiry: L - 50 } }, "expired");
  await expect(409, /not a proof of funds/i, { record: built.record, amount: NET, challenge: ch }, "a pool record");
  await expect(413, null, { record: "x".repeat(300 * 1024), amount: X, challenge: ch }, "oversized body");
  // 5. The quorum records the verdict as a Credential; the borrower confirms it; the ledger shows it.
  const v0 = await http("POST", `${nodes[0]}/pof/verify`, { record: pof.record, amount: X, challenge: ch, credential: true });
  must(v0.credential?.uri, `node 0 withholds the credential: ${v0.credentialRefused ?? "?"}`);
  let recorded = false;
  try {
    const cr = await http("POST", `${C}/pof/credential`, { record: pof.record, amount: X, challenge: ch });
    must(cr.tx && cr.subject === holder.address, `credential: ${JSON.stringify(cr).slice(0, 160)}`);
    must(cr.credentialType === v0.credential.credentialType && cr.uri === v0.credential.uri && cr.expiration === v0.credential.expiration, "credential fields differ from node 0's derivation");
    log(`CredentialCreate ${cfg.explorer}/transactions/${cr.tx} signed by ${cr.signers.length} of ${cfg.signers}; URI ${Buffer.from(cr.uri, "hex").toString()}`);
    const acc2 = await client.submitAndWait(holder.sign(await client.autofill({ TransactionType: "CredentialAccept", Account: holder.address, Issuer: cfg.pool, CredentialType: cr.credentialType })).tx_blob);
    must(acc2.result.meta?.TransactionResult === "tesSUCCESS", "CredentialAccept");
    const objs = (await client.request({ command: "account_objects", account: holder.address, type: "credential", ledger_index: "validated" })).result.account_objects ?? [];
    const hit = objs.find((o) => o.Issuer === cfg.pool && String(o.CredentialType).toUpperCase() === cr.credentialType.toUpperCase());
    must(hit && (hit.Flags & 0x00010000) && String(hit.URI).toUpperCase() === cr.uri.toUpperCase() && hit.Expiration === cr.expiration, `ledger credential ${JSON.stringify(hit)}`);
    log(`credential accepted on the ledger: expires ${new Date((hit.Expiration + RIPPLE_EPOCH) * 1000).toISOString()}`);
    recorded = true;
  } catch (e) { if (e.status === 404) log("coordinator has no /pof/credential route (older build) — signing path skipped"); else throw e; }
  console.log(`\nOK — ${nodes.length}/${nodes.length} nodes verified the proof on their own state; wrong amount, stale, expired, pool record and oversized refused${recorded ? "; the verdict was recorded as an accepted Credential on the ledger" : ""}`);
} finally { await client.disconnect(); }
