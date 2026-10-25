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

## 5. Tier 4 — the accepted design: proof of control without revealing nullifiers

**Status: design of record for proof of funds (project lead, 2026-10-25); sent to the audit as a DESIGN-REQ the
same day; implementation post-event in audited slices.** **The implementation hand-off, with the charter
(the two prohibitions and the seven soundness invariants) at its top, is `docs/design/TIER4-PROOF-OF-FUNDS-REWRITE.md`,
authored by the audit session and committed verbatim; where the two texts differ, that document is the target.** This is the only design under which proof of funds is a
feature of the pool: the proof reveals no nullifier, no commitment and no total, so nothing the holder does
afterwards is needed, and no later spend is linkable to it. The nullifier-revealing flow built on 2026-10-24 is a
demo of the plumbing around it and is labelled as such.

### 5.1 The statement

A new Halo 2 circuit (IPA over Pallas, no trusted setup — the same proof system as the pool, `docs/WHY-HALO2.md`),
with **k note slots** (a circuit constant; k = 4 to start). It is not an Orchard Action and not a `BundleRecord`:
nothing of it can reach an apply path.

**Public inputs:** `R_S` — the note-commitment root of the pool at state S; `N_S` — the root of the pool's
nullifier set at the **same** state S (§5.2); `X` — the threshold in drops (u64); `c` — the challenge,
BLAKE2b-256 of the canonical challenge bytes `confidant/pof/v1|borrower|X|L|E|lender` (its own hash domain,
`ConfidantPoFChal_`, listed in the SPEC §11 inventory) reduced to a Pallas base-field element. Nothing else.

**Witness per slot i:** the spending key `sk`; the note `(addr_i, v_i, ρ_i, ψ_i, rcm_i)`; the Merkle path `π_i`
of `cm_i` in the commitment tree at S; the non-membership witness `ω_i` of `nf_i` in the nullifier tree at S.

**Constraints per slot:**
- **(a) ownership** — `ak`, `nk`, `rivk` derived from `sk` in-circuit as in Orchard's spend; `ivk = CommitIvk(ak, nk, rivk)`;
  the note's `pk_d = [ivk] g_d` — the note is addressed to a key the prover holds.
- **(b) membership** — `cm_i = NoteCommit(addr_i, v_i, ρ_i, ψ_i, rcm_i)`; the Sinsemilla Merkle path `π_i` leads to `R_S`.
- **(c) the nullifier, never exposed** — `nf_i = DeriveNullifier(nk, ρ_i, ψ_i, cm_i)` computed in-circuit, bit-exact
  with `orchard::Note::nullifier` (Poseidon PRF, fixed-base multiplication by K, add `cm`, extract x).
- **(d) unspent at S** — `nf_i` is **not** in the nullifier set at S: `ω_i` is a leaf `(a, next)` of the indexed tree
  (§5.2) with a Poseidon path to `N_S`, and the in-circuit comparisons `a < nf_i < next` (255-bit, by decomposition;
  `next = ∞` sentinel for the largest leaf).
- **(e) dummies** — a slot with `v_i = 0` skips (b) and (d), Orchard's dummy-spend rule, so k is a maximum, not the count.
- **(f) distinctness** — over the non-dummy slots `nf_1 < nf_2 < …` strictly (dummies last), so no note is counted twice.
- **(g) threshold** — `Σ v_i ≥ X`; every `v_i` and `Σ v_i − X` range-checked to 64 bits.

No signatures, no value commitments, no `rk`: the proof is bound to the challenge by the public input `c`; whoever
presents it cannot change `c` or `X`; ownership is proven, not signed. The verifier checks the proof against **its own**
`(R_S, N_S)` for one state S — the two roots are one atomic snapshot object in the verifier (never mixed across log
positions) — and `c` against the challenge it recomputes from the request's fields, exactly as the read side does
today. The verdict, the per-node check, the on-ledger Credential and the lender page keep their shape; what changes
is the proof type behind them.

**What the proof says:** "someone holding the keys to unspent notes worth at least X in this pool, at state S
(ledger L), answered this challenge." A snapshot, not a lock — cryptographically now, with no step for the holder
afterwards. What the lender receives, `(proof, public inputs)`, contains nothing linkable to any later spend; it is
safe to hand over and to re-verify later against the same S.

### 5.2 The nullifier set as an indexed Merkle tree

Every node already holds the set of nullifiers it has seen (the public log). For (d) it keeps the same set as an
**indexed Merkle tree** (sorted leaves, each leaf `(value, next)` pointing to the next larger value; Poseidon over
Pallas; depth 32), inserted in log order — deterministic, so every node and any third party rebuilding from the
public log computes the same root. `N_S` joins `R_S` in the verifier's state at every log position; `rebuild` and
`rebuild_checked` reconstruct it. Not anchored on the ledger in the first slice (reconstructible from the anchored
log); adding it to the anchor memo is a later option.

### 5.3 What the holder's wallet needs

The witnesses are built from public data plus the holder's keys: the commitment path `π_i` from the public log
(the wallet already rebuilds the tree to spend); the non-membership witness `ω_i` from the sorted nullifier set,
also rebuilt from the public log. Proving ≈ one Orchard bundle of k actions (seconds in the browser on 16 threads,
longer on a phone); verification tens of milliseconds on a node. Nothing about the holder's notes leaves the wallet.

### 5.4 Engineering notes

- Gadgets: `halo2_gadgets` 0.5 (Sinsemilla Merkle, Poseidon, ECC, lookup range checks). Orchard 0.15's
  `NoteCommit` / `CommitIvk` / `DeriveNullifier` gadgets are `pub(in crate::circuit)` even behind its
  `unstable-voting-circuits` feature, so the slot's ownership/commitment/nullifier gadgets are re-implemented on the
  same primitives (≈ 300–600 lines) with **bit-exactness tests against `orchard`** on random notes — a pin for the audit.
- The circuit has its own proving and verifying key (`Params`-style lazy build as today), its own proof type, and no
  op tag: the `OpTag::ProofOfFunds` builder on the wallet and `Verifier::verify_proof_of_funds` (the revealing path)
  are removed from the product path once this ships.
- Node: `POST /pof/verify` keeps its challenge, windows, bounds and verdict; the body carries the new proof and its
  public inputs instead of a record; the sidecar kind `credential-pof` keeps its pins (own node's verdict at signing
  time, fields from its own node). The outstanding-credentials cap stays (it bounds the pool's reserve, not privacy).
- Cost at the node: one proof verification per request, as today; the indexed tree costs O(log n) per nullifier insert.

### 5.5 The audit's ruling (DESIGN-PASS, 2026-10-25) and its pins

The audit session ruled the statement sound for "controls unspent notes worth ≥ X at S" and the leak surface
reduced to (R_S, N_S, X, c, k) — nothing note-level (`docs/audit/RESP-pof-tier4-design-2026-10-25.md`). Nine
pins are gates for the implementation slices; the starred ones are load-bearing for soundness:

1. ⭐ **Bit-exact nullifier.** The in-circuit `DeriveNullifier` equals `orchard::Note::nullifier` bit for bit, tested
   on random notes — otherwise (d) proves the non-membership of a phantom and "unspent" means nothing.
2. ⭐ **Atomic `(R_S, N_S)`.** One snapshot object taken at one log position; the verifier rejects unpaired roots.
   The attack otherwise: a note spent between S1 and S2 is a member at `R_S1` and a non-member at `N_S2`.
3. ⭐ **The indexed tree equals the spent set.** `N_S` commits to exactly the nullifiers the verifier's `is_spent`
   knows, rebuilt deterministically from the log; a test replays a log and checks tree membership == the set at
   every position. One missing nullifier = a spent note provable unspent.
4. ⭐ **No apply path.** The proof is not a `BundleRecord` and can never be decoded or applied as a spend —
   type-enforced and mutation-tested as in `core/tests/apply_path_guards.rs` (drive it through every mutating
   entrypoint → refused).
5. **Threshold range-sizing.** `Σ v_i` can reach `k·2^64` in the field; range-check the **sum** to the width the
   bound allows, not only each `v_i` and the difference. For XRP the pool's total value is below 2^57 drops (the
   supply), so a 64-bit range check on the sum is sound; the circuit states that bound as an assertion.
6. ⭐ **k never forces a consolidation.** If a holder had to merge notes to fit k slots before proving, that merge
   would be a spend revealing their nullifiers and a human step — the failure this design removes. k is sized so a
   realistic X needs no pre-consolidation; the wallet refuses to prove rather than consolidate.
7. **Freshness binding.** "Unspent as of S" is a current signal only while S is recent: the node accepts only its own
   current snapshot (or one within the challenge's window of `L`), and `check_window` on `L`/`E` stays.
8. **Own proving-key domain**, and the SPEC §11 hash-domain inventory gains the challenge hash domain
   (`ConfidantPoFChal_`), diffed at the ref.
9. **A mutation row on every clause (a)–(g) and on pins 1–7**; a survived mutation on any clause is an unsound proof.

Ruled sound: no signatures (c is a genuine public input in the transcript); nf strict order for distinctness; the
new circuit as a non-Action, non-record type; reusing the endpoint/sidecar scaffolding with the proof swapped and
removing the revealing proof from the product path. Interim recommendation to the owner: the revealing endpoint
**off by default** (feature flag), enabled deliberately for a demo if at all — a label documents, a flag enforces.

### 5.6 Slices

1. **Core:** `NullifierTree` (indexed, Poseidon) with its root in `PoolState`, rebuilt from the log; mutation rows on
   insertion order, sentinel, root in the snapshot.
2. **Circuit:** the statement (a)–(g); bit-exactness of (b)/(c) against `orchard`; a negative test per clause (wrong key,
   non-member commitment, spent nullifier, duplicate slot, sum < X, wrong `c`, roots from two positions); mutation rows
   per clause.
3. **Wallet:** witness builders (CLI, wasm), the challenge hash domain, no note data in the output.
4. **Node:** proof-type swap behind the existing endpoint and sidecar kind; the atomic `(R_S, N_S)` snapshot;
   fail-closed on any mismatch; the revealing path removed.
5. **UI:** the one-button shape of `docs/UI-RULE.md`; no refresh step exists any more.

Each slice gets its REQ → RESP before merge, against the pins of §5.5. Until slice 4 ships, the repository carries
the 2026-10-25 ruling: the nullifier-revealing proof of funds is a demo of the flow, not the feature.

### 5.7 Still outside this design

Notes with **spending conditions** (time locks, covenants, a lock without moving) need another circuit change and
stay a roadmap item; collateral escrow stays the separate construction of §6.

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
