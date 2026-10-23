# Demo script — Confidant on XRPL testnet

The 45-second story is SPEC §10. This is the operator's run-sheet for it: what to click, what the
audience sees, what to say (v2 wording: notes non-custodial, the quorum can drain the pool account but
cannot spend anyone's notes, the internal graph is hidden but not the endpoints, ZK first, TEE a
footnote, the protocol fee is an operating fee), and what to do when something stalls. Measured
timings are from a rehearsal on a laptop (see the table at the end); re-measure on the demo machine.

## Before the session (T − 60 min)

1. **Backend up.** `curl -s https://confidant.im/api/config` answers with the pool, `quorum`,
   `signers` and `fees`; `curl -s https://confidant.im/api/state` shows `anchoredRoot == root`. If not:
   `sudo systemctl restart confidant.target` on the host and watch
   `journalctl -u 'confidant-*' -f` for `pool custody verified` and `fees: shield 25 bps…`.
2. **Pool pre-populated** (`backend/quorum/prepopulate.mjs`, ≈ 25 s per holder): 8–12 holders with
   fixed denominations (5 / 10 / 25 XRP) and a few private transfers, run in the last hour so the
   explorer's recent history looks lived-in. Say on stage that it is a synthetic set funded from the
   faucet — never imply real users.
3. **Two browser tabs**, both on the UI (`https://confidant.im/`): tab A = the live wallet, tab B = the
   XRPL testnet explorer on the pool account (`cfg.explorer/accounts/<pool>`), zoomed so the memo
   column is readable. A third tab with `?demo=1` is the fallback (simulated data, every button works
   offline).
4. **Open the UI as a secure context** so the multithreaded wallet is used: `https://confidant.im/`
   (TLS setup: `backend/deploy/README.md`, "HTTPS on a domain"); if the domain or its certificate
   fails on the day, an SSH tunnel from the demo laptop
   (`ssh -N -L 8080:127.0.0.1:80 <host>`, then `http://localhost:8080/`). Over plain `http://<ip>/`
   browsers ignore the COOP/COEP headers and the wallet silently runs on one thread (≈ 16 s per
   proof instead of ≈ 3 s); the wallet card says which ("N threads").
5. **Warm the wallet** in tab A: open it once, create or restore the browser wallet, let the
   "proving keys ready (N threads, X s)" line appear under the balance, fund from the faucet. Keys
   live in this tab's worker; do not close the tab.
6. **A second wallet** (Bob) in another browser profile or on a second laptop, also warmed, its
   Orchard address copied to a sticky note.

## The 45 seconds

| # | Say | Click | Audience sees |
|---|---|---|---|
| 1 | "XLS-96 hides the amount. The chain still shows who paid whom. Watch us hide that too." | — | tab B: the pool account's history — Payments in, anchors out, nothing linking them |
| 2 | "Alice shields 25 XRP — an ordinary Payment with a memo; the proof she built in her own browser, keys never left it." | Shield → amount 25 → **Shield** | 3-step progress: proving (a few seconds on threads), deposit validated, quorum anchored; then "Shielded 25 XRP (note 24.9375 after the 0.25 % fee) · proved in X s on N threads" with explorer links |
| 3 | "Now she pays Bob inside the pool. Here's the ledger… only an anchor. No sender, no receiver, no amount." | Transfer → Bob's address → 24.9375 → **Send** | tab B: one new 1-drop Payment pool → sink with a `confidant/anchor` memo; nothing else |
| 4 | "Bob withdraws to a fresh address. Two of three independent verifiers signed the payout — none of them holds Bob's keys." | (Bob) Unshield → **fresh** → 24.9375 → **Withdraw** | "Withdrew 24.9375 XRP; r… receives 24.875 XRP after the 0.25 % fee" and the multisigned payout on the explorer (two Signers) |
| 5 | "Compliance: we hand a regulator one viewing key. It reveals exactly Bob's activity and balance — nothing else." | (Bob) Audit → **my key** → **Reveal** | the two notes (received, spent) and balance 0 |
| 6 | "Private on XRPL, auditable on demand, state on the ledger. Today it's a quorum; here's the XLS that makes verification native." | — | tab B again: the anchors; the footer's honest label |

The header link **How Confidant works →** opens `/how-it-works.html`, a one-page overview (problem, shield → transfer →
unshield, what makes it safe, this weekend's lending features, the enclave horizon, honest limits) — send it to
judges and lenders; it is phone-friendly.

The header shows **N XRP shielded in the pool · public** as a headline stat left of the status pill, with
"pool account B = N + E float & unswept fees · how it is counted →" under it; that link opens `/balances.html`, which
reconciles the two figures live from the ledger and answers the usual questions (memo, stray XRP, fees, float, anchors):
say "the pool's total is public — every shield and unshield is a Payment on the ledger; what's private
is who holds what and who paid whom inside."

Keep the fee line short: "the pool takes 0.25 % on the way in and out — public policy, every sweep is
on the ledger — transfers inside are free."

## The "what if your server dies" question

Answer with the snapshot: the ledger names every bundle by hash; the public log is pinned on IPFS under
CID `bafybeifgshmfpvfw75wbi5755urzjxejvjwv5lwds6lys6nelajd6etzda` (gateway: https://gateway.pinata.cloud/ipfs/bafybeifgshmfpvfw75wbi5755urzjxejvjwv5lwds6lys6nelajd6etzda; the CID is also on the
ledger: https://testnet.xrpl.org/transactions/8CBBA25CC347F510E626467A4987EF2BC77744944483165E60D960445625C03B). A stranger rebuilds the pool from the XRP Ledger plus that object:
`confidant-node --rebuild --mirror https://gateway.pinata.cloud/ipfs/bafybeifgshmfpvfw75wbi5755urzjxejvjwv5lwds6lys6nelajd6etzda --xrpl-rpc https://s.altnet.rippletest.net:51234 --pool … --signers … --quorum 2 --fee-shield-bps 25 --fee-unshield-bps 25 --fee-treasury …`
→ `/state` shows the live root (rehearsed 2026-10-23: 15 s, every proof re-verified). Honest line: pinned, not
permanent; the ledger carries the state, not the bytes.

## If something stalls

- **Shield lingers on step 3 with "Anchoring — the verifiers wait until the deposit is 2 ledgers old (≈ 8 s)"**:
  that is the screening standby, not an error; the page retries by itself (the node's refusal text is in the
  tooltip). Say: "the verifiers only accept a deposit once it is two ledgers deep."
- **Payout error after the anchor**: the note is spent and the user unpaid — say so, click **Withdraw**
  again (same amount, same address): `/endorse` is idempotent, the sidecars re-sign, the payout lands.
- **Backend offline** (status pill red): switch to the `?demo=1` tab and narrate the same six steps on
  simulated data; say it is simulated.
- **Browser without threads** (status line shows "1 thread"): proving takes ≈ 20 s per step; talk
  through the proof while it runs ("this is Halo 2, no trusted setup, on the Zcash Orchard circuit").
- **"Create wallet" looks stuck**: the spending key is instant, the testnet faucet takes 10–30 s. The
  card appears at once and says "funding… (10–30 s)" with the fund button disabled — nothing to press.
  If it ends in an error, press **fund (faucet)**: it funds the same account (generated and stored
  before the faucet was asked), never a second one.
- **Faucet down**: use the pre-funded Alice account from the rehearsal (secret in the demo laptop's
  password manager, never in the repo).

## Measured timings (fill in on the demo machine)

| Step | single-threaded | N threads |
|---|---|---|
| proving keys (once per tab, in the background) | 12.1 s | 6.0 s (16 threads, 20-core laptop) |
| shield proof | 16.2 s | 3.2 s |
| deposit validated + anchor + standby | ≈ 25 s wall | same |
| transfer proof + anchor | 23 s wall | 11 s wall |
| unshield proof + anchor + payout | 31 s wall | 20 s wall |
| whole cycle (shield, transfer, unshield, reveal), headless Chrome | 127 s | 75 s |

Measured 2026-10-13 against the deployed testnet quorum from a laptop (local page); the same flow
against the deployed UI itself: 131 s over plain `http://<ip>/` (1 thread, proof 16.3 s) and 77 s
through the localhost tunnel (16 threads, proof 3.1 s). **2026-10-14, over `https://confidant.im/`
(TLS live, PR #28):** cross-origin isolated, 16 threads, proving keys 6.0 s, shield proof 3.2 s,
whole cycle with fees 81 s — the production path, no tunnel needed. Re-measure on the demo machine
(a 4-core laptop will land between the columns).
