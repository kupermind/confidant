# REQ — the demo issuer as a service: `backend/quorum/issuer.mjs` (writes the pool's entry credential to any account that asks)

Sent 2026-10-25 (night before the demo) from the implementation session to the audit session, on the project lead's
instruction, after the lead observed that a judge cannot try the pool from their own browser: shielding needs the
entry credential, and until now the only way to get one was the team's issuer key pasted into that browser.
**Deployed from the branch for the demo; the merge to `main` waits for this ruling** (the repo rule: every `backend/`
change gets a slice ruling before merge).

## What is proposed

A small HTTP service, run on the demo host as `confidant-issuer` (user `confidant`, behind the site's TLS at
`/issuer/`), that holds the **demo issuer's key only** (`ISSUER_KEY` = the file the check tooling already uses) and
answers:

- `GET /health` → `{ ok, issuer, typeText, days, perHour, issuedThisHour, policy }`
- `POST /issue { subject }` → `{ status: "issued" | "accepted", issuer, credentialType, typeText, expiration, tx | null }`

It signs `CredentialCreate` (type from `/config.credential`, 30-day expiry) from the issuer account to `subject`, and
for an expired credential of its own a `CredentialDelete` first. The holder's `CredentialAccept` stays with the
holder: the app signs it in the browser after the service answers. Nothing else changes: the nodes' gate
(`credential.rs`, ledger read at submit and at endorse) is untouched; no pool key is involved; the coordinator is not
in the path.

The app (`frontend/index.html`) prefers the service: when the account lacks the credential it calls `POST /issuer/issue`,
then accepts. The team's key saved in the browser (the under-the-hood page's fold) becomes the fallback when the
service is down; with neither, the one step to do is said before any press. The under-the-hood page says what the
issuer is: a demo service whose policy is "whoever asks"; the gate on the ledger is real.

## Why this is honest and what it changes

- The gate as a **mechanism** is unchanged and real: every verifier node reads the credential from the ledger and
  refuses a deposit without it. What becomes a stub is the issuer's **policy** — on this testnet it was already the
  team writing credentials for whoever asked at the event; the service only removes the hand. Every surface says so
  (the under-the-hood page, README, DEMO, SUBMISSION, API-BOUNDARY).
- Bound A of the proof-of-funds credential ("eligible under the gate") therefore means "has asked the demo issuer".
  Bounds B (one PoF credential per borrower) and C (the pool's outstanding cap, 8) are unchanged and still the ones
  that bound the pool account's reserve and fees.
- The issuer account pays the reserve for each unaccepted credential and the fee for each write; the service caps
  writes per hour and per subject, and is idempotent (a live credential is answered, never rewritten).

## Guards and their tests (`backend/quorum/issuer-lib.mjs`, `backend/quorum/test/issuer.test.mjs`)

| Guard | Test | Removed → |
|---|---|---|
| classic XRPL address only (X-addresses, junk refused) | `isClassicAddress` | red |
| another issuer's or another type's credential does not count | `judgeExisting` create cases | red |
| type compared case-insensitively | `judgeExisting` live cases | red |
| an expired credential is replaced, a live one is answered | `judgeExisting` expiry cases | red |
| accepted vs issued reported from the flag | `judgeExisting` live cases | red |
| global hourly cap | `RateLimiter` | red |
| per-subject window | `RateLimiter` | red |
| the hour window slides | `RateLimiter` | red |
| body ≤ 4 KiB | `parseIssueRequest` | red |
| `subject` validated | `parseIssueRequest` | red |

Mutation table (each guard removed in turn, `node --test test/issuer.test.mjs`): M1–M10 **all killed** (10/10),
2026-10-25. Not unit-tested, by construction: the serialized signing (`queue`), the `actNotFound → 404` mapping and
the start-up refusal when `ISSUER_KEY` is not the issuer the nodes trust (`/config.credential.issuer`) — exercised by
the deploy (`GET /issuer/health`) and by the first live issue.

## Questions for the ruling

1. Is "the issuer's policy is a stub, the gate is real" an acceptable label for the demo, given that a judge can now
   enter the pool with one press? (The alternative — the team's key in every browser — is strictly worse.)
2. Is the rate limit (60/h overall, one per account per 10 min, 30-day expiry) the right shape, or should the service
   also require the subject to be a funded account older than N ledgers?
3. Anything in the key handling you want changed before this merges (the key file is read once at start; the service
   runs as the same user as the nodes; `NoNewPrivileges`; env file mode 600)?
