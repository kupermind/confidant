# Proof of shielded funds, and collateral escrow as a separate construction — proposal

> **Project lead's ruling, 2026-10-25 (after using the live feature).** A proof of funds that reveals the nullifiers of
> the proven notes degrades the holder's privacy — the node operators and, in the package flow, the lender learn when
> those notes are next spent — and makes the holder the last link of the protection (a manual refresh, in the right
> order). That is not an acceptable product property. **The only accepted design is §5: a circuit that proves control of
> unspent notes ≥ X without revealing nullifiers.** Everything built on 2026-10-24 (core slice, read side, package flow,
> on-ledger credential) is a demo of the flow and is labelled as degrading the holder's privacy; it is not the feature.
> Two findings go with the ruling: (1) the package flow hands the lender the record, so the lender sees the nullifiers —
> contrary to requirement 6 ("nullifiers stay with the nodes"); (2) the holder tab said "without naming your notes" and
> the lender page "not told: which notes" — both over-claims, corrected the same day. Interim rule for the demo: the
> on-ledger credential flow (nodes only) over the package flow, and the wallet, not the user, refreshes the notes.

**Status: design of record for the event (proposed 2026-10-14, decided 2026-10-16 in ROADMAP §0). Core slice built on-site 2026-10-24:** a fourth op tag `ProofOfFunds` that every apply path (submit, endorse, replay, memo) refuses before any signature check; `Wallet::build_proof_of_funds` / read-only `Verifier::verify_proof_of_funds` under the separate domain `ConfidantPoF_v1_`, registered in the hash-domain allow-list on purpose; break tests both directions, 100 % coverage, mutation table (audit REQ). **Tier 1's read side built on-site the same day:** `POST /pof/verify` on every node (challenge `{ borrower, lender, ledgerIndex, expiry }` recomputed to canonical bytes, freshness judged against the node's own validated ledger, 256 KiB / 4-at-a-time bounds, never logged or persisted; node coverage gate, mutation table, audit REQ), wasm `buildProofOfFunds`, `/config.nodes`. The `credential-pof` signing path got its own IMPL-PASS in-window (`docs/audit/RESP-pof-credential-2026-10-24.md`) and is live, with node-side bounds (gate eligibility, one credential per borrower, outstanding cap 8, fail-closed reads). Input: the pre-event spike
(draft PR #29), the audit's conditional DESIGN-PASS (`docs/audit/RESP-pof-design-2026-10-14.md`)
and the apply-path guards it required first (PR #31, PASS). This document fills what the spike
leaves out — the part that lives **on the XRP Ledger** — and sets proof of funds apart from
collateral escrow, the construction that actually locks (§1a). It doubles as the source for the pitch's "what comes next" slides. The decisions
(ROADMAP §0 (a)–(g)): A + C; the node endpoint verifies; X in clear; the pool stays on testnet and the
loan side runs on devnet or on slides; track
Lending & Borrowing.

Honest line, everywhere this is shown: a proof of funds is an **underwriting signal** ("someone who
controls ≥ X answered a challenge bound to account B"), **not proof that B owns ≥ X**, and it is a
**snapshot, not a lock** — by construction, not by scope (§1a).

## 1a. Three constructions, not a ladder (2026-10-14, after the audit's answer to the lead's objection)

With the Orchard circuit unchanged, spending a note is a unilateral act of its key holder: the circuit
enforces validity (nullifier, value balance), not external spending conditions, and nothing outside
the circuit can stop a valid spend from confirming. So "a proof of funds that locks the funds" does
not exist in this model. There are exactly three distinct objects:

| | What it does | Funds | Holder gives up | Lender's recourse |
|---|---|---|---|---|
| **Proof of funds** (§2 core slice, Tier 1 credential, Tier 2 tripwire) | attests that the holder controlled ≥ X as of ledger index L; detects and reacts when the proven notes move | **never move** | nothing — can still spend | detection (revocation, expiry), not prevention |
| **Shielded collateral escrow** (§4, formerly "Tier 3") | the collateral is moved into a note under joint control {holder, lender, arbiter}; release and liquidation follow on-ledger loan state | **move** into joint custody | sole custody of the collateral, and permanent visibility of that note to the lender via the joint viewing key | prevention by key control — an escrow, with an arbiter |
| **Circuit-level encumbrance** (§5, "Tier 4") | spending conditions or a no-nullifier-reveal proof inside a new circuit | never move | nothing | prevention without moving — the only "lock without moving", and it needs a new circuit |

Escrow is a legitimate and novel product on XRPL (the native Escrow cannot hold shielded notes), but
it is **not** proof of funds and is not presented as its evolution: the two do different things and
cost the holder different things. The deck shows them as two constructions. The event builds only
the first.

---

## 1. What the spike proves, and what it leaves off the ledger

The spike is core-only: `Wallet::build_proof_of_funds` and a read-only
`Verifier::verify_proof_of_funds`. Read together, the audit's conditions push every remaining piece
onto the ledger:

| Audit condition | Consequence | What is missing on XRPL today |
|---|---|---|
| 6: the record never goes to a coordinator, never enters the public log, and as few parties as possible see the nullifiers — preferably the quorum verifies, not the lender | the lender never holds the proof, so the lender cannot check it | **a verdict on the ledger** that the lender can read: an XLS-70 Credential issued by the pool account |
| 5: the lender re-verifies at decision time | a lender without the proof cannot re-verify | **freshness on the ledger**: the Credential's `Expiration`, plus **revocation** when the proven notes move (§3) |
| 5: the challenge binds the borrower's XRPL account | the account has to be proven, not just named | **account binding on the ledger**: the borrower's `CredentialAccept` is a transaction signed by that account |
| quorum rule (SPEC §5): signers never blind-sign | each signer must verify the proof itself before the pool account issues anything | **a new sidecar transaction kind** (`credential-pof`), checked against the signer's own node — a security-critical (trust-path) slice |
| 5 / Q-POF-5: a snapshot, not a lock | with the circuit unchanged no proof of funds can lock (§1a) | nothing to add on the ledger: a lock is escrow (§4) or a new circuit (§5) |

## 2. Tier 1 — a quorum-issued, expiring Credential (event candidate)

```
holder (browser wallet)            each verifier node (k of N)          pool account (k-of-N)        borrower / lender
───────────────────────            ───────────────────────────          ─────────────────────        ─────────────────
1. challenge = borrower account ‖ X ‖ validated ledger index L ‖ expiry E ‖ lender id
2. build_proof_of_funds(X, challenge)
3. POST /pof/verify ──────────────► verify_proof_of_funds on its OWN
   (to the nodes, never a             state; checks L is recent and E
    coordinator; not logged)          is ≤ L + window; keeps the
                                      nullifiers in a private watch list
4.                                  ◄── coordinator asks for k sidecar signatures on
                                        CredentialCreate{Subject = borrower,
                                        CredentialType = "ConfidantPoF", Expiration = E,
                                        URI = H(X ‖ L ‖ E)}; each sidecar signs only
                                        if ITS node verified exactly this (borrower, X, L, E)
5.                                                                     CredentialCreate on ledger
6.                                                                                                  borrower: CredentialAccept
                                                                                                    (signed by the borrower's account)
7.                                                                                                  lender / loan broker reads the
                                                                                                    accepted Credential from the ledger
```

**Built 2026-10-24 — steps 1–3 (the read side).** The challenge is the struct `{ borrower, lender, ledgerIndex, expiry }`
with canonical bytes `confidant/pof/v1|<borrower>|<X>|<L>|<E>|<lender>` (UTF-8, `|`-separated; the borrower a classic
address, the lender id 1–64 printable characters without `|`). Each node checks the fields, then `L ≤ validated`,
`validated − L ≤ 200`, `E > validated`, `E − L ≤ 21 600` against the validated ledger index **it reads itself**, then
`verify_proof_of_funds` on its own state. Verdict `{ ok, amount, challenge, validatedLedger, root, bundleHash, meaning }`;
400 for a refused challenge, 409 when the proof does not verify, 413 over 256 KiB, 429 beyond four concurrent
verifications. The node keeps no copy and logs nothing (requirement 6); the browser reaches the nodes at `/config.nodes`
(`/node/<i>/pof/verify`), never the coordinator.

**Built 2026-10-24 — steps 4–6 (the signing path, `credential-pof`); IMPL-PASS in-window the same day, live on the testnet quorum.** Every verdict carries the Credential the node itself derives from the challenge it verified:
`Subject` = the borrower, `CredentialType` = hex("ConfidantPoF"), `Expiration` = `close_time(L)` (read from the ledger,
immutable once validated) + 4 s per ledger from `L` to `E` — the same value on every node — and `URI` = hex of the UTF-8
text `confidant/pof/v1|X|L|E|<16 hex of BLAKE2b-64("ConfidantPoFURI_", challenge bytes)>`: the threshold and the window in
clear (decision (d): a loan broker reads one field), the lender bound by digest, not named. `POST /pof/credential` on the
coordinator asks its node, autofills `CredentialCreate { Account: pool, Subject, CredentialType, Expiration, URI }` and asks
every sidecar; each sidecar **re-verifies the proof on its own node now** (`POST /pof/verify`: own state, unspent re-checked
at signing time — requirement 5) and signs only a `CredentialCreate` whose five fields equal **its node's** credential
fields, with Fee / Sequence / LastLedgerSequence pinned as for payouts and no memos (requirements 1–4); k blobs are
multisigned and submitted. The borrower then signs `CredentialAccept` in the app; a lender reads the accepted credential
from the ledger and matches its URI against the package (lender page, step 3). Nothing about the proof is persisted or
logged on any node (requirement 6).

**Bounds on the signing path (the review's finding: an attacker-chosen `Subject` could make the quorum spend fees and
lock reserves without limit).** The Credential fields are derived only when the request asks (`credential: true`), and
only when **the node's own ledger reads** allow it — a coordinator cannot bypass them, because a sidecar signs only its
node's fields: (A) when the pool's credential gate is on, the borrower must hold the gate's accepted credential (a
proof-of-funds Credential goes to eligible accounts only); (B) the borrower must not already hold a proof-of-funds
Credential from this pool, accepted or pending (one per account; `tecDUPLICATE` would refuse anyway, the node refuses
first); (C) the pool account's outstanding (unaccepted) proof-of-funds Credentials must be under `POF_CREDENTIAL_CAP`
= 8 — reserve exposure ≤ 8 × one owner reserve, fee spend ≤ 8 per expiry window. Otherwise the verdict carries
`credential: null` and the reason in `credentialRefused`; the sidecar and the coordinator refuse with it (409). Reserve:
the pool account carries one owner reserve per credential until the subject accepts it; expired ones may be deleted by
anyone (not automated). The production shape is the borrower's own XRPL signature over the challenge in the request
(consent before issuance) — not for the event.

**Two conditions on the cap (audit, 2026-10-24).** The node counts one `account_objects` page (400 objects) and treats a further page as "more than the cap": the borrower's and the pool's credential lists are never judged from a partial page (withheld with a reason). That is sound only while the cap stays well below the page size — `POF_CREDENTIAL_CAP` is 8 and a compile-time assertion keeps it ≤ 200; raising it near or above the page size needs true pagination first. And the cap is not a hard ceiling: nodes count independently, so concurrent requests can overshoot by the in-flight count (bounded by the four verification permits per node) — a griefing bound, not an exact invariant. A ledger read that fails while a Credential is asked for is a ledger error (502), never a derivation; the plain verdict needs no such read.

- **What reaches the ledger:** the borrower's account, the type, the expiry and a hash of
  (X, L, E). No notes, no nullifiers, no proof. **Decided (d): X is in the URI in clear** for the
  event — a loan broker reads one field. Hash-only, with X given off-ledger, keeps the threshold off
  the public ledger and stays an option to add later.
- **Who sees the nullifiers:** the nodes only (audit Q-POF-3 (i)): "only the quorum can link the later
  spend", not the lender.
- **Freshness:** `Expiration` makes the snapshot nature visible on the ledger; after E the
  Credential is dead and anyone may delete it.
- **Lender side** (checked 2026-10-14 against the XLS-66 and XLS-65 specs): the Lending Protocol has
  **no on-ledger borrower gate** — it issues uncollateralized fixed-term loans and "relies on
  off-chain underwriting"; `LoanSet` is signed by both the borrower and the loan broker's owner
  (`CounterpartySignature`). So the broker "requires" the Credential **by policy**: it counter-signs a
  `LoanSet` only for a borrower holding an accepted, unexpired `ConfidantPoF` Credential from the
  pool account. That is exactly the underwriting slot proof of funds fills. On-ledger gating by
  Credential exists on the **lending side**: a private Single Asset Vault (`tfVaultPrivate` +
  `DomainID`) admits depositors only through a Permissioned Domain (XLS-80) — the same shape as
  option A. Loan state for an escrow arbiter (§4) is on the `Loan` object (`lsfLoanDefault`, set by
  the broker via `LoanManage` after `GracePeriod`).
- **Shared with option A:** reading Credentials from the ledger and multisigning
  `CredentialCreate`/`CredentialDelete` belong in one Credential module in the node and the sidecar,
  as the audit advises.

**What the Credential means** (on the object, not only in the Tier 2 note): "the borrower
**controlled** ≥ X as of ledger index L", **not** "controls it now". Between signing and the
lender's read the holder can spend; without Tier 2 that window is unguarded. A lender who treats an
unexpired Credential as current solvency is trusting the borrower not to have spent. This line goes
on the lender surface next to "signal, not proof of ownership".

**Requirements for the `credential-pof` signing path** (review on PR #27, 2026-10-14; the audit's
design view in `docs/audit/RESP-pof-ledger-tiers-2026-10-14.md`, not an authorization — the firsthand IMPL-PASS is owed on the built slice):
1. **Ask your own node.** A sidecar signs only on its own node's verdict for exactly
   (borrower, X, L, E), computed from that node's state — never on the coordinator's claim (the same
   shape as `fee-sweep`).
2. **Stand-alone kind.** Never combined with an anchor or a payout in one request; no memos; `Fee`,
   `Sequence` and `LastLedgerSequence` pinned as for payouts.
3. **Shape pinned against the verdict:** `Account` = pool, `TransactionType` = `CredentialCreate`,
   `Subject`, `CredentialType`, `Expiration` = E, and `URI` = **the node's own recomputation** of
   H(X ‖ L ‖ E) from the values it verified — never the coordinator's hash echoed back. The node signs
   its own computation or refuses.
4. **Freshness checked by the node** against the validated ledger index it reads itself: L within a
   window, E ≤ L + max.
5. **Unspent re-checked at signing time — mandatory.** The gap between `POST /pof/verify` and the
   signature is a live window; a cached verdict replayed after the funds moved is the attack this
   closes. A verdict cache is safe only behind this re-check.
6. **Never persisted, never logged** — a hard invariant of the endpoint. Only the nullifier watch
   list is kept, privately.

**Operations** (not trust-path, but in the design):
- **Reserve** (XLS-70, checked 2026-10-14): a Credential costs one owner reserve to the **issuer
  until the subject accepts it**, then to the subject. Anyone may delete it once expired. So the
  pool account carries reserve only for unaccepted Credentials; the quorum deletes expired
  unaccepted ones, otherwise the reserve ratchets up with issuance and eats into the pool account's
  excess.
- **Ingress cost.** `POST /pof/verify` is new unauthenticated ingress that forces a proof
  verification (~10 ms) per request: bound it per node (size limit, rate limit per client, a
  concurrency cap), or a flood of bogus records griefs the verifiers.

## 3. Tier 2 — live Credential: revoke when the funds move (cheap, not a lock)

Each node keeps the nullifiers it verified in step 3 in a **private watch list**, keyed by the
Credential. When any of them shows up in an accepted bundle — in any operation — the proven notes
have moved. The quorum then multisigns **`CredentialDelete`** for that Credential (sidecar kind
`credential-revoke`; each signer checks that its own node saw the nullifier spent).

- The lender sees the revocation **on the ledger**, so "funds still in place" becomes something the
  lender can watch, not a promise. A loan can treat revocation as a covenant breach.
- It is still **not a lock**: the holder can move the funds, and the lender only learns that they
  did. It is a tripwire.
- Privacy is unchanged from Tier 1: the nullifiers stay with the nodes. The revocation's timing
  reveals that the holder transacted at about that time, which is the same linkage the audit already
  accepted, now to the public. Label it.
- It is only as live as the quorum: if fewer than k signers act, no revocation happens before E.
  Expiration remains the backstop.

## 4. Shielded collateral escrow — a separate construction, post-event (formerly "Tier 3")

**This is escrow, not proof of funds** (§1a): the collateral moves into a note the holder no longer
controls alone. Kept here because it is the honest answer to "how would a real lock work with the
circuit unchanged", and because it reuses the same wallet and quorum; it leaves the proof-of-funds
ladder and the tiers table (§6).

**Idea:** a lock is not a proof about a note — it is **who can spend it**. The holder moves X into a
note whose **spend-authorization key is shared** between the holder, the lender and an arbiter, using
FROST threshold signatures on RedPallas (the scheme Orchard spend authorization already uses). No
single party can spend the note. The circuit, the pool and the verifier stay as they are: to them
it is an ordinary private transfer, and later an ordinary spend.

**Why it fits Orchard without changing the circuit** (checked against the crates the project uses):
- An Orchard address comes from a full viewing key `(ak, nk, rivk)`; `ak` is the
  spend-validating key. `FullViewingKey::from_bytes(ak ‖ nk ‖ rivk)` exists in `orchard` 0.15.5, so
  an address can be built from an `ak` that **no single party holds**.
- Spend authorization is a RedPallas signature under a re-randomized key `rk = ak + [α]G`.
  `reddsa` 0.5.2 (already in `Cargo.lock`) has `frost::redpallas` with re-randomized FROST and the
  even-y adjustment Orchard requires for `ak` (`into_even_y`).
- `orchard` 0.15.5 accepts signatures produced outside the wallet: `Bundle::prepare` /
  `append_signatures` and the PCZT signer role (the path Zcash uses for FROST and hardware wallets).

**Parties and keys:** a 2-of-3 FROST group {holder, lender, arbiter}. Distributed key generation
yields the group key `ak`; `nk` and `rivk` are shared by all three, so each can see the note and
compute its nullifier. The **arbiter is the pool's quorum**: its share is held k-of-N by the
signers. That nesting is the hardest part — for a demo, a single arbiter key with the label "would
be k-of-N in production".

**Lifecycle**

| Step | Who signs | Ledger sees |
|---|---|---|
| Lock: holder transfers X into the joint address | holder alone (from the holder's own notes) | one ordinary anchor memo — indistinguishable from any transfer |
| Lender checks the lock | nobody; the lender trial-decrypts with the joint viewing key and checks the note is unspent | nothing |
| Release after repayment | holder + lender, back to the holder | one ordinary anchor |
| Release if the lender disappears | holder + arbiter — the arbiter's signers sign only after reading **on the ledger** that the loan is repaid | one ordinary anchor |
| Liquidation on default | lender + arbiter — the arbiter's signers sign only after reading **on the ledger** that the loan is in default | one ordinary anchor; the lender unshields later (public, like any exit) |

**What this gives:** a real lock enforced by key control, not by a promise and not by the pool's
rules; the on-ledger loan state (an XLS-66 loan, or any ledger object both sides agree on) drives the
arbiter, so the release and liquidation policy is checkable by anyone. No proof of funds is involved
and no nullifier is revealed — the lender sees exactly one note, the collateral, and nothing else of
the holder's. It is an escrow: what the holder gives up is sole custody of the collateral.

**Honest limits** (the audit's two lines, to be stated next to the custody caveat, as the defining
properties of this product)
- Two of three can move the collateral: lender + arbiter can liquidate, holder + arbiter can
  release. The arbiter is the quorum, so **the same k signers can now also co-sign, with either the
  lender or the holder, the spend of a specific shielded collateral note** — a materially larger
  blast radius than "drain the pool account's public XRP"; its own line next to that caveat.
- **The joint viewing key gives the lender permanent sight of that one note**, past the loan term. The
  pool's privacy claim must say it does not cover escrowed collateral.
- Orchard has no time locks; "release after T if nobody acts" needs the arbiter.
- Key generation is interactive (three parties online once), and the joint viewing key lets the
  lender see that one note forever.
- Cost: core gets an external-signature path and joint address support; the wallet gets a FROST
  round; the quorum gets an arbiter service with ledger-checked policies; all of it is trust-path
  work with its own audit cycle. **Not event scope.**

## 5. Tier 4 — circuit-level (post-event): the only lock without moving

- A proof of funds that reveals **no nullifiers** (a membership-and-unspent proof against a
  nullifier-set accumulator) — needs a new circuit.
- Notes with **spending conditions** (time locks, covenants) — needs a new circuit.

Both are outside the "Orchard reused unchanged" line (ROADMAP §4.5) and stay roadmap items.

## 6. What fits the 36 hours

| Piece | Event | Presentation only | Post-event |
|---|---|---|---|
| A: Credential-gated shielding (node + sidecar Credential module) | ✅ | | |
| C core slice (audit conditions 2–4: own record type, break tests both directions, coverage 100 %) | ✅ | | |
| C Tier 1: `POST /pof/verify` on nodes, sidecar kind `credential-pof`, quorum-issued expiring Credential | read side ✅ built 2026-10-24 (endpoint on every node, wasm `buildProofOfFunds`, `/config.nodes`); signing path ✅ built 2026-10-24 (`credential-pof`, `POST /pof/credential`, the app's record + confirm, the lender's ledger check) — **IMPL-PASS in-window, live 2026-10-24** | — | |
| C Tier 2: revoke on spend | stretch | ✅ | |
| Shielded collateral escrow (§4; not a PoF tier) | | ✅ — one slide, as a separate construction | ✅ |
| Tier 4: circuit-level | | one line | ✅ |
| UI surfaces (FE team): credential issue/accept (A), "prove funds" in the wallet, lender page reading the Credential | ✅ built 2026-10-24, rebuilt 2026-10-25 to one button (`docs/UI-RULE.md`): Compliance row + provider page (A); **Prove funds** = one press — whole balance, own account, generated request id, 24 h → quorum Credential → the account's confirmation → notes refreshed by the wallet; `lender.html` reads the Credential from the ledger by address (C; no package, no per-node table on the lender's side) | | |

The shared Credential module is what makes A and C Tier 1 one piece of work rather than two.

**Ship rule, above the schedule** (audit, 2026-10-14): a **new signing path ships only with its own
firsthand IMPL-PASS inside the window**; a read gate does not carry that risk.
- **A** is a ledger read plus refuse-at-submit-and-endorse, fail-closed — the screening gate's shape,
  already IMPL-PASS'd once; the sidecar needs nothing new. A's REQ goes to the audit first.
- **Tier 1** (`credential-pof`) is a new signing path: it ships only if A lands **and** Tier 1 gets its
  own IMPL-PASS in-window. Otherwise the flow diagram on the slide is the correct outcome, not a
  failure. A demo deadline never ships an unverified signing path.
- **Tier 2** (`credential-revoke`) is another signing path plus a node state change (watch list,
  revocation): stretch at most, under the same rule.

**Track: Lending & Borrowing** (decided 2026-10-14, ROADMAP §0 (f)); **network: the pool stays on testnet** (decision (g) revised 2026-10-23); the Lending Protocol is
enabled only on devnet, so the broker/loan side runs there with separate accounts, or on slides. Proof of funds is an
underwriting signal a loan broker on the XRPL Lending Protocol checks before lending; escrow and
Tier 4 are the "what comes next" for collateral.
