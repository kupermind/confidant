# Proof of shielded funds, and collateral escrow as a separate construction — proposal

**Status: PROPOSAL (2026-10-14), not built, nothing here is decided.** Input: the pre-event spike
(draft PR #29), the audit's conditional DESIGN-PASS (`docs/audit/RESP-pof-design-2026-10-14.md`)
and the apply-path guards it required first (PR #31, PASS). This document fills what the spike
leaves out — the part that lives **on the XRP Ledger** — and proposes how a proof of funds becomes
a **lock**. It doubles as the source for the pitch's "what comes next" slides. Decisions are in
ROADMAP §0.

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
| 5 / Q-POF-5: a snapshot, not a lock | a quorum-enforced lock is out of scope for the event | **a lock** — §4 |

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

- **What reaches the ledger:** the borrower's account, the type, the expiry and a hash of
  (X, L, E). No notes, no nullifiers, no proof. X can be in the URI in clear text, or only as a hash
  with X given off-ledger — the owner's choice (a clear X is simpler for a lender, a hashed X keeps
  the threshold off the public ledger).
- **Who sees the nullifiers:** the nodes only (audit Q-POF-3 (i)): "only the quorum can link the later
  spend", not the lender.
- **Freshness:** `Expiration` makes the snapshot nature visible on the ledger; after E the
  Credential is dead and anyone may delete it.
- **Lender side:** a loan broker accepts a borrower only with an accepted, unexpired
  `ConfidantPoF` Credential from the pool account. Where the lending stack supports gating by a
  Permissioned Domain (XLS-80), the domain lists this issuer and type. *To verify before the
  event:* which of XLS-65 / XLS-66 / XLS-80 are enabled on testnet or devnet, and how a loan
  broker can require a Credential.
- **Shared with option A:** reading Credentials from the ledger and multisigning
  `CredentialCreate`/`CredentialDelete` belong in one Credential module in the node and the sidecar,
  as the audit advises.

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
| C Tier 1: `POST /pof/verify` on nodes, sidecar kind `credential-pof`, quorum-issued expiring Credential | ✅ if A lands by Saturday evening | otherwise the flow diagram (§2) | |
| C Tier 2: revoke on spend | stretch | ✅ | |
| Shielded collateral escrow (§4; not a PoF tier) | | ✅ — one slide, as a separate construction | ✅ |
| Tier 4: circuit-level | | one line | ✅ |
| UI surfaces (FE team): credential issue/accept (A), "prove funds" in the wallet, lender page reading the Credential | ✅ minimal | | |

The shared Credential module is what makes A and C Tier 1 one piece of work rather than two.
