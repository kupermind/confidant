# BitVM & garbled-circuit Bitcoin bridges — research note

*Prep for BitDevs NYC Socratic Seminar #163 (Chaincode Labs, 2026-10-22), two days before the
hackathon. Two of the posted topics are directly relevant to this repo: a Ledger spy-implant
hardware-wallet hack, and garbling Groth16 for BitVM bridges (the two papers below). Written for
the crypto/ZK lead; background, not spec. Sources are linked; numbers are the papers' own.*

## 1. The two papers on the BitDevs topic list

| Paper | Who | Result | Why it matters |
| --- | --- | --- | --- |
| [Garbling Groth16 with Native Group Operations](https://eprint.iacr.org/2026/2100) (ePrint 2026/2100, Sep 2026) | Khambhati (UCLA), Feickert, Lewe, Tiwari (Alpen Labs) | Garble Groth16 *verification* with native elliptic-curve group operations instead of a Boolean circuit. A projective **partial garbling for conditional disclosure on invalid proofs**: evaluation reveals the garbler's secret iff the proof is invalid; the proof stays public. A rekeying gadget handles the pairing whose both arguments come from the evaluator. Size O(λ²); DDH + ROM. **BN254: 2.4 MiB garbled program vs 48 GB for Boolean garbling — over four orders of magnitude.** | Collapses the dominant cost in garbled-circuit Bitcoin bridges |
| [Argo: Leaking a Secret from an Invalid Groth16 Proof](https://eprint.iacr.org/2026/2165) (ePrint 2026/2165, Sep 2026) | Eagen, Lai (Glass Coins) | A randomized encoding of Groth16 *invalidity*: a decoder derives a secret iff the proof is invalid; DDH in one source group, **standard model**. Coupled with Argo MAC / Duty-Free Bits it yields a garbled circuit for invalidity; with BitVM, a **permissionless, optimistic Groth16 verifier for Bitcoin** (the decoder can be anyone, not fixed at setup). BN254: **~21 kB offline encoding** (1000× smaller than projective garbling); generation in tens of ms vs minutes for Yao. | "First protocol that makes permissionless, optimistic SNARK verification on Bitcoin practical" |

Related, same group, two weeks old: [The Post-Quantum Cost of Garbled-Circuit Bitcoin Bridges](https://eprint.iacr.org/2026/2383)
(Tiwari, Feickert, Oct 2026) — the pairing-based components are post-quantum-vulnerable; PQ proving
systems shrink the circuit but balloon the proof, which then needs **multiple Bitcoin blocks to encode**.

## 2. BitVM in one page

BitVM (Robin Linus, Oct 2023) never executes a program on Bitcoin; it **verifies claims about
off-chain execution**, using the chain only as a court of last resort — the optimistic-rollup pattern
inside Bitcoin Script, with no soft fork. Sources: [Spark research](https://www.spark.money/research/bitvm-trustless-bitcoin-verification)
(Jun 2026), [Citrea, Clementine v2](https://www.blog.citrea.xyz/clementine-v2-with-garbled-circuits-and-toop/) (Jun 2025).

| Generation | Mechanism | Dispute cost | Trust / caveats |
| --- | --- | --- | --- |
| **BitVM1** (2023) | Program → NAND circuit in Taproot leaves; hash-preimage bit commitments; interactive bisection | ~70 on-chain txs; a Groth16 verifier ≈ 3 GB of Script | Two parties, verifier pre-designated. Proof of concept |
| **BitVM2** (Aug 2024) | Single-round fraud proof: a Groth16 verifier *in Bitcoin Script* (~1.1 GB, split across Taproot leaves) | 3–4 txs, ~2-week window; assert ≈ 4.8 MB, disprove ≈ 3.5–4 MB (near-full blocks) | **Permissionless 1-of-n honest verifier.** Bitlayer bridge on mainnet Jul 2025; Citrea Clementine v1 |
| **BitVM3** (Bitcoin Amsterdam 2025) | Garbled circuits: the challenger evaluates off-chain and obtains a fraud secret only if the operator lied | ≈ $9 per dispute, ≈ $0.20 per challenge tx (~1000× cheaper than BitVM2) | Research as of mid-2026; BOB proof-of-concept (cut-and-choose + adaptor signatures) on mainnet |
| **Garbled-circuit bridges** (Citrea Clementine v2, Delbrag-inspired) | Groth16 verifier as a garbled AND/XOR circuit built at setup; a dispute is **one hash check**; disprove tx ~100,000× smaller than BitVM2's; assert 1.3 MB → ~80 kB; no operator collateral; TOOP (Fairgate) removes operator liquidity | Minimal on-chain | Permissioned watchtower graph (one garbled circuit per operator↔watchtower edge), 1-of-N honest; proving the circuit's correctness takes ~14 days; round-based circuit breaker |

Standing limitations across versions: a **liveness assumption** (someone must be watching),
**fixed denominations**, a **challenge window** (≈ 2 weeks unless operators front liquidity), an
enormous implementation surface (emulated big-integer arithmetic, Taproot trees with hundreds of
thousands of leaves, formal verification still early), and — per Alpen's own paper — a
**post-quantum problem** in the pairing-based core. The two BitDevs papers attack the *size* problem
(48 GB → 2.4 MiB → 21 kB). That is why the thread calls the numbers "crazy": the cost objection to
BitVM bridges is evaporating.

## 3. Relevance to Confidant

Confidant is not a bridge: value never leaves the XRP Ledger, and the pool account is a k-of-N
SignerList of independent verifiers rather than a locked bridge contract (see `README.md` and `SPEC.md`
for the exact trust model and the honest labels; nothing here changes them). The overlap is narrower
and more interesting:

- **The pattern, not the code.** Both papers are **Groth16-specific** — they exploit the pairing
  structure. Confidant reuses the Zcash **Orchard Action circuit, which is Halo 2 (IPA, no pairings)**,
  so neither result ports directly. What transfers is the *shape*: "conditional disclosure on an
  invalid proof" / "anyone can prove a proof invalid by publishing a secret." That is a route to
  **optimistic, enforceable verification without an on-ledger verifier** — exactly the gap `README.md`
  names ("today's XRPL has no on-ledger Halo 2 verifier; an amendment adding one would make
  verification native"). Worth a line in the amendment sketch as an alternative to a native verifier.
- **Verification economics.** Today each quorum verifier re-verifies every proof and signs only roots
  it computed. An Argo-style primitive would let *any* watcher, not only quorum members, cheaply
  challenge a bad root. Whether a Halo 2 / IPA analogue exists is an open research question — a good
  thing to ask Alpen / Glass Coins people in the room.
- **Post-quantum.** Halo 2's IPA commitments rest on discrete log, so the PQ paper's point applies in
  spirit; same debate as the amendment sketch's open question on secp256k1 in XLS-96.
- **Competitive map** (likely in or around that room): Alpen Labs, Citrea (Clementine), Bitlayer,
  BOB, Fairgate (TOOP), Glass Coins, ZeroSync (Linus).

## 4. Where the sibling settlement layer stands (TRUST) — the honest comparison

BitVM is the strongest *competing* answer to "programmability on Bitcoin without a soft fork," and it
replaces multisig honeypots (Liquid's federation, the very thing that failed in September 2026) with
1-of-n optimistic fraud proofs. That is a real trust-model improvement *for a bridge*, and the room
will treat it as the right direction. Don't argue against it; locate the alternative beside it.

| | BitVM / garbled-circuit bridge | TRUST (sibling layer, Phoenix/XPerp) |
| --- | --- | --- |
| What it trusts | A 1-of-n honest *and online* verifier; the math (pairings, DDH) | The TEE manufacturer + attestation; an M-of-N operator quorum |
| Where the BTC is | **Locked in a bridge; what you use elsewhere is a claim on it** | **Never leaves its own ledger** — no wrapped representation anywhere |
| Finality | ≈ 2-week challenge window (or operator-fronted liquidity) | The chain's own: minutes on Bitcoin, seconds on XRPL |
| Amounts | Fixed denominations | Arbitrary |
| Maturity | First mainnet bridge Jul 2025; garbled variants in research | Production cluster, Mainnet demo, 20+ internal review rounds |
| Open problem | Liveness, implementation surface, PQ | Hardware trust base; operator-set decentralisation |

One-line framing: *BitVM minimises who you trust to run a bridge; the alternative removes the bridge
and trusts hardware instead. BitVM moves the BTC to an execution environment; the other brings
execution to where the BTC already is.* Different problems; they can coexist. With garbling making
bridges cheap, "bridges are expensive" is no longer a safe argument — the durable one is **no wrapped
asset, no exit window, native finality.**

## 5. Two talking points for the seminar (contribute, don't pitch)

1. **Ledger implant →** the cross-incident pattern: Coldcard (single-key entropy), Liquid
   (wrapped-asset inflation — the HSMs signed what consensus called valid), Blink (custodian), Bitget
   (spoofed authorization), now a supply-chain implant. Different failure modes, one lesson:
   keys-in-hardware is necessary; *verifiable code and no single point of trust* is the fix.
2. **Garbled bridges →** genuine enthusiasm for the 48 GB → 21 kB arc, then the question: once the
   bridge is cheap and 1-of-n, what remains is a claim on BTC somewhere else, a two-week exit and
   fixed denominations — so is moving the BTC the right primitive, or is bringing the logic to the
   BTC? Ask it as a question.
