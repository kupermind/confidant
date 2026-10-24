# REQ — credential gate (XLS-70, hackathon feature A): node side

Sent 2026-10-24 (event day) from the implementation session to the audit session, verbatim (branch
`credential-gate-node`). Built on-site; the design is the contract in `docs/API-BOUNDARY.md` §2
("Credential gate"), written the same day.

```text
REQ (impl, read gate, no new signing path): a node started with --credential-issuer <r> --credential-type <hex>
[--credential-type-text <label>] accepts a shield only if the deposit's sender holds an XLS-70 Credential on
the ledger from that issuer, of that type, accepted by the subject (lsfAccepted) and not expired.

== Design ==
backend/node/src/credential.rs: CredentialGate { policy: Option<CredentialPolicy{issuer (canonical), type (hex,
1–64 bytes, upper-cased), typeText (given or decoded when printable)} }. check(account, entry, now):
  none on the ledger → refuse; entry not for (Subject=account, Issuer, CredentialType) → refuse ("the ledger
  answered for another credential"); lsfAccepted clear → refuse; Expiration ≤ now → refuse; else pass.
  Gate off → pass. Every refusal: "no accepted credential from <issuer> of type <typeText> for <account>: <why>".
Ledger trait gains credential(subject, issuer, type) → ledger_entry { credential } on the validated ledger;
  Some(node) or None on entryNotFound; any other RPC failure is an error → the node fails closed (502), never
  accepts. The lookup is keyed by the triple the policy demands; the answer is checked against it again.
node.rs: the gate runs inside the deposit path (screened_deposit → vouched_deposit) after the screening
  gate, so submit_shield and endorse(Shield) both run it — the sidecar rule is unchanged (it signs nothing its
  node did not accept). The sender account comes from the ledger's tx answer (deposit_source), never the client.
  now = seconds since the Ripple epoch from the system clock (Expiration semantics of XLS-70).
http.rs: NodeError::Credential → 403; GET /policy { credential | null, screening { active, standbyLedgers } };
  the coordinator copies credential into GET /config for the UI (coordinator.mjs).
Deploy: render-env.mjs CREDENTIAL_ISSUER / CREDENTIAL_TYPE / CREDENTIAL_TYPE_TEXT → CREDENTIAL_ARGS; the node
  unit passes $CREDENTIAL_ARGS. Same policy on every node of a quorum.
Honest label (SPEC, NODE-OPERATOR): gatekeeping on a trusted issuer's say-so at the public boundary; the
  issuer vouches for whoever it chooses; nothing about anyone's notes. Not "KYC'd pool".

== Tests ==
backend/node/tests/node.rs credential_gate_refuses_deposits_without_an_accepted_credential_at_submit_and_endorse:
  policy parsing (bad issuer, bad hex, text derivation, label override); check() on none / not accepted /
  expired (≤ now) / wrong issuer, subject, type / case-insensitive type hex / gate off / message wording;
  node: gated vs open at submit (none, not accepted, expired) and at endorse (refused while expired, accepted
  once good), the open node never asks the ledger, a failing lookup is a ledger error (fail closed);
  HTTP: /policy reports the gate, a refused shield is 403 naming the credential, open node reports null.
Coverage: cargo llvm-cov -p confidant-node … --fail-under-lines 95 → 95.7 % lines (credential.rs 98 %,
  http.rs 100 %); all 11 node tests green; clippy -D warnings clean.

== Mutation table ==
Run alone against the credential test, files restored after each row. 5/5 killed.
M1 accepted-flag check removed → killed
M2 expiry check removed → killed
M3 gate not consulted in the deposit path (policy filtered to None) → killed
M4 issuer/subject/type match against the answer removed → killed
M5 refusal mapped to 409 instead of 403 → killed

== Asked ==
IMPL-PASS for the gate as a read-only deposit check; a view on reading Expiration against the system clock
rather than the ledger's close time; confirm the sidecar needs nothing new.
```
