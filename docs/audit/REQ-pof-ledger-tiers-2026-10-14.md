# REQ — proof of funds on the ledger (owner's proposal, Tiers 1–3): design view

Sent 2026-10-14 from the implementation session to the audit session, verbatim. Relays the repo
owner's `docs/PROOF-OF-FUNDS.md` (PR #27 branch, status PROPOSAL) and his three questions, with the
dev session's read. Nothing built; the RESP is a design view, not an authorization to ship.

```text
The owner posted a proposal that fills what the PoF spike leaves off the ledger (PR #27 branch, docs/PROOF-OF-FUNDS.md, status PROPOSAL, nothing decided) and asked three questions; the lead asked me to answer with your view where it is a trust-path matter. Summary of the proposal, then the questions with my read.

== Proposal ==
Tier 1 (event candidate, trust-path slice): the holder POSTs the PoF record to each verifier node's new read-only endpoint POST /pof/verify (never via the coordinator, never logged); each node runs verify_proof_of_funds on its OWN state, checks the challenge's ledger index L is recent and expiry E ≤ L + window, and keeps the nullifiers in a private watch list. The coordinator then asks k sidecars to multisign an XLS-70 CredentialCreate from the pool account {Subject = borrower, CredentialType = "ConfidantPoF", Expiration = E, URI = H(X ‖ L ‖ E)} via a NEW sidecar kind `credential-pof`; each sidecar signs only if ITS node verified exactly this (borrower, X, L, E). The borrower signs CredentialAccept. Nothing but borrower account / type / expiry / hash reaches the ledger; nullifiers stay with the nodes (your Q-POF-3(i)). Lender reads the accepted, unexpired Credential (optionally gated by a Permissioned Domain).
Tier 2 (stretch): nodes watch the verified nullifiers; when one is spent in any accepted bundle, the quorum multisigns CredentialDelete (sidecar kind `credential-revoke`, each signer checks its own node saw the spend). A tripwire, not a lock; revocation timing leaks "the holder transacted now" (labelled).
Tier 3 (slides + post-event): a real lock with the circuit unchanged — collateral moved into a note whose spend-auth key is a 2-of-3 FROST RedPallas group {holder, lender, arbiter = the pool's quorum, its share k-of-N}; FullViewingKey::from_bytes(ak‖nk‖rivk) for the joint address, reddsa frost::redpallas with into_even_y, Bundle::prepare/append_signatures/PCZT for external signatures; the arbiter's signers release or liquidate only after reading loan state on the ledger. Tier 4: circuit-level, post-event.
Proposed scope cut: event = A (credential-gated shielding) + C core slice (your conditions 2–4) + Tier 1 only if A lands by Saturday evening; Tier 2 stretch; Tiers 3–4 slides.

== Owner's questions, with my read ==
Q-PF-1 Is the sidecar rule for `credential-pof` right: sign only if your own node verified exactly (borrower, X, L, E)?
 My read: yes, as the binding point, same shape as fee-sweep — and it needs the same pins: stand-alone kind (never combined with anchor/payout); tx shape pinned against the node's verdict (Account == pool, TransactionType CredentialCreate, Subject, CredentialType, Expiration == E, URI == H(X‖L‖E) as the node computed, no memos), Fee/Sequence/LLS as for payouts; the node's verdict keyed by (borrower, X, L, E) with freshness (L within a window of the validated index the node reads itself, E ≤ L + max) and RE-CHECKED at signing time that the nullifiers are still unspent on that node (a gap between verify and sign is a window); the verdict must not be derivable from the coordinator's claim (the sidecar asks its node, like /fees). Also: the record is never persisted or logged — only the nullifier watch list, privately. One ledger-side effect to name: every Credential object issued by the pool account consumes owner reserve on the pool account; expired ones should be deleted (anyone may), or the reserve ratchets — relevant to the pool-excess bound only indirectly (reserve is not Balance), but worth a line.
Q-PF-2 Does "arbiter = quorum" in Tier 3 need its own line next to "collusion of k signers can drain the pool account"?
 My read: yes — it widens what k signers can do from "move the pool account's public XRP" to "co-sign, with either the lender or the holder, the spend of a specific shielded collateral note"; and the joint viewing key lets the lender see that one note forever. Both belong next to the custody caveat and on the Tier 3 slide. (Tier 3 is slides only for the event.)
Q-PF-3 Is the scope cut realistic: A, plus C's core slice, plus Tier 1 only if A lands by Saturday evening?
 My read: A by Saturday evening is realistic (same shape as the screening gate: one new Ledger read for Credential objects, refusal at submit and endorse, fail-closed; the sidecar needs nothing new for A). Tier 1 by Sunday 02:00 is tight: it is a second trust-path slice (new sidecar kind + node endpoint + verdict cache + freshness) needing its own IMPL-PASS inside the window; the fallback (flow diagram on the slide) is right. Tier 2 changes the node's state model (private watch list, revocation signing) — stretch at most.

Your view on 1–3 (and anything you want written into the design before Saturday) goes to the owner verbatim; the REQ for whichever slices are built comes to you on-site, A first.
```

## Supplement — the lead's objection: Tier 3 is escrow, not proof of funds (2026-10-14)

Sent after the first RESP, verbatim.

```text
The project lead read the Tier 1–3 exchange and raises an Occam's-razor objection he wants your view on before anything reaches the slides:

"Tier 3 is no different from a private transfer from the borrower to whoever lends them the funds — the collateral MOVES into a note the holder no longer controls alone. Proof-of-funds schemes are interesting precisely when they prove the funds exist WITHOUT moving them. Otherwise we are re-inventing (re-discovering) escrow."

My read, for you to confirm, correct or sharpen:
1. He is right about the category. Tier 3's "lock" is achieved by transferring control of the collateral to a 2-of-3 group {holder, lender, arbiter}; that is a private ESCROW (a joint-custody collateral account with an arbiter), not a proof of funds. It proves nothing about the holder's other funds and it moves the collateral. Its only differences from a plain private transfer to the lender are (a) neither side can act unilaterally (the arbiter rule), and (b) release/liquidation are driven by on-ledger loan state — both are exactly what escrow adds over a transfer. So: a legitimate product ("shielded collateral escrow", which XRPL's native Escrow cannot do for shielded notes), but a different product with a different name, and it should not be presented as the "lockable" evolution of proof of funds.
2. The genuinely interesting object is the base PoF: an attestation that funds exist, without moving them (Tiers 0–2: snapshot, expiring Credential, tripwire). A lock WITHOUT moving funds would need either a quorum-enforced encumbrance on notes (trust-path change, you ruled out of scope) or a circuit with spending conditions (Tier 4) — i.e. within "Orchard reused unchanged" there is no PoF-lock; there is PoF (snapshot) or escrow (move). The honest framing for the deck: "proof of funds = underwriting signal, point-in-time; collateral = escrow under joint control, a separate construction".
3. Consequence for the plan: Tier 3 leaves the PoF story (and the "tiers" ladder, which implies continuity); if the owner wants it at all it becomes its own option ("private escrow for collateral", post-event, with its two caveats you named). Tier 4 stays as the only true "lock without moving" and is post-event anyway.

Questions: (Q-ESC-1) do you agree Tier 3 is escrow by another name and should be renamed/removed from the PoF ladder? (Q-ESC-2) is there ANY construction you can see, with the Orchard circuit unchanged and no quorum encumbrance, that locks funds without moving them — or is "PoF-lock" simply not a thing in our model? (Q-ESC-3) anything else the lead's point changes in your view on the scope cut (A + C core + Tier 1-if-A)? Your answer goes to the owner verbatim on PR #27.
```

