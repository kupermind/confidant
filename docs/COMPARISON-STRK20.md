# Confidant vs STRK20 — a comparison (2026-10-14)

STRK20 is the closest production system to Confidant's design: a note-based shielded pool with
nullifiers, viewing keys, mandatory deposit screening and auditor disclosure — on Starknet, by
StarkWare, live on mainnet since 2026. Confidant is the same family of design on the XRP Ledger,
built for the XRPL hackathon and running on testnet. This page states where the two agree, where
they differ because of the chain underneath, and where they differ by choice. Everything about
STRK20 is taken from its own documentation and public posts as of the date above (sources at the
end); everything about Confidant from `SPEC.md`. Russian: `COMPARISON-STRK20.ru.md`.

## 1. One table

| | **Confidant** (XRPL) | **STRK20** (Starknet) |
|---|---|---|
| Status | Testnet; hackathon project (NYC, Oct 24–25, 2026); a 2-of-3 quorum and the browser wallet deployed on a testnet host | Mainnet: the standard shipped 2026-03-10, first asset (strkBTC) 2026-05-12; Sepolia testnet; integrations with AVNU, Ekubo, Vesu, Endur, a CCTP bridge |
| Assets | XRP (the core is token-agnostic; issued/MPT tokens are an open decision) | any ERC-20 on Starknet |
| Proof system | Zcash **Orchard Action circuit reused unchanged**; Halo 2 (IPA over Pallas/Vesta), **no trusted setup** | **STARKs** (Stwo prover) over a "virtual Starknet execution" of the action |
| Where proofs are made | In the holder's browser (WebAssembly; ≈ 3 s on 16 threads, ≈ 16–20 s single-threaded) or natively (≈ 0.9 s). Keys and notes never leave the page | A **proving service** (the SDK sends the signed invocation to it; "run your own prover" is possible); the docs cite ≈ 29 s on a 12-core / 46 GiB machine |
| Where proofs are verified | By a **k-of-N quorum of verifier nodes**: each re-verifies on its own state and signs only roots it computed; the pool account is an XRPL SignerList with the master key disabled; every accepted operation is anchored in a ledger memo, so anyone can rebuild the state | **On-chain, in the pool contract** — Starknet verifies STARKs natively; no operator quorum is needed for correctness |
| Custody of pooled funds | Federated: **collusion of k signers can drain the pool account, but cannot spend anyone's notes** (notes are non-custodial). Said in every document | The pool contract; the contract is **upgradeable by the Starknet Security Council (7 of 12)**, per the OpenZeppelin audit "with no delay" |
| Note model | Orchard notes; commitment tree + nullifier set in every verifier; nullifiers derived from the spending-side key | Notes = (owner, token, 128-bit amount); Poseidon-derived note ids and nullifiers computed from the **viewing key** and a channel key; channels/subchannels for discovery; "open notes" with plaintext amounts for DeFi |
| Hidden inside the pool | Sender, receiver, amount and linkage of internal transfers | Sender, receiver, amount, token, which notes were spent |
| Public at the boundary | Shield and unshield amounts and XRPL addresses; timing. **An empty pool has no privacy**; pre-populate | Deposit and withdrawal amounts and addresses; timing; the submitter unless a paymaster is used; shadow-account balances; channel-open linkability (their own caveat) |
| Keys | One secret in the browser (the spending key); the **full viewing key** is derived from it and is what a holder discloses; a viewing key cannot spend | A separate **viewing key**, registered on-chain once and immutable; it decrypts and derives nullifiers but cannot spend (the Starknet account signs); must be backed up separately from the wallet — losing it loses access to notes |
| Deposit screening | **Optional, per node**: configured public lists (e.g. OFAC's XRP entries), a standby of N ledgers, canonical addresses; refused at submit and at endorse (403); re-auditable from the public log. Honest label: gatekeeping on public data, **not** a proof of innocence | **Mandatory and enforced on-chain**: Elliptic screens the depositing address, an FPI-managed signer attests, "a deposit without a valid screening signature is rejected onchain"; a self-hosted prover cannot bypass it |
| Disclosure to an auditor | **Holder-initiated**: the holder hands their full viewing key (whole history) to whom they choose; nothing is escrowed; one-payment disclosure is roadmap | **Escrowed at registration**: the user's viewing key is encrypted to the auditor's public key and stored on-chain; the key is held by FPI behind a 3-of-4 multisig (StarkWare + FPI) with a 7-of-12 Security Council backup; released under a defined "lawful request"; scoped windows claimed |
| Who the client talks to | Any verifier node's public log (`GET /log`), downloaded whole and trial-decrypted — **no "which notes are mine" lookup**; the coordinator holds nothing | A discovery provider (channels), the proving service (sees the signed invocation unless self-hosted), a paymaster (sees the sender's IP, per their docs) |
| Fees | A **protocol fee** in basis points on shield and unshield (transfers free), credited net, accounted per node, replayable from the ledger, swept to a treasury k-of-N; XRPL transaction fees paid by the pool account | No protocol fee mentioned; Starknet gas; a paymaster for sender privacy |
| Composability | None today (XRPL has no smart contracts); XLS-56 Batch for atomic anchor + payout is roadmap | Anonymizer contracts and **shadow accounts** (per-dapp pseudonymous positions): private swaps, lending, staking, a cross-chain bridge |
| Review | Continuous internal audit cycle (design + implementation rulings in `docs/audit/`, 100 % core coverage, mutation tables); no external audit yet | OpenZeppelin audit of the Cairo contracts (2026-03, 11 findings: 0 critical/high, 2 medium — resolved) and a formal verification of the pool by StarkWare |

## 2. Same design, different chain

The two systems agree on the shape: a pool of encrypted notes, a commitment tree, nullifiers that
reveal nothing but prevent double spends, a viewing key that reads but cannot spend, mandatory or
optional screening at the public boundary, and selective disclosure instead of "no disclosure".
Both say the same honest thing about the boundary: amounts and addresses of deposits and
withdrawals are public, and only movement inside the pool is hidden.

The structural difference is the chain. Starknet verifies STARKs natively, so STRK20's correctness
needs no operator: the contract refuses an invalid proof. The XRP Ledger has no on-ledger
zero-knowledge verifier, so Confidant replaces the contract with a **k-of-N quorum of independent
verifiers** that each re-verify every proof and every deposit on their own state and sign only
roots they computed themselves. That buys a working shielded pool on XRPL today at the price of a
federated custody caveat (`SPEC.md` §4), and it is why Confidant anchors every operation on the
ledger: anyone can rebuild the verifiers' state from the ledger and the public log and check that the
quorum signed only what the rules allow. The path to parity is an amendment that adds a verifier to
the ledger (`SPEC.md` §15); until then Confidant says "federated", never "trustless".

## 3. Differences by choice

- **Proving in the browser vs a proving service.** Confidant proves in the holder's browser
  (the Orchard circuit compiled to WebAssembly, multithreaded under a secure context); nothing but
  the finished bundle leaves the page. STRK20's SDK sends the signed invocation to a proving service
  that executes it and returns a STARK; running one's own prover is supported but is not the default
  path. The trade is speed and hardware on the user's side against a service that sees the
  transaction's contents.
- **Holder-initiated vs escrowed disclosure.** In Confidant the holder keeps the only copy of the
  viewing key and discloses when they decide; the operators cannot read anyone's history, and the
  design says so. In STRK20 every registered user's viewing key is encrypted to the auditor's key
  at registration, and FPI, behind a multisig and a defined lawful-request process, can decrypt a
  user's history. STRK20's is the stronger compliance guarantee (no cooperation from the user is
  needed); Confidant's is the stronger privacy guarantee against the operator. Neither is "right";
  they serve different regulatory postures, and Confidant's roadmap (§4.5.3) adds one-payment
  disclosure so a holder can disclose one payment instead of their whole history.
- **Optional per-node screening vs mandatory on-chain screening.** Confidant's gate is a node
  policy on public lists with a replayable audit; STRK20's is an attestation by a named screening
  provider verified by the contract. Hackathon option A — gating shields on an XLS-70 Credential
  issued by a screening issuer and checked at submit and endorse — is exactly STRK20's attestation
  shape on XRPL's native primitives (ROADMAP §0).
- **Note discovery.** Confidant's wallet downloads the whole public log and trial-decrypts; no
  server learns which notes are whose (ROADMAP §4.6 keeps this as an invariant). STRK20 uses
  channels derived from the viewing key so a recipient scans only its channels, which scales better
  and needs a discovery provider. If Confidant's log outgrows full download, the acceptable successors
  are per-range public filters or PIR, not per-note queries.
- **Fees.** Confidant charges a configurable protocol fee on shield and unshield (Railgun's model),
  verifiable by replaying the ledger; STRK20 documents no protocol fee.
- **Composability.** STRK20's anonymizers and shadow accounts let shielded funds use DeFi; XRPL has
  no smart contracts, so Confidant's scope is transfers, exits and disclosure, with atomic anchor +
  payout (XLS-56) and full on-ledger data availability as roadmap items.

## 4. What Confidant takes from this comparison

1. **Screening as an on-ledger attestation** (option A): the same guarantee as STRK20's FPI-signed
   deposit, expressed as an XLS-70 Credential from a screening issuer, checked by every node at
   submit and endorse, with the refund path for refused deposits.
2. **Scoped disclosure**: one-payment and per-window reports (§4.5.3) so disclosure is proportional,
   which STRK20 also claims for its auditor path.
3. **An external audit and formal reasoning before any real value**: STRK20 shipped with an
   OpenZeppelin audit and a formal verification of the pool; Confidant's internal cycle is not a
   substitute for that.
4. **Key backup UX**: STRK20's separate, immutable viewing key is a documented footgun (lose it,
   lose the notes). Confidant derives the viewing key from the one wallet secret, so there is one
   thing to back up — keep it that way and say so in the UI.
5. **Not copied, on purpose**: viewing-key escrow to an operator-held auditor key, and a default
   remote proving service. Both are legitimate choices for STRK20's posture; Confidant's honest
   label rests on the operators not being able to read anyone's history and on nothing leaving the
   holder's browser but the finished bundle.

## Sources

- STRK20 by Example: what is STRK20, viewing keys, notes and nullifiers, actions and proofs,
  compliance, proving configuration, builder overview, deployed contracts —
  https://strk20-by-example.org/ (pages read 2026-10-14)
- Starknet blog, "The compliance layer of onchain privacy: STRK20" (2026-07-14) —
  https://www.starknet.io/blog/compliance-layer-onchain-privacy-strk20/
- OpenZeppelin, Starknet privacy contracts audit (2026-03-18 … 03-31) —
  https://www.openzeppelin.com/news/privacy-contracts-audit
- StarkWare, "Formal verification of the STRK20 privacy pool" —
  https://starkware.co/blog/strk20-formal-verification/
- Starknet, STRK20 build page — https://strk20.starknet.io/build
- Confidant: `SPEC.md` (§4 trust model, §6 fee model, §15 amendment sketch), `ROADMAP.md`
  (§0 hackathon options, §4.5.3, §4.6), `docs/audit/`
