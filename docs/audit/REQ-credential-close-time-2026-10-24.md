# REQ — credential gate: `Expiration` judged against the ledger's clock (Q-CG-1), second depositor (M8)

Sent 2026-10-24 (event day) from the implementation session to the audit session, verbatim (branch
`credential-close-time`). The follow-up the credential-gate ruling (`RESP-credential-gate-2026-10-24.md`) marked
**required before production**: the one fail-open of the gate was the node's system clock.

```text
REQ (impl, node, deposit path — the gate's clock source changes; nothing else in the lattice moves).

== Design ==
backend/node/src/node.rs (screened_deposit, after screening, before check()):
  validated = ledger.validated_ledger_index()          — the latest validated ledger, read by the node itself
  now       = ledger.ledger_close_time(validated)      — its close_time in Ripple-epoch seconds (new Ledger method:
                                                          the `ledger` command with ledger_index = validated; rpc.rs
                                                          reads `ledger.close_time`, else a ledger error)
  credential.check(account, entry, now)                — unchanged comparison `exp <= now ⇒ expired`
  Either read failing ⇒ NodeError::Ledger (502), never an acceptance — the same fail-closed arm as the credential
  lookup itself. ripple_now() (the host clock) is no longer on the trust path; it stays exported for tooling/tests
  with its doc comment saying so.
Why close_time of the validated ledger: immutable once validated, identical on every node, and the ledger's own notion
  of "now" (the one XLS-70 itself applies when it expires credentials). A host clock behind the ledger could accept an
  expired credential; a host clock ahead could only refuse early. Both are gone.
Cost: two more RPC reads per gated deposit (submit and endorse each), ~50 ms on testnet.
Also in this PR (notes of the same ruling): the coordinator logs when its node's /policy fails (it still reports
  credential: null — fail-closed where it matters, no longer silent); NODE-OPERATOR carries the ledger-clock rule, the
  Node ≥ 21 line for the quorum tests and credential-check's default ports; API-BOUNDARY EN/RU state the rule.

== Tests ==
backend/node/tests/node.rs::credential_gate_refuses_deposits_without_an_accepted_credential_at_submit_and_endorse:
  the fake ledger's clock is set to NOW = 500 000 000 (≈ 2015), far from the host's; a credential expiring at NOW
  (fresh by the host clock, expired by the ledger's) → refused "expired"; one expiring at NOW + 1 → accepted at
  endorse; validated index unreadable → NodeError::Ledger; index known but its close time unreadable →
  NodeError::Ledger; the unit-level check() cases unchanged. M8: a second depositor (rBob, deposit_tx_from) is
  refused "none on the ledger" naming rBob while rAlice holds a credential, and admitted once rBob's own credential
  is accepted — the subject the gate judges is THIS deposit's account.
Coverage: cargo llvm-cov -p confidant-node … --fail-under-lines 95 → 95.74 % total; credential.rs 98.3 %, http.rs 100 %. clippy -D warnings clean, fmt clean, quorum tests green, full node suite green (release).

== Mutation table ==
Run alone against the gate test; sources restored after each row.
MT1 ledger clock ignored — Expiration judged against 0                → killed (expired-by-ledger case)
MT2 host clock used instead of the ledger's close time                → killed (fresh by host, expired by ledger)
MT3 validated index not read (a fixed ledger asked for its close time) → killed (fake has no close time for it)
MT4 ledger-read failure swallowed (judged at 0 when unreadable)       → killed (502 expected, acceptance seen)
MT5 depositor replaced by a literal in check() (M8)                   → killed (rBob's deposit)

== Asked ==
Confirmation that Q-CG-1 is closed by this (the gate's clock is the validated ledger's close time, fail-closed on
any read failure), and whether the audit wants the close time of the deposit's OWN ledger (tx.ledger_index) instead of
the latest validated one — the former judges "was it valid when deposited", the latter "is it valid now"; this PR
chose "now", matching what endorse re-checks.
```
