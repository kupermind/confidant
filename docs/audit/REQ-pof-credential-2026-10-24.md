# REQ — proof of shielded funds (hackathon feature C): Tier 1, the signing path (`credential-pof`)

Sent 2026-10-24 (event day) from the implementation session to the audit session, verbatim (branch
`pof-credential`, stacked on the read side `REQ-pof-endpoint-2026-10-24.md` and the UI). This is a **new signing path**:
under the ship rule (`docs/PROOF-OF-FUNDS.md` §6) it ships only with its own firsthand IMPL-PASS inside the window;
otherwise the flow diagram on the slide is the correct outcome. Built from `docs/PROOF-OF-FUNDS.md` §2 steps 4–6 and
the six `credential-pof` requirements (PR #27 review, `RESP-pof-ledger-tiers-2026-10-14.md`).

```text
REQ (impl, node + sidecar + coordinator + UI; a new sidecar kind): the quorum records a verified proof of funds as an
expiring XLS-70 Credential from the pool account to the borrower.

== Design ==
Node (backend/node/src/pof.rs, node.rs): every POST /pof/verify verdict now carries `credential`, THIS node's own
  derivation from the challenge it verified — PofChallenge::credential(amount, close_time(L)):
    subject            = challenge.borrower
    credentialType     = hex("ConfidantPoF") = 436F6E666964616E74506F46
    expiration         = close_time(L) + 4 s × (E − L)   (Ripple-epoch seconds; close_time(L) read from the ledger by
                         the node itself — `ledger` with ledger_index L — immutable once validated, so every node derives
                         the same value; L unreadable → 502, no verdict)
    uri                = hex of the UTF-8 text "confidant/pof/v1|X|L|E|<16 hex of BLAKE2b-64(personal
                         "ConfidantPoFURI_", challenge bytes)>" — X and the window in clear (decision (d): a loan broker
                         reads one field), the lender bound by digest, not named; the lender recomputes the match from
                         the verdict it already holds (credential.uri), not from the digest.
  Nothing else changed in the verdict; still read-only, never persisted or logged.
Sidecar (backend/quorum/signer.mjs, lib.mjs checkTxShape): kind "credential-pof", alone in txs, with
  `pof: { record, amount, challenge }` in the body.
    1. Ask my own node: POST /pof/verify with that body — the node re-verifies on ITS state NOW (nullifiers unspent
       re-checked at signing time: requirement 5; freshness of L/E against its own validated ledger: requirement 4).
       Refusal → 409 "my node did not verify the proof of funds", nothing signed (requirement 1).
    2. checkTxShape("credential-pof"): TransactionType == CredentialCreate (per-kind type pin; Payments keep theirs),
       Account == pool, Fee ≤ ceiling, Sequence == the pool's next as the sidecar read it, LastLedgerSequence present and
       within the anchor window; then Subject, CredentialType (hex, case-insensitive), Expiration (exact integer) and
       URI (hex, case-insensitive) must EQUAL my node's `credential` fields; a missing/malformed verdict refuses; no
       memos (requirements 2, 3). The coordinator's values are never echoed back: the tx is compared against the node's.
    3. Sign with the multisign flag, return the blob.
Coordinator (backend/quorum/coordinator.mjs): POST /pof/credential { record, amount, challenge } → its own node's
  verdict (cheap early refusal) → autofill CredentialCreate { Account: pool, Subject, CredentialType, Expiration, URI }
  from that verdict, LastLedgerSequence = now + 20 → every sidecar POST /sign with the pof body → k blobs multisigned and
  submitted → { tx, issuer, subject, credentialType, expiration, uri, amount, challenge, validatedLedger, signers,
  refusals }. The record is forwarded for verification only; the coordinator keeps and logs nothing of it.
Holder UI (frontend/index.html): after k of N verdicts, "record it on the ledger as a Credential" → POST /pof/credential →
  then "Confirm with my signature": CredentialAccept { Account: borrower, Issuer: pool, CredentialType } signed by the
  holder's XRPL account in the browser (refused when the browser's account is not the challenge's borrower). Lender UI
  (frontend/lender.html step 3): account_objects type credential of the borrower, issuer == pool, type ConfidantPoF →
  accepted / issued / expired + expiry time, URI decoded, "matches the package you verified" iff URI == the nodes'
  credential.uri from the verdicts. Honest label on both: "controlled ≥ X as of ledger L", not "controls it now".
Live check (backend/quorum/pof-check.mjs step 5): POST /pof/credential → tx; fields equal node 0's derivation; the
  holder's CredentialAccept; account_objects shows the credential accepted with that URI and Expiration. Skipped with a
  note when the coordinator predates the route. Result on the live quorum is posted on the PR once deployed.
Reserve (ops): the pool account carries one owner reserve per credential until the subject accepts it; expired
  unaccepted credentials may be deleted by anyone (not automated in this slice; NODE-OPERATOR says so).
Not in this slice: Tier 2 (watch list, revocation on spend) — the Credential's Expiration is the only freshness.

== Tests ==
backend/node/tests/node.rs (proof_of_funds_endpoint_…): credential fields pinned at node level and over HTTP (subject,
  type hex == hex("ConfidantPoF"), expiration == close_time(990) + 510·4, uri == hex(uri_text) upper-case, uri_text =
  prefix|120|990|1500|<16 hex>, lender NOT named in the URI); another lender → another digest, same subject/expiration;
  another amount → another digest AND another clear field; ledger L unreadable → 502. Edge challenge at validated − 200
  also gets its close time, so the window edge still verifies.
backend/quorum/test/shape.test.mjs ("credential-pof: every field is the signer's own node's recomputation"): honest tx
  passes (hex case-insensitive); refused: no verdict / malformed expiration, a Payment, wrong Subject, wrong
  CredentialType, Expiration ±1 / missing, wrong URI, memos, foreign Account, wrong Sequence, LastLedgerSequence outside
  the window, Fee above the ceiling; an anchor that is a CredentialCreate is still "not a Payment".
Frontend: browser smoke 33/33 (prove → record → "Credential ConfidantPoF issued … Confirm" → "Confirmed ✓" with two
  activity lines; lender step 3 "accepted ✓ … matches the package you verified ✓" with the decoded URI; lender page
  layout at 320/390). Pure tests pin challengeString on both pages.
Coverage: cargo llvm-cov -p confidant-node … --fail-under-lines 95 → 96.01 % total; pof.rs 100 %, http.rs 100 %.
cargo clippy --workspace --all-targets -D warnings clean; fmt clean; quorum tests 15/15.

== Mutation table ==
Sources restored after each row. Rust rows run alone against proof_of_funds_endpoint_…; JS rows against the quorum
shape tests. 13/13 killed.
MC1 credential(): expiration ignores the window (close time only)      → killed
MC2 credential(): digest replaced by a constant (lender unbound)         → killed
MC3 credential(): digest over bytes(0) (amount only in clear)            → killed
MC4 credential(): uri not the hex of uri_text                            → killed
MC5 credential(): subject taken from the lender field                    → killed
MC6 node: ledger close time not read (0 used)                            → killed
MS1 sidecar: Subject check removed                                       → killed
MS2 sidecar: CredentialType check removed                                → killed
MS3 sidecar: Expiration check removed                                    → killed
MS4 sidecar: URI check removed                                           → killed
MS5 sidecar: memos allowed                                               → killed
MS6 sidecar: missing verdict accepted                                    → killed
MS7 sidecar: transaction type not pinned per kind                        → killed

== Asked ==
IMPL-PASS for the signing path under requirements 1–6 — or the ruling that it stays on the slide. Specifically:
  (a) Expiration = close_time(L) + 4 s/ledger: deterministic across nodes, but drifts from ledger E by the real ledger
      interval (±25 %); the Credential may outlive E by minutes. Acceptable for Tier 1, or should E become a timestamp
      in the challenge (a breaking change to the read side's challenge format)?
  (b) the URI binds the lender by an 8-byte digest; X, L, E are in clear on the public ledger (decision (d)). Confirm.
  (c) the sidecar trusts its node's `credential` fields over HTTP on localhost, as it trusts /endorse and /fees today.
  (d) the pool account's reserve exposure: one reserve per unaccepted credential, no automatic cleanup in this slice.
  (e) the sidecar's re-verification is the only "unspent at signing" guarantee (requirement 5); there is no verdict
      cache anywhere. Confirm that is the intended shape.
```

## Supplement — the dev session's finding and the bound it landed (2026-10-24)

Sent to the audit session with the REQ, verbatim excerpt:

```text
== FINDING — reserve and fee drain through an attacker-chosen Subject ==
Nothing in the node, the sidecar or the coordinator binds challenge.borrower to the requester. The borrower is a free-text classic address the holder types. So anyone holding ONE valid proof — of any amount ≥ 1 drop, in a pool they entered once — can call POST /pof/credential repeatedly with a different `borrower` each time (any existing classic address: foreign accounts, random funded accounts), and for each one the quorum (k sidecars, all passing their pins) multisigns and submits a CredentialCreate FROM THE POOL ACCOUNT. Each one: (1) costs the pool account a multisig transaction fee; (2) locks one owner reserve (0.2 XRP today) in the pool account until the subject accepts (never, for a foreign subject) or until expiry + a manual CredentialDelete. tecDUPLICATE only limits one credential per (subject, type), so the bound is the number of classic addresses the attacker can name — unbounded. Consequence: the pool account's SPENDABLE balance shrinks by 0.2 XRP per credential while the fee ledger's poolExcess does not account for reserve, so with enough credentials unshield payouts start failing (tecUNFUNDED_PAYMENT) — a griefing attack on withdrawals from an unauthenticated endpoint, plus a fee-drain. The amplification is also k + 1 SNARK verifications per request.
```

The audit confirmed the finding at the ref and withheld the IMPL-PASS for the signing path until a bound
landed (floor: the outstanding-credentials cap, node-side; preferred: plus the compliance-gate check on the
borrower; the borrower's own signature over the challenge and the owner-reserve belt on `poolExcess` recorded
as must-land before real value). The bound landed on the branch the same night: `Node::pof_issuance_refusal`
— (A) borrower admitted by the compliance gate when it is on, (B) fewer than `--pof-max-outstanding` (default
25, ≤ 200) unaccepted `ConfidantPoF` Credentials issued by the pool account, both failing closed on a ledger
read error; `verdict.credential` becomes `null` with `credentialRefused`; the sidecar and the coordinator refuse
with 409. Mutation rows: cap removed, gate check removed, accepted ones counted, further page ignored, read
failure treated as zero — each must be killed by the new test rows (table in the RESP).
