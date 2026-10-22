# Data availability — where the log lives, and what dies with what

*Roadmap-level design, 2026-10-23. Not an implementation plan; nothing here changes the trust model
(`SPEC.md` §4) or the hackathon scope (`ROADMAP.md` §0). It answers the question anyone who
understands the system asks first: "the ledger has no smart contract — if your server dies, what
survives?"*

## 1. The data, layer by layer

| Layer | What | Where it lives today | Permanence | Who needs it |
|---|---|---|---|---|
| **L0 — ledger** | per accepted operation, one anchor memo (≤ 1 KB): version, op, root after, bundle hash, external context (deposit tx hash / payout destination), every `(nullifier, cmx)` pair; plus the deposit and payout Payments themselves | XRPL, in the pool account's history | as long as XRPL **full-history** servers keep it (validators prune; a node joining must read a full-history server — `ensure_full_history`) | everyone: the public state (commitment tree, nullifier set, roots), the deposit → shield and unshield → payout links, the audit of the quorum's honesty |
| **L1 — bundle bytes** | per operation, the record: proof (Halo 2, a few KB), per action `epk` + `enc_ciphertext` (580 B) + `out_ciphertext` (80 B), `cv_net`, `rk`, spend-auth signatures, binding signature, `value_balance`, flags, anchor | every verifier node's memory and `verifier-state.json`; served by `GET /log`; **not** on the ledger — the anchor carries its hash | as long as at least one node (mirror) keeps it | a **new node** (`rebuild_checked` re-verifies every proof), a **holder's wallet** (finds its notes by trial-decrypting the ciphertexts), an **auditor** (reveal reads ciphertexts with the viewing key), the **fee ledger** replay |
| **L2 — holder-local** | the spending key; the decrypted notes with their leaf positions | the holder's browser (today: only the key persists; notes are re-derived from L1 at every session) | as long as the holder keeps it | the holder, to spend |

Two facts follow. **Integrity is never at risk**: L1 is checked against L0 by hash, bundle by bundle,
and every bundle is re-verified by whoever rebuilds; a mirror cannot forge, only withhold. **Availability
of L1 is the only soft spot**: without the bytes, the tree and the nullifier set still rebuild from
L0, but a new node cannot re-verify the past, and a holder cannot find notes it has not already seen.
Today every node, and therefore every copy of L1, sits on one host.

## 2. What dies with what

| Failure | Service | Pool XRP | Public state | Holders' notes |
|---|---|---|---|---|
| Coordinator or one node down | restarts; any node serves `/log`; a replacement coordinator re-drives operations (`endorse` is idempotent) | safe | intact | intact |
| **The host lost, with backups of `data/quorum`** | restore, or `--rebuild --mirror` from the backup's records | safe (signer keys restored) | intact (and re-derivable from L0) | intact |
| **The host lost, no backup** (today's single point of failure) | nodes can be rebuilt from L0 alone, **without** re-verifying the past | **stuck**: the SignerList keys are gone, the master key is disabled | tree, nullifiers, roots: recoverable from L0 | **lost** for every holder who has not kept their notes: the ciphertexts existed only in L1 |
| Operators disappear but data survives | anyone may run a node (`docs/NODE-OPERATOR.md`) and a coordinator (holds nothing) | spendable only with k signer keys — the federated caveat, unchanged | intact | intact if a mirror exists |
| A mirror serves a truncated or altered log | detected: records must match the anchors' hashes and order; missing records are visible as anchors without bytes | — | — | withholding is a liveness attack on the holders of the withheld notes; it cannot steal |
| XRPL full-history servers stop serving old ledgers | — | — | **L0 itself becomes unavailable** for the pruned range; the same dependency every XRPL history consumer has | — |

The honest line today: *state on the ledger, bytes mirrored by hash; the mirror is one host.* The
mitigations below change the last clause.

## 3. Options, with the properties that matter

Properties: **A** availability guarantee, **P** permanence, **C** cost per operation, **L** latency for a
wallet sync, **F** privacy of fetching, **D** external dependency, **Claim** the honest sentence it
licenses.

| Option | A | P | C | L | F | D | Claim |
|---|---|---|---|---|---|---|---|
| **M. Mirrors = verifier nodes on independent hosts** (status quo, properly deployed) | k-of-N nodes each hold L1; one survivor suffices | operators' lifetime | 0 | best (one HTTP fetch of the whole log) | the node sees who syncs, not which notes are whose (full download, no per-note query — `ROADMAP.md` §4.6) | the operators | "any node is a full mirror; anyone can run one" |
| **B. On-ledger chunks** (`ROADMAP.md` §4.5.1): the record's canonical bytes as ≤ 1 KB memos `confidant/bundle` = hash ‖ index ‖ count ‖ bytes, in Payments anyone may send; `rebuild_checked` reassembles and hash-checks | the ledger's own | XRPL full history | ≈ 7–8 KB per 2-action bundle → ≈ 8 Payments × ≈ 11 drops ≈ 90 drops (a fraction of a cent); scales with actions | fine: `account_tx` of one account; chunks may come from any sender, authenticated by hash, so the pool account's sequence is not serialised | reads of public history | XRPL full-history servers (shared with L0) | **"anyone rebuilds everything from the XRP Ledger alone"** (the Shielded Bitcoin paper's assumption A2) |
| **I. IPFS content-addressed mirror**: CID = multihash(blake2b-256) over the same canonical bytes, so the bundle hash already in the anchor memo *is* the address; nodes pin what they accept; snapshots of the whole log per N records with the snapshot CID in a periodic checkpoint memo | only while pinned (own kubo nodes, a pinning service, holders who pin their own bundles) | pinners' lifetime | ≈ 0 on-ledger; pinning cents per GB-month | per-CID DHT lookups are slow — use snapshots; gateways are fast | a gateway sees which CIDs a client fetches; harmless while the client fetches everything (it must, for trial decryption); snapshots make this moot | IPFS network + pinners | "a public mirror anyone can pin, addressed by the hash on the ledger" — **not** "from the ledger alone" |
| **Fc. Filecoin storage deals** for log snapshots (epochs) | contractual for the deal term, enforced by Filecoin's proofs and penalties | the deal term, renewable; the Filecoin chain's lifetime | cents per GB; minimum deal sizes favour snapshots over per-bundle objects | cold storage: retrieval is minutes, not a sync path | as IPFS | Filecoin | "archived under a storage deal anyone can verify on Filecoin" — archival, not hot |
| **Ar. Arweave** | one-time payment, permanent by design | Arweave's lifetime | cents per MB once | cold | as IPFS | Arweave | "archived permanently" — same caveat as Filecoin: while that chain lives |
| **H. Holder-side persistence**: the wallet keeps its decrypted notes, positions and its own bundle bytes (IndexedDB, exportable) | the holder's own | the holder's | 0 | n/a | best — nothing fetched for already-known notes | none | "your notes are yours to keep; the mirror is only needed for notes you have not seen and for Merkle paths, whose leaves (`cmx`) are on the ledger" |

Not viable on XRPL today: NFToken `URI` and DID `Data` fields (hundreds of bytes, not KBs), Hooks (not
on mainnet), any on-ledger blob object — memos are the only general-purpose carrier, which is why B
is shaped the way it is.

## 4. Threats the design must answer, and which option answers them

- **"Your server dies."** M (second host) answers it today; B answers it without operators at all.
- **"Your operators censor a holder by withholding their bundles."** Detectable from L0 (anchors
  without bytes). B removes the lever entirely; I/Fc/Ar remove it as long as someone else pinned or
  archived; H means the holder was never exposed for notes they had already seen.
- **"Who guarantees the bytes exist at anchor time?"** Today the k endorsing nodes each hold them.
  With B, the rule becomes **publish-before-anchor**: a signer refuses to sign the anchor until the
  chunks of that bundle are on the ledger (it can publish them itself — it has the bytes from `endorse`).
  This is an availability rule, not a backing rule; see §6.
- **"Fetching reveals which notes are mine."** No option here changes the invariant that a wallet
  downloads the whole log and trial-decrypts locally (`ROADMAP.md` §4.6); snapshots keep IPFS fetches
  coarse; B's `account_tx` reads are indistinguishable from any explorer's.
- **"You depend on another chain."** I/Fc/Ar do. B does not, beyond XRPL full history. The design
  keeps B as the canonical layer and treats I/Fc/Ar as redundancy, so the honest claim never rests on a
  second chain.
- **"The ledger prunes."** Shared with every XRPL application that reads history; mitigated by
  snapshots (I/Fc/Ar) that let a node rebuild from an archive plus the recent ledger.

## 5. Staged plan

| Stage | When | What | Honest label after it |
|---|---|---|---|
| **0 — ops** | before the event; no code | off-host backup of `data/quorum` (states + signer keys); one verifier node on a second host, joining with `--rebuild --mirror`; both already supported | "every node is a full mirror; we run two hosts" |
| **1 — cheap redundancy** | post-event, ≈ 1–2 days | I: a publisher next to each node adds every accepted record to IPFS with blake2b-256 raw-leaf CIDs (= the bundle hash) and pins it; `--mirror` accepts an IPFS gateway; log snapshots per N records with the snapshot CID in a checkpoint memo. H: the wallet persists its notes, positions and own bundles in IndexedDB, with export | "a public mirror anyone can pin; the ledger names every object by hash; your notes are yours to keep" |
| **2 — canonical DA** | post-event, with an audit slice | B: chunked `confidant/bundle` memos, `rebuild_checked` reassembling and hash-checking; publish-before-anchor as a sidecar rule; cost and size model recorded per bundle shape | "anyone rebuilds the whole pool from the XRP Ledger alone (full history)" |
| **3 — archive** | after 2, operational | Fc and/or Ar for epoch snapshots; a `docs/` page listing deal ids / tx ids so anyone can verify the archive exists | "archived under verifiable storage deals, independent of the operators" |

Stage 0 is the one that matters this week; it needs no change to `main` and respects the feature
freeze. Stages 1–3 are the answer to the question in the title and stay roadmap.

## 6. Audit boundaries (so the plan does not drift into the trust path)

- Availability is **never** backing. The audit's standing condition on §4.5.1 holds for every option
  here: backing remains "a release bound to a verified, consumed deposit", never "the bytes were
  published". The moment rebuild-without-mirror gates a value path, it is a trust-path slice.
- Publish-before-anchor (stage 2) is a sidecar **refusal** rule — fail-closed, like the screening gate
  and the fee-sweep pins — and gets its own slice: shape of the chunk Payments, who may send them, how a
  signer checks completeness before signing.
- Nothing above adds a reader of private data: L1 is public by construction; copies of it anywhere
  change who can **find** notes, not who can **read** them.

## 7. What to say on stage, by stage

Today: *"The ledger holds the state and names every bundle by hash; the bundle bytes are mirrored by the
verifier nodes, which anyone can run; a node can rebuild from the ledger plus any mirror and re-verifies
every proof."* Do not say "everything is on the ledger" until stage 2; do not say "permanent" until
stage 3, and then add "under storage deals on Filecoin/Arweave, while those networks run".
