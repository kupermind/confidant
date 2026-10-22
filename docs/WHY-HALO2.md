# Why Halo 2, and why the Orchard circuit unchanged

*Decision taken 2026-10-04 (design review Q-POOL-1, `docs/audit/RESP-pool-design-2026-10-04.md`),
written up 2026-10-23. Companion to `SPEC.md` §5.1 (the parameters) and
`docs/COMPARISON-STRK20.md` (the closest production system, which made the other choice).*

Confidant is a shielded pool on the XRP Ledger: value goes in, moves privately, comes out, and a holder
can disclose their own history. The proof system is the part nobody can patch later — notes, nullifiers
and the commitment tree all inherit its properties — so it was the first decision, and it was made
against the requirements below rather than by fashion.

## 1. What the proof system had to satisfy

| Requirement | Why it matters here |
|---|---|
| **Verifiable by many independent parties, off-chain** | The XRP Ledger has no on-ledger zero-knowledge verifier for any system. Verification is done by a k-of-N quorum of independent nodes that each re-verify every proof on their own state (`SPEC.md` §4). Proofs must verify in milliseconds on commodity hardware; proof size is not on the critical path (proofs are mirrored by hash and anchored by memo, not stored on the ledger). |
| **No trust assumption that cannot be audited afterwards** | The pool already carries one federated trust assumption (the quorum, stated plainly everywhere). A proof system with a trusted setup would add a second one — a ceremony whose toxic waste nobody can later prove destroyed. One avoidable assumption is one too many for a system whose pitch is "auditable". |
| **A production, audited shielded-note circuit to reuse** | The judged work is XRPL integration, not cryptography. Writing a new note/nullifier/Merkle circuit means months of circuit engineering plus an audit cycle, and a project that could never honestly claim security during a hackathon. Reusing a circuit that has protected real value for years turns the cryptographic risk into an integration risk — which we can actually review. |
| **Proving on the holder's machine, including a browser** | The honest label "notes are non-custodial, keys and plaintext never leave the holder" requires client-side proving. The prover must compile to WebAssembly and run in seconds on a laptop. |
| **A maintained Rust implementation** | The core, the verifier node and the browser wallet are one Rust codebase; a C++ or JavaScript-only prover would have split it. |

## 2. The options, as weighed in October 2026

| | Trusted setup | Proof size / verify | Production shielded-note circuit to reuse | Browser prover | Verdict |
|---|---|---|---|---|---|
| **Groth16** (BN254; circom/snarkjs; Railgun, Tornado, Shielded Bitcoin paper) | **Circuit-specific ceremony**, repeated for every circuit change | smallest proofs (≈ 200 B), ≈ 1–2 ms pairing verify | Railgun's circuits — a different note format and compliance model | yes (snarkjs, slow) | Best on-chain economics, but on XRPL there is no on-chain verifier to economise for, and it adds the one assumption we wanted to avoid |
| **PLONK over KZG** (universal setup; Aztec/Noir, PSE halo2-kzg) | **Universal ceremony** (one SRS, reusable) | small proofs, fast pairing verify | none for shielded notes we could take unchanged | yes | Weaker setup requirement than Groth16, still a setup; no circuit to inherit |
| **STARKs** (hash-based; Starknet's STRK20, Stwo) | none | large proofs (tens–hundreds of KB), heavier proving; verification cheap in a STARK-native VM | STRK20's pool is a Cairo contract tied to Starknet's VM, not a portable circuit | heavy; STRK20 proves on a remote service (≈ 29 s on a 12-core machine per its docs) | Right answer on a chain that verifies STARKs natively; nothing to reuse on XRPL, and the browser story is the weakest |
| **Bulletproofs** (Monero, Mimblewimble) | none | small proofs, but **linear-time verification** | range proofs only; no shielded-transfer circuit of ours to reuse | n/a | Fine for ranges, wrong tool for a full spend circuit verified by many nodes |
| **Halo 2 with the IPA commitment** (Pallas/Vesta; Zcash Orchard) | **none** | proofs of a few KB; verify ≈ 10 ms per bundle natively — fine off-chain | **the Zcash Orchard Action circuit**: live on Zcash mainnet since NU5 (2022), audited before activation, maintained by the Zcash teams as the `orchard` / `halo2_proofs` crates | yes — WebAssembly, rayon multithreading | **Chosen** |

Halo 2 was the only option that met every row at once: no setup, a shielded-note circuit already in
production with its own audit history, a Rust prover that compiles to WebAssembly, and verification
cheap enough for a quorum that re-verifies everything.

## 3. What "reused unchanged" buys, and what it costs

**Bought.** The Orchard Action circuit gives, per action, membership of the spent commitment under an
anchor (Sinsemilla tree, depth 32), knowledge of the spending key (RedPallas spend authorization),
correct nullifier, correct output commitment, and hiding value commitments tied to the public value
balance by the binding signature (`SPEC.md` §5.1). Everything Confidant adds lives *around* that
circuit: deposit binding through the XRPL memo (`delivered == value_balance`), the anchor memos that
make the state rebuildable from the ledger, the k-of-N endorse protocol, the protocol fee, the
screening gate. Each of those is reviewable Rust and JavaScript with tests and a mutation table — not
new cryptography. The audit cycle in `docs/audit/` is a cycle over integration, which is why it could
run in days.

**Costs accepted, and why they are acceptable here.**

- *Verification is slower than pairing-based systems* (≈ 10 ms vs ≈ 1 ms). Irrelevant off-chain, and
  there is no on-ledger verifier on XRPL for any system today.
- *Proofs are a few KB, not 200 bytes.* They are never written to the ledger; the ledger carries a
  ≤ 1 KB anchor memo per operation and the bundle bytes are mirrored by hash (full on-ledger data
  availability is a roadmap item with its own cost model, `ROADMAP.md` §4.5.1).
- *Pallas and Vesta are not XRPL's curves.* By design nothing XRPL-related is proven inside the
  circuit: XRPL signatures (secp256k1 / ed25519) are made by `xrpl.js` outside the proof, and the
  deposit is bound by a memo every node checks against the ledger. No non-native field arithmetic,
  no lookup-table curve libraries, no circuit changes (`docs/API-BOUNDARY.md` §6a).
- *Browser proving needs threads to be pleasant.* Single-threaded the proof takes ≈ 16 s; on 16
  threads 3.1 s, with the worker at ≈ 135 MB — measured on the live site (`SPEC.md` §5.11). Threads
  need a cross-origin-isolated page, which the deployment provides.
- *Discrete-log security, not post-quantum.* Shared with every option except STARKs; stated in the
  roadmap, not hidden.
- *The circuit cannot be extended.* Anything new — a proof of funds, a disclosure report — must be
  built as a different use of the same circuit (a separate signature domain, a distinct record type,
  read-only verification) and must leave every pool apply-path untouched. The repo enforces this with
  a test that pins every hash domain in the core and a test that every state-changing entrypoint
  re-verifies both signatures under the pool sighash before consuming a nullifier
  (`core/tests/apply_path_guards.rs`); the audit's conditions for such extensions are recorded in
  `docs/audit/RESP-pof-design-2026-10-14.md`.

## 4. What would change the decision

- **An XRPL amendment that verifies proofs on-ledger** (`SPEC.md` §15). Then on-ledger verification
  cost becomes real, and a pairing-based system — or an amendment that carries a Halo 2 verifier —
  would be re-weighed against the quorum. Until then the quorum is the verifier, and Halo 2's cost
  profile fits it.
- **A circuit-level feature we cannot express by reuse** — a proof of funds that reveals no
  nullifiers, notes with spending conditions (`docs/PROOF-OF-FUNDS.md` §5). That would mean our own
  circuit and our own audit cycle; it is post-event work, and the choice of proving system would be
  made again at that point, not inherited.
- **A production requirement for post-quantum security.** None of the discrete-log options survives
  it; the honest answer today is that STARK-style systems do, at the price of proof size and tooling.

Nothing in the hackathon scope touches these. The decision of 2026-10-04 stands: Halo 2, the Orchard
Action circuit unchanged, verification by a quorum, proving in the holder's browser.

## References

- `SPEC.md` §4 (trust model), §5.1 (circuit parameters), §5.11 (measured performance), §15 (amendment sketch)
- `docs/audit/RESP-pool-design-2026-10-04.md` (Q-POOL-1: circuit reuse, no objection)
- `docs/COMPARISON-STRK20.md` — the STARK-on-Starknet design, and why the chain decides the verifier
- `docs/BITVM-GARBLED-BRIDGES.md` — why the recent Groth16 results do not port to a Halo 2 circuit
- Zcash: the Orchard protocol and the `orchard` / `halo2_proofs` crates (https://github.com/zcash/orchard, https://zcash.github.io/halo2/)
