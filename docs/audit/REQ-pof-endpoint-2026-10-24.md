# REQ — proof of shielded funds (hackathon feature C): Tier 1, the read side (`POST /pof/verify`)

Sent 2026-10-24 (event day) from the implementation session to the audit session, verbatim (branch
`pof-endpoint`, stacked on the core slice `REQ-pof-core-2026-10-24.md`). Built on-site from the reviewed design
(`docs/PROOF-OF-FUNDS.md` §2 steps 1–3; requirements 4 and 6 of the `credential-pof` list; the ingress-cost note).
Steps 4–6 (the `credential-pof` signing path) are **not** in this slice: the ship rule in §6 applies to them.

```text
REQ (impl, node + wallet-side bindings, no apply-path change, no signing path): the node's read-only verdict on a
proof of funds, bound to the lender's challenge.

== Design ==
backend/node/src/pof.rs
  PofChallenge { borrower, lender, ledgerIndex, expiry } — the lender's fields. Canonical bytes = UTF-8
    "confidant/pof/v1|<borrower>|<amount>|<ledgerIndex>|<expiry>|<lender>"; the wallet signs over exactly this
    string (the UI / wallet service builds it from the same fields) and the node RECOMPUTES it from the request's
    fields — the client never sends the bytes it signed over.
  validate(): borrower must be a classic XRPL address (canonical(b) == Some(b); X-addresses refused so both sides
    sign the same bytes); lender id 1–64 characters (chars, not bytes), no control characters, no '|' (the
    separator); expiry > ledgerIndex; expiry − ledgerIndex ≤ 21 600 (≈ 24 h).
  check_window(validated): ledgerIndex ≤ validated; validated − ledgerIndex ≤ 200 (≈ 13 min); expiry > validated.
    `validated` is the ledger index the node reads itself (Ledger::validated_ledger_index) — never the request's.
  PofVerdict { ok, amount, challenge, validatedLedger, root, bundleHash, meaning } — root = the node's own state
    the proof was checked against; meaning = the honest label verbatim ("… an underwriting signal, a snapshot, not
    a lock, and not proof that the borrower owns the notes").
backend/node/src/node.rs — Node::verify_proof_of_funds(record, amount, challenge):
  1. try_acquire one of POF_PERMITS = 4 semaphore permits, else NodeError::Busy (429) — the concurrency cap on an
     unauthenticated ingress that costs a SNARK verification per request;
  2. challenge.validate() → NodeError::Pof (400);
  3. validated = ledger.validated_ledger_index() (502 when unreadable);
  4. challenge.check_window(validated) → NodeError::Pof (400);
  5. bytes = challenge.bytes(amount); under the verifier lock: Verifier::verify_proof_of_funds(record, amount, bytes)
     (core: op tag, value balance == amount, anchor, nullifiers unspent + distinct, proof, both signatures under
     ConfidantPoF_v1_) → NodeError::Core (409 for the refusals); then read state().root and bundle_hash(record).
  Nothing is applied, persisted or logged: no log line, no state write, no file (requirement 6). The verdict is
  the only output and it echoes the request's public fields plus the hash.
backend/node/src/http.rs — POST /pof/verify {record, amount, challenge} → PofVerdict; route-level
  DefaultBodyLimit::max(256 KiB) (a two-action record is ≈ 19 KB) → 413 before parsing; Pof → 400, Busy → 429.
  The coordinator has no proxy for it (design: "to the nodes, never a coordinator"); the browser reaches the nodes
  at `/node/<i>/pof/verify` on the UI origin (nginx locations in backend/deploy/nginx/confidant-ui*.conf, only
  `pof/verify` and `state` are exposed) and learns the URLs from the coordinator's /config.nodes (PUBLIC_NODES env,
  default /node/<i>).
Holder side: frontend/wasm buildProofOfFunds(amount, challenge) (wasm32 check green), wallet-worker op
  proofOfFunds; wallet service POST /wallets/{id}/pof {amount, challenge}; CLI `wallet build-pof`.
backend/quorum/pof-check.mjs (npm run pof:check): live check against running nodes — credential (if the gate is
  on) + shield anchored; proof built for (borrower = the depositor, lender "pof-check", L = validated index now,
  E = L + 600); every node answers ok with root == its own /state root and an unchanged bundle count; node 0
  refuses another amount (409), a stale L (400 "not recent"), an expired E (400 "expired"), the shield record
  (409 "not a proof of funds"), a 300 KiB body (413). Result on the live quorum is posted on the PR once deployed.

== Tests ==
backend/node/tests/node.rs::proof_of_funds_endpoint_is_read_only_bounded_and_judges_the_challenge:
  canonical bytes pinned; happy path at node level (every verdict field, state snapshot and log length unchanged)
  and over HTTP; another amount → 409; another lender → 409 (InvalidSpendAuth); a shield record → NotAProofOfFunds;
  the proof record on submit_transfer → NotAPoolOperation; 400 for: non-address borrower, X-address borrower,
  empty / 65-char / '|' / control-character lender, expiry == ledgerIndex, expiry − ledgerIndex > 21 600,
  ledgerIndex > validated, validated − ledgerIndex > 200, expiry ≤ validated — each asserting the reason text;
  the edges (validated − 200, expiry = validated + 1, expiry = ledgerIndex + 21 600) still judged; a 64-character
  non-ASCII lender accepted (chars, not bytes); no validated ledger → 502; all permits held → 429 then OK again
  with every permit back; a body of 256 KiB → 413; state and log unchanged after the whole exercise.
backend/wallet/tests/wallet.rs: build_proof_of_funds → OpTag::ProofOfFunds, verified read-only by the test
  verifier, balance unchanged, InsufficientFunds beyond the balance; POST /wallets/{id}/pof → 200 with
  op "proofoffunds" and valueBalance == amount.
Coverage: cargo llvm-cov -p confidant-node --all-targets --ignore-filename-regex '(main|rpc)\.rs'
  --fail-under-lines 95 → 95.93 % total; pof.rs 100 %, http.rs 100 %. Wallet gate (90 %) run before the push.
cargo clippy --workspace --all-targets -D warnings clean; fmt clean; wasm32 check green.

== Mutation table ==
Run alone against proof_of_funds_endpoint_…; sources restored after each row. 14/14 killed.
M1  validate: borrower address check removed                       → killed
M2  validate: lender id check removed                              → killed
M3  validate: expiry ≤ ledger index accepted                       → killed
M4  validate: expiry window unbounded                              → killed
M5  check_window: ledger index ahead of validated accepted         → killed
M6  check_window: stale ledger index accepted                      → killed
M7  check_window: expired challenge accepted                       → killed
M8  bytes(): lender dropped from the signed bytes                  → killed (pinned bytes)
M9  node: concurrency bound removed (no 429)                       → killed
M10 node: challenge fields not validated                           → killed
M11 node: freshness window not checked                             → killed
M12 node: amount taken from the record, not the request            → killed (another-amount → 409 expected, got 200)
M13 node: core verification skipped                                → killed
M14 http: body limit removed                                       → killed
(M12 first "died" of a type error — value_balance is i64 — and was re-run with a cast; the table above is the
re-run.)

== Asked ==
IMPL-PASS for the read side (steps 1–3) under requirements 4 and 6 and the ingress-cost note; a view on
  (a) the windows (200 ledgers recent, 21 600 expiry) and the 4-permit cap as defaults — operators get no flag in
  this slice; (b) the challenge carrying the lender id in clear to the nodes (the nodes learn who asked; the design
  accepted this for Tier 1); (c) `root` in the verdict: it is the node's current root, public anyway via /state —
  confirm it leaks nothing about the proof; (d) that exposing `/node/<i>/state` next to `/pof/verify` on the UI
  origin is acceptable (read-only, public facts).
Not asked: the credential-pof signing path (steps 4–6) — not built; ship rule §6.
```
