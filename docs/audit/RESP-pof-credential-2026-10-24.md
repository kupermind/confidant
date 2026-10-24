# RESP — proof of shielded funds (hackathon feature C): Tier 1, the signing path (`credential-pof`)

From the audit session, 2026-10-24 (event day), verbatim. Four rulings in sequence on PR #69's branch: the
conditional ruling on the owner's mechanics at `02691ce` (mechanics PASS, gated on a bound for the dev session's
finding — reserve and fee drain through an attacker-chosen Subject); the seven checks the bound had to meet; an
IMPL-PASS at `58f6f5d` (the dev session's PR #74, **artifact-bound**: that tree was superseded by the owner's own
bound `793614a` and never merged); and the **IMPL-PASS that ships**, on the reconciled head — the owner's bound
plus the dev session's deltas (PR #75), ruled at `c5e1760` and carried to `2916b0f` (node/sidecar/coordinator
code byte-identical). Recorded before real value: (D) the borrower's own XRPL signature over the challenge in the
request (subject consent), and the owner-reserve belt on `poolExcess` / the unshield `available`.

## 1. Conditional ruling on `02691ce` (the #69 part of a two-verdict message; the #67 part is in `RESP-pof-endpoint-2026-10-24.md`)

```text
════════ #69 — SIGNING PATH: mechanics PASS, but the signing path is GATED on your finding ════════
**The no-blind-sign crown is intact, verified firsthand.** checkTxShape's credential-pof branch binds Subject/CredentialType(hex case-insensitive)/Expiration(exact)/URI to THIS node's `pofCredential`, pins type=CredentialCreate per kind, Account=pool, Fee≤ceiling, Sequence=self-read-next, LLS in-window, no memos; the coordinator's values never enter the comparison. pin-5 (re-verify inside /sign, no cache), pin-1 (sign only on own node's verdict → 409 else), pin-3 (node derives, sidecar compares against node fields) — all hold. close_time(L) read by the node itself, immutable-once-validated → deterministic across nodes (502 if unreadable). The signing MECHANICS earn their pass.

**But your finding is CONFIRMED in code on all three tiers, and it is exactly what the ship-rule exists for — so I withhold IMPL-PASS for the signing path until a bound lands.** `subject = self.borrower.clone()` straight from the challenge, no gate/no cap (pof.rs credential()); coordinator autofills `Subject: c.subject` with no requester binding (coordinator.mjs:117). The proof is read-only (nullifiers never consumed) and `borrower` is a self-signed field in the sighash, so ONE funded position re-signs over any borrower → unbounded pool-account CredentialCreates → 0.2 XRP owner reserve + fee each. And the crown does NOT defend it: every honest node derives the same `subject=borrower` from the attacker's own challenge, so all k sidecars sign. I also confirmed `poolExcess = balance − outstanding_backing` (node.rs:171) ignores the owner reserve, so the reserve-lock does exactly march unshield toward `tecUNFUNDED` — a self-griefable brick of the demo's own withdrawal path. Testnet $0 means this is NOT an emergency and NOT a reason to pull #67 or to not demo — but an unbounded unauthenticated trigger for pool-account spends/locks is precisely the shape "a new signing path ships only with its own IMPL-PASS" is meant to stop, and it would ride silently to mainnet if passed as-is.

**My ruling on the bound (ruling the mechanism; ship-vs-slide follows from the standing rule, not my waiver):**
- **FLOOR = your (B), the outstanding-credentials cap N, NODE-SIDE.** It bounds the fund harm (reserve N×0.2 + fee N/window) attacker-agnostically, gate-on or -off, and the sidecar inherits it (node refuses to derive `credential` above N → no verdict → sidecar 409 → nothing signed; coordinator's own node → 502). This is the minimum that makes the signing path non-trivially-griefable. It's proportionate to testnet exposure — I am NOT demanding the full production fix tonight.
- **PREFER (A)+(B)** as you proposed: (A) derive only when the borrower holds feature-A's compliance credential (when the gate is on) adds identity-narrowing + the product story cheaply — but (A) is NOT the floor (gate-off = nothing; even gate-on, a credentialed account can be named as Subject without consent), so it's an addition, (B) is the floor.
- **(D) subject-consent = the REQUIRED before-real-value binding, time-stable.** The borrower signing the /pof/credential request is the only thing that binds Subject to someone who controls that account → attack reduces to self-addresses → tecDUPLICATE bounds to 1 per controlled account → closed. Record it as MUST-land before the borrower field ever drives real-value pool spend (mainnet / real reserve). (D) is the real fix; (B) makes tonight safe. (C) min-X is the weakest — skip.
- **Belt (record; tonight-optional if N is small and the pool is funded with headroom above base+N×0.2+backing): make poolExcess / unshield `available` subtract the pool account's owner reserve** so an unshield is never signed the reserve-locked balance can't fund. This is the Q-FEE-3 lineage — the conservation bound must account for ALL encumbrances, not just note backing. With a small N + funded demo pool, B alone averts the unshield-DoS; the belt is the cleaner invariant.

So: add ≥(B) (ideally A+B) with its own tests + a mutation row (cap-removed → unbounded issue accepted) and I pass the signing path in-window. Without a bound tonight, it stays on the slide — that's the ship-rule, not my call to waive.

**Your smaller notes, ruled:** (a) Expiration drift — keep E a ledger index (a timestamp breaks the read-side challenge bytes); the drift only affects the on-ledger credential's auto-expiry/reserve-release TIMING, not the lender's decision (the lender uses package-E, not credential.Expiration), so it's cosmetic — label "as of ledger L"; note it marginally lengthens the reserve-lock the griefer already maxes at E−L=21600, which B bounds by count. (b) 8-byte URI digest with X/L/E in clear — confirmed; fine for a "matches the package" display, but flag it's a short (64-bit) digest, not a forge-resistant commitment (≈2^32 to collide) — acceptable for an underwriting display, not for anything an adversary gains from colliding. (c) sidecar trusting its node over localhost = same model as /endorse,/fees — consistent, fine. (d) reserve + no auto-cleanup IS the finding's consequence; B caps magnitude, D closes it, the manual expired-delete is recovery not prevention. (e) re-verify is the only unspent-at-signing guarantee, no cache — confirmed, that IS the intended shape (pin-5).
Plus: the coordinator's /pof/credential has no body cap of its own — add one (the ingress-cost lesson applies to the coordinator, not just the nodes; it parses before forwarding). And your URI personalization "ConfidantPoFURI_" being outside the #31 core/src domain allow-list is a real completeness gap — the inventory scanning only core/src certifies the subset as the whole; extend the domain inventory (or SPEC §11) to enumerate backend personalizations too. Not trust-path (it's a correctly-domain-separated display digest), but the inventory is now incomplete — good catch, worth closing.

Send the #69 13-row re-run when it lands; my ruling stands on the firsthand read, the mutations are confirmatory. And tell me which bound you land so I can gate on the exact one.
```

## 2. The seven checks for the bound

```text
The plan matches my ruling exactly — (B) floor + (A) addition node-side, (D) and the poolExcess/owner-reserve belt as recorded follow-ups, plus the coordinator body-cap and the SPEC §11 inventory line. 13/13 + the coverage are confirmatory as expected. Approved in shape; I'll gate on the pushed head. So there's no ambiguity, here is exactly what I'll verify firsthand when it lands — build to these:

1. ⭐**CRITICAL — the cap/gate reads must FAIL CLOSED.** If the `account_objects` read (B) or the compliance `ledger_entry` read (A) errors or times out, the node must REFUSE to derive `credential` (null + reason), never derive-anyway. The whole bound evaporates if a griefer can induce an RPC error to bypass the count — the cap must fail toward refusal, same posture as feature A's gate. This is the one that would silently void the bound, so it's the first thing I'll check.

2. **The count's three filters:** (i) type=credential AND CredentialType == hex(ConfidantPoF) (so the PoF cap never counts the *compliance* credential type), (ii) issued-by-pool (an unaccepted credential sits in the pool/issuer's owner directory and the pool pays its reserve — that's the set that matches the reserve harm), (iii) expired-but-undeleted included — you have (iii), confirming (i)/(ii) at the ref.

3. **Page-marker-refuse is fail-closed only while N ≤ one account_objects page.** "A marker beyond the first page → refuse" is correct *because* default N is small (N < page size → any marker means > N → refuse). Add a named time-stable condition in the code/SPEC: **raising N near or above the account_objects page size requires true pagination**, else a large N under-counts (first page + marker = "at least a page," which no longer reaches N). Keep N small and this never bites; document the condition so a future bump doesn't silently reopen it.

4. **Null-credential inheritance both ends:** the sidecar must refuse on `credential: null` specifically (your MS6 guard is `!c?.subject` → c=null → 409, which does catch it — I'll confirm), and the coordinator surfaces 409-with-reason not its old 502. Confirm the read-side lender verdict (ok/amount/.../meaning) is genuinely unchanged — a valid proof still returns ok=true with credential:null+reason, since the cap bounds *on-ledger issuance*, not the underwriting verdict.

5. **(A) keys on the BORROWER, not the requester:** the compliance check reads `account = challenge.borrower` through feature-A's same fail-closed `check()`. (A) narrows subjects to credentialed accounts; it does not add consent — that's (D)'s job — so (B) remains the real bound on the fund harm. Confirming it reuses the verified lattice unchanged.

6. **Mutation rows pin both directions:** cap-removed → unbounded issuance accepted (killed), gate-removed → killed; and the permit direction — below N derives, at N refuses; gate-on-without-cred refuses, gate-on-with-cred / gate-off derives. A cap that only ever refuses (or only ever permits) is a survived mutation.

7. One race note, not a blocker: k sidecars + the coordinator's node each count independently, so concurrent requests can overshoot N by the in-flight count (bounded by POF_PERMITS per node). The cap is a griefing bound, not an exact invariant — fine, just don't document N as a hard ceiling.

On the SPEC §11 split — keeping the #31 allow-list scoped to core/src while the SPEC line enumerates the backend domain is the right division (the code guard stays where it's enforceable; the inventory is the human-complete list). Just make sure nothing reads the core/src allow-list as "all domains" — the SPEC line is what closes that.

Send the head + the new mutation rows when pushed and I'll do the firsthand pass against these seven. (D) and the belt stay recorded for before-real-value — not tonight.
```

## 3. IMPL-PASS at `58f6f5d` (PR #74 — artifact-bound; that tree was superseded and never merged)

```text
IMPL-PASS — the credential-pof signing path (#69 mechanics + #74 bound) earns it, verified firsthand at 58f6f5d against all seven, not from the map. It ships under the rule. Each check as I read it:

1. ⭐**FAIL CLOSED — confirmed.** `pof_issuance_refusal` (node.rs:282): both the compliance `credential()` read (289) and the `account_objects` read (308) `match` the Result and `return Some(reason)` on `Err` — withhold, never derive-anyway; the refusal runs AFTER core-verify (362→367) and `credential = refused.is_none().then(derive)`. This is the one that would have voided the bound, and it fails toward refusal. B5/B6 (read-failure→zero/absent) pin it.
2. **Filters — confirmed.** `outstanding_pof_credentials` (pof.rs:41): LedgerEntryType=="Credential" ∧ Issuer==pool ∧ CredentialType eq_ignore_ascii_case hex("ConfidantPoF")=436F6E…506F46 ∧ `(Flags & LSF_ACCEPTED)==0` (Rust precedence binds `&` tighter than `==`, so the mask is right; missing Flags→unwrap_or(0)→counted=fail-closed). The Issuer==pool filter also correctly excludes any credential the pool holds as *subject*. ConfidantKYC / another issuer's PoF ignored.
3. **Pagination — confirmed.** `account_objects` filters `type:"credential"` SERVER-SIDE at `validated`, limit 400, returns the marker; a marker → `max(cap,count)` → refuse; default N=25, LIMIT=200 < 400 and main.rs refuses to start above it. So a marker always means >400 ≥ cap → refuse. (One doc nuance: the page counts all Credential-type objects in the pool's directory, so "stay well below page size" is about total pool-issued credentials of every type — immaterial at N=25.)
4. **Null inheritance both ends — confirmed.** signer.mjs:50 `if (!verdict.credential) throw 409` BEFORE checkTxShape (belt over the existing `!c?.subject`); coordinator.mjs:116 now 409 with the reason + `details.credentialRefused` (was 502); PofVerdict.credential Option (null on wire) + credentialRefused, doc-commented fail-closed. Read verdict unaffected: ok=true/root/meaning stand when credential is null.
5. **(A) keys on borrower — confirmed.** `ledger.credential(borrower,issuer,type)` then `credential.check(borrower,…,ripple_now())`, only when `policy()` is Some — feature-A's reader+judge unchanged. (A) narrows subjects; it does not add consent — that stays (D)'s job — so (B) remains the fund-harm bound.
6. **Both directions — confirmed by the code + your table.** B1 cap-removed→unbounded, B2 gate-removed, B3 accepted-counted→too-early, B4 page-ignored, B5/B6 read-failure→fail-open all killed; 24-derives/25-refuses + cap-0 pin the boundary. B7 (cap==0 early-return removed) is a correct equivalent mutant — without it, `outstanding>=0` refuses anyway, the early return only gives the clearer message.
7. **Race — documented as a griefing bound, not a hard ceiling.** Agreed, non-blocking.
Coordinator body cap confirmed: `serve()` counts bytes while reading and throws 413 before `JSON.parse` for every route. Default N=25 ⇒ ≤5 XRP max reserve — proportionate for the event.

**One correction — don't commit this part of your summary as-is.** You reported "SPEC §11 inventory now lists ConfidantBundle, ConfidantPoF_v1_, ConfidantSigHash and names ConfidantPoFURI_." It did NOT land in this PR: no SPEC file changed (02691ce..58f6f5d touches only API-BOUNDARY, API-BOUNDARY.ru, NODE-OPERATOR, PROOF-OF-FUNDS, the REQ), and `ConfidantPoFURI_` appears only as the URI-format line in PROOF-OF-FUNDS.md:86 — not in any domain-separation inventory. The "SPEC §11" text you may have seen is #31's own doc note quoted inside REQ-apply-path-guards. So the completeness point I raised — the domain inventory enumerating the backend `ConfidantPoFURI_` next to the core-scan set — is STILL OPEN. It's non-trust-path (the digest is correctly domain-separated; the code is fine), so it does NOT hold up the IMPL-PASS — but add it for real (or point me to the SPEC file if I'm mis-locating it; the doc-stat says none changed). This is exactly why I read the artifact rather than the summary — CHANGED≠CORRECT, and it bit a favourable claim.

Minor, optional, pre-existing from #69 (not a gate): `ledger_close_time(L)` at node.rs:356 uses `?`, so a transient close_time read failure 502s the whole verdict — including the lender's read side — whereas the two issuance reads degrade to credential:null. close_time(L) of a validated ledger is immutable so the practical risk is ~nil; for full decoupling of the read verdict from issuance machinery you could degrade it to credential:null+reason like the others. Whenever.

So: merge #74 into pof-credential and approve #69 on this — the signing path is IMPL-PASS'd. Recorded before-real-value: (D) subject-consent, the poolExcess owner-reserve belt. Open doc follow-up: the domain inventory (actually land it). Commit both rulings verbatim; for #74's RESP, fix the SPEC-§11 line to "open follow-up," not "done."
```

The "did-not-land" correction above was struck by the audit itself after re-measuring (its diff had been
scoped to `docs/`; `SPEC.md` lives at the repository root):

```text
You're right and I was wrong — reverse my correction, record the inventory as LANDED. I re-measured unfiltered: `git diff --stat 02691ce..58f6f5d` shows SPEC.md +12/−3 and SPEC.ru.md +6/−1, and `git show 58f6f5d:SPEC.md` has at line 535 exactly the wording I asked for — "That scan covers core/src only; the complete inventory of hash domains in this repository is the three above plus the backend's `ConfidantPoFURI_` (backend/node/src/pof.rs … a display binding, not a signature domain)" — with ConfidantPoF_v1_/pof_ctx added to the core-scan bullet at 531-532. The RU mirror too. It landed, in the right shape; the completeness point is CLOSED, not open.

The cause is mine and it's worth naming because it's the exact lesson I invoked against the claim: my two "measurements" were both scoped to `docs/` — `git diff --stat … -- 'docs/*'` and `git show …:docs/SPEC.md` — and SPEC.md lives at the repo ROOT, so my own grep scanned a subset and I certified "no SPEC changed" as the whole repo. The ref I diffed (58f6f5d) was correct; the SCOPE was wrong. That's "a completeness guard that scans a subset certifies the subset as the whole," committed by me while checking for it — a path filter silently excluding the file that mattered. Thank you for pushing back with the hunk; that's the right move and it's what caught it. No stale checkout, no force-push question — just my scoped search.

So the RESP records: domain inventory LANDED (SPEC.md/SPEC.ru.md), my mid-review "did-not-land" struck as a scoped-search error on my side. Everything else stands: the signing path is IMPL-PASS'd, all seven verified firsthand, (D) + the owner-reserve belt recorded before-real-value. And agreed on close_time → leave it a recorded follow-up since it moves #69's pinned "L unreadable → 502" row; don't touch the owner's slice tonight.
```

## 4. The branch moved: the pass is artifact-bound, a re-pass is owed on one reconciled head

```text
Hold confirmed — #69 approval is on ice until you send the reconciled head. And you've framed it exactly right: my IMPL-PASS binds to the tree I actually read (58f6f5d), not to the branch. A branch head that moved to 02691ce + the owner's 793614a is a DIFFERENT artifact and carries none of my pass — same discipline as "deployed ≠ this tree": the approved commit must BE the one that merges, or the approval is for something that isn't there. So nothing rides on #69 until I've done a firsthand pass on whatever single head you reconcile to.

Terms for that pass, so you can aim the reconciliation:
- Whichever implementation wins — the owner's 793614a, yours rebased over it, or a merge — I run the SAME seven against the actual head, firsthand, fail-closed reads FIRST. The owner's code gets audited exactly like yours; authorship buys nothing here (audit with hunger, all errors are errors regardless of origin). I'm not assuming mine is better and I'm not rubber-stamping his because he wrote it — I read the head on its merits.
- His commit says "gate eligibility, one credential" — that may be a different shape than your (B) cap (an eligibility gate + a per-subject "one credential" rule reads closer to what tecDUPLICATE already enforces per (subject,type) than to a reserve-exposure cap). The acceptance bar is unchanged: whatever lands must (i) bound the FUND harm — total reserve + fee the pool can be made to spend — not just dedupe per subject; (ii) fail closed on every ledger read (compliance + the count/eligibility read); (iii) inherit to the sidecar (node withholds → 409 → nothing signed); (iv) key on the challenge's borrower; (v) null-inheritance both ends; (vi) mutations both directions. A per-subject "one credential" alone does NOT bound the fund harm — an attacker names N distinct subjects — so if his bound is only that, it needs the cap too.
- ⭐If you MERGE both bounds rather than pick one, I audit the SEAM specifically: two bounds interacting can leave a gap neither has alone (e.g. one path derives the credential under his eligibility rule while bypassing your cap, or the two disagree on what "unaccepted" counts). Give me one coherent refusal path, not two stacked guards with a crack between them.

Don't commit the RESP as "approved/merged" — record it as IMPL-PASS against 58f6f5d (valid for that artifact), branch reconciliation pending my re-pass. Send the reconciled head when it's one implementation and I'll turn it around firsthand. Still also holding for A's live rerun.
```

## 5. The IMPL-PASS that ships — the owner's bound `793614a` + the dev session's deltas (PR #75), ruled at `c5e1760`, carried to `2916b0f`

```text
════════ #75 — PoF signing-path bound: IMPL-PASS, it SHIPS ════════
Ruled against c5e1760 and carried to the moved head 2916b0f. All seven hold on the owner's single implementation (A gate → B borrower-dedup → C fund-cap), verified at the ref, not the map:
1. **Fail-closed — confirmed.** `credential_bounds` (node.rs:333): all three ledger reads use `?` — compliance credential(:343), borrower list(:351), pool count(:364) — plus close_time(:296); an Err propagates to NodeError → 502, never a derivation. Withholds are `Ok(Some(reason))` → credential:null. D3/D4/D5 (per-account read failures) pinned — and the per-account fake-ledger switch was the right call; a shared switch would have let each read's mutation survive behind another's error.
2. **Filters.** `outstanding_pof_credentials` → `is_pof_credential_from` (Issuer==pool ∧ CredentialType eq_ignore_ascii_case hex(ConfidantPoF)) ∧ `(Flags & LSF_ACCEPTED)==0` (missing Flags→unwrap_or(0)→counted = fail-closed). (C) reads the POOL's directory, which is where unaccepted pool-issued credentials sit, so it's the correct fund backstop regardless of (B)'s completeness.
3. **Pagination.** both (B) and (C) `if more → withhold`; `const _: () = assert!(POF_CREDENTIAL_CAP <= 200)` is a COMPILE-TIME bound to the 400 page — stronger than a runtime start-guard. account_credentials type-filtered server-side at validated, limit 400, returns the marker.
4. **Null both ends + crown intact.** PofVerdict.credential Option + credential_refused; signer.mjs:50 refuses null before checkTxShape; coordinator:116 409 with reason. checkTxShape's credential-pof branch unchanged — binds Subject/CredentialType(ci)/Expiration(exact)/URI(ci) to THIS node's pofCredential, no memos.
5. **(A) keys on borrower** via feature-A's credential()+check(), judged against **close_time(validated)** not the system clock — your Q-CG-1 shape baked into this path too.
6. **Both directions** — MB1–MB8 + D1–D5 + the opt-in test (node.rs tests "without the flag no Credential is derived and no account is read").
7. **Race** documented as a griefing bound, not a ceiling.
⭐**Reachability is airtight:** `challenge.credential(amount,t)` has exactly ONE call site (node.rs:319) inside `(Some(t), None) =>`, reachable only when want_credential ∧ core-verify's `?` (:307) passed ∧ credential_bounds→None. No bypass of the bound. The opt-in `want_credential` also resolves the close_time coupling I flagged on 58f6f5d — the plain lender verdict does no issuance reads, so it can't be 502'd by them. Cap=8 (≤1.6 XRP reserve) is tighter than my earlier pass's 25; fine. SPEC inventory present on the shipping head (SPEC.md:532 names ConfidantPoFURI_).
Head-move 2916b0f: `git diff --stat c5e1760 2916b0f -- backend core` is EMPTY — the node/sidecar/coordinator code is byte-identical, so the pass carries; the rest is the merged #67/#68/#70 docs/UI (already-passed slices) + the AP4M deletion. **Merge #75, approve #69, cite 2916b0f as the shipping artifact; record my 58f6f5d pass as artifact-bound and this as the one that ships.** One non-security note to route to the owner, not me: the AP4M files are gone from the TREE (good — and they were under hackathon/, never in backend/core, so never in the node's trust path), but they remain in git HISTORY at 793614a; if the hackathon-only license forbids history retention too, that's an owner/licensing call, not a security ruling.
```

## 6. Dev session's record

Finding (reserve and fee drain through an attacker-chosen Subject) and the two bounds are in the REQ and its
supplements. Mutation tables run independently: the owner's MC1–MC6 / MS1–MS7 on `02691ce` (13/13 killed);
B1–B7 on `58f6f5d` (6 killed, B7 equivalent); the owner's MB1–MB8 on `793614a` (his table); D1–D5 on `c5e1760`
(5/5 killed). Follow-ups recorded in `docs/PROOF-OF-FUNDS.md`: (D) subject consent before real value; the
owner-reserve belt on `poolExcess`; degrading a failed `close_time(L)` read to `credential: null` instead of 502.
Licensing note for the owner: the AP4M kit files remain in git history at `793614a`.
