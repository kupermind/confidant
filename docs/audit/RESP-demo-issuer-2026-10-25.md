# RESP — the demo issuer as a service (`backend/quorum/issuer.mjs`): ruling

From the audit session, 2026-10-25, verbatim, in answer to `REQ-demo-issuer-2026-10-25.md` (the owner's REQ on PR #100,
head `f85a983`) relayed by the dev session together with its own review findings F1–F3 (posted on PR #100). The ruling
was made firsthand on the branch head. Verdict: **approve as a demo convenience, conditional on F1 + F2 fixed before
merge and the label sharpened**; the audit verifies the merged head firsthand and the redeploy is from it.

**The Q1 label in §1 is superseded by §3** (two corrections the same day, the second carrying the owner's hard rule:
never "not KYC" — this IS KYC, running in demo mode with a placeholder decision policy). F1–F3 and the rest stand unchanged.

```text
Ruled firsthand at f85a983 (issuer.mjs read in full; findings confirmed in code, not from your summary). Verdict: **approve the demo issuer as a demo convenience, conditional on F1 + F2 fixed before merge and the label sharpened.** It is NOT a P1/P2 violation — I'll say why, because the distinction from the PoF disaster matters.

**Not the PoF trap.** P2 forbids a feature that regresses a guarantee the system already had. The entry gate's honest label was always "issuer-vouching at the public boundary, NOT a KYC'd pool — mechanism, not policy." It never promised real admission control. Automating issuance (anyone-asks-gets) does not regress a promised property; it makes the stub explicit. No core property is touched: graph-hiding is untouched (the issuer never sees a note), integrity=k-of-N is untouched (the issuer is not a signer), and it holds no pool key. Contrast PoF, which regressed graph-hiding — an unconditional, promised property — and leaned on a human. So this is acceptable with an honest label; PoF was not acceptable at any label. (One consequence to note, not an objection: making ConfidantKYC open makes PoF's bound-(A) a no-op — but (A) was never the fund bound, (C) the cap is, as I ruled; and PoF is flagged off / headed to Tier-4 anyway.)

Verified the key blast radius firsthand: only `ISSUER_KEY` loaded (33–35), all signing is `issuer.sign`/`Account: issuer.address`, line 38 refuses to start unless the key == `/config.credential.issuer`. Never a pool/signer key. ✓

**Q1 — label:** acceptable, and yes make your addition, but sharper and mandatory on every surface: not just "the policy is a stub" (that reads as "a simple policy exists") but **"the pool's entry is OPEN in effect — the demo issuer writes the entry credential to anyone who asks; this is NOT KYC/admission control, it demonstrates the credential MECHANISM only."** Time-stable condition, write it: this is a demo-only path and must NEVER be the real-value/mainnet entry — a real pool needs a third party with a real policy, or the gate is theater. With that, honest. And yes, strictly better than the team key in every browser (worse key exposure).

**Q2 — rate-limit shape:** fine for a demo; do NOT add a funded-account-age requirement (it only slows a judge and buys no security). What matters is F1 — the cap must actually hold.

**Q3 — key handling:** a hot issuer key on a public endpoint is acceptable here because the blast radius is exactly what the service already grants anyone (ConfidantKYC issuance), the pool account is not involved, and the start-up address pin is the right guard — keep the env 600 / NoNewPrivileges / same-user. Time-stable: demo-only, throwaway issuer account; fund it enough that the (capped) reserve/fee spend won't hit tecINSUF_RESERVE mid-demo. Nothing to change in the key handling itself.

**F1 — BLOCKER before merge, and take the FULLER fix, not just check+note.** Move the read + judge + check + note ALL inside the serialized closure. "At least check+note inside" fixes the cap but leaves the duplicate and replace races: two concurrent "none" both read stale and hit tecDUPLICATE; two concurrent "replace" churn Delete/Create. Full serialization + `submitAndWait` (which you already use) means request-2's serialized `account_objects` read sees request-1's validated write → answers "accepted/issued", no duplicate. Plus map tecDUPLICATE on CredentialCreate (and a Delete that races to tecNO_ENTRY) to the idempotent answer `{status:"issued"/"accepted", tx:null}` as a belt for out-of-band creation. Tests: two concurrent `issue()` for one subject → exactly one write, the other answered; and the cap holds under an N-burst (N concurrent → ≤ perHour writes). Re-run the 13 mutations.

**F2 — fix (non-security, same lesson as #62):** match the error CODE `e.data?.error === "actNotFound"` (or both code and lowercased message), not the prose in `e.message`. Nothing is written either way so the gate is unaffected — it's the 404-vs-502 class and the operator doc. Good to see the #62 "match a structured code, not prose" lesson generalize; worth a one-liner in the code comment so it isn't reintroduced.

**F3 — minor, fix before the demo day ideally, non-blocking:** disconnect the stale `client` before reconnecting in `xrpl_()` (line 43) to stop the leak; and reconsider swallowing `uncaughtException` (line 83) — with `Restart=always`, letting a broken process exit and restart is cleaner than keeping a half-dead one.

**One addition of mine:** the internal limiter bounds ISSUANCE (writes) only — and the idempotent "none" path (61) returns before `limiter.check`, so read requests (each an `account_objects` RPC through the issuer's client) are not bounded at all. Front the endpoint with a per-IP REQUEST limit in nginx (you already say "add your own rate limit in front" — make it explicit that it's for request-flooding, since the internal cap is issuance-only), or a flood of reads can grief the issuer's RPC. Demo-acceptable with the nginx limit; note it in NODE-OPERATOR.

So: fix F1 (fuller) + F2 + sharpen the Q1 label + the nginx request-limit note; F3 is nice-to-have; re-run mutations + the two concurrency tests; then I verify the merged head firsthand and the redeploy is from it (artifact identity, same as #69/#72). Merge waits for the fixes + my check on the merged head. Everything else here is sound.
```

## 2. Correction to the Q1 label (same day, verbatim; superseded by §3)

```text
Correction to my #100 ruling — only the Q1 LABEL; everything else stands. I wrote "pool entry is OPEN in effect — NOT KYC — mechanism only." That is wrong and the owner corrected it: **this IS the KYC gate, running in demo mode.** Do not label it "not KYC" — that denies the real thing the system has.

Replace the Q1 label wording with this, on every surface:
- **(a) The KYC gate is real and production-grade** — entry is credential-gated, the credential is issuer-attested, and every verifier node checks it fail-closed before it signs. Affirm this; it is genuine KYC architecture.
- **(b) For the demo, the issuer's POLICY is open** — the demo issuer auto-issues the credential to anyone who asks, rate-limited — so a demo credential does **not** mean the holder was vetted. Production runs the **same gate** with a real issuer policy.

So the honest one-liner is "KYC that runs in demo mode: the gate is real, the demo's issuance policy is open." Not "not KYC," and not "we KYC'd these users" — both are false. Affirm the mechanism, be honest the demo issuance is permissive.

Time-stable, restated correctly: the **mechanism/gate graduates to production**; it is the demo's **open issuance policy** that must never be the production policy (production needs a real issuer applying a real policy). Earlier I phrased the time-stable condition as "never the production entry," which wrongly implied the gate itself is demo-only — it isn't; only the permissive issuance is.

F1 (blocker, fuller fix), F2 (match the code), F3 (minor), the nginx request-limit note, and the artifact-identity-on-merge all stand unchanged. Record this label in place of the earlier one.
```

## 3. Definitive Q1 label — the owner's hard rule (same day, verbatim; this is the label of record)

```text
Owner tightened Q1 and made it a hard rule — this supersedes both my earlier Q1 wordings. **Never write "not KYC" (or "entry is open", "theater", "mechanism only"). This IS KYC.**

His argument, which is the correct frame: the issuer's decision logic is the ONLY missing piece. Give the server ANY decision logic — even "deny everyone" — and it is self-evidently KYC. The architecture is a complete, working KYC system: credential-gated entry, the credential is issuer-attested, every node verifies it fail-closed before signing, the holder accepts it on-ledger. What was not built for the hackathon is a full KYC **decision pipeline**; the current issuer policy is a **placeholder that issues to any requester (rate-limited)**.

Definitive Q1 label, on every surface:
> "This is KYC. The credential gate, the issuer attestation, the fail-closed node check and the on-ledger accept are the real, working system. The issuer's admission **decision logic is a placeholder for the hackathon** — it currently issues to any account that asks, rate-limited; a real deployment drops its own decision logic (approve/deny by policy) into the same issuer with no change to the gate."

Honesty still holds both ways: do NOT say "not KYC", and do NOT claim the demo vetted anyone (the placeholder issues to all). Time-stable, corrected: the KYC system graduates to production as-is; it is the **placeholder decision policy** that gets replaced with real decision logic — not the gate.

F1 (blocker, fuller fix), F2 (match the code), F3, the nginx request-limit note, and artifact-identity-on-merge are all unchanged. Record this Q1 label in place of anything I sent earlier.
```

## 4. IMPL-PASS on the fixes (PR #101, head `8ce47ae`), verbatim

```text
IMPL-PASS on the #100 fixes, verified firsthand at 8ce47ae (binds to the merged head — artifact-identity check on merge below).

- **F1 fuller — correct.** `makeIssuer` (issuer-lib.mjs:81–101): read → judge → `limiter.check` → (delete if replace) → create → `note`, the WHOLE flow inside one `serialized` queue. Because `submit` uses `submitAndWait`, request-2's serialized `read` at `validated` sees request-1's already-validated credential → judges "none" → no duplicate; the cap `check` is serialized so it holds under a burst; the tecDUPLICATE belt counts the refused attempt (:94, so a dup burst still consumes the cap, no fee-burn amplification) and re-reads to answer idempotently; tecNO_ENTRY on the delete is tolerated. `submit` throws with `.code` = the tec (issuer.mjs:66), so the belts' `e.code` checks key correctly. Still only the issuer key signs (issuer.address). 
- **F2 — correct.** `rippledCode(e) = e.data.error` (code, not prose), 404 only on `actNotFound`, with the #62-lesson comment. 
- **F3 — correct.** `xrpl_()` disconnects a stale client before reconnecting (:52) and logs client errors; `uncaughtException` → `exit(1)` for Restart=always. 
Your 23/23 mutations + the concurrency tests (2-concurrent→1 write/1 attempt; 10-burst/perHour 3→3+7×429; expired→1 delete+1 create; out-of-band dup→answered) are confirmatory of what I read.

**Label + the lead's frame: carried, and it is honest — I checked every clause against the code.** The gate / issuer attestation / fail-closed node check / on-ledger accept ARE the real working system (verified). Admission IS allow-by-default (verified: any requester, rate-limited). And "a demo credential attests the request, not a vetting" is the honest caveat that keeps the label from overclaiming — exactly right, keep it verbatim. The lead's frame (gate real; admission allow-by-default = a normal business model, not a shortcut/defect; KYC's limit-tiering is out of the hackathon's scope) is sound and I carry it: open registration with KYC-tiered limits is a real model, so "allow-by-default, standard practice" is accurate, not apologetic. No "not KYC", no "stub/placeholder", no apology — agreed and recorded on my side.

On your noun question: **"policy" is the right noun** — allow-by-default IS a genuine admission policy (a chosen default), not the absence of one. That actually reinforces the lead's point, so keep "admission policy: allow-by-default." No change wanted.

**Artifact identity on merge:** the owner merges #101 + #100 and redeploys. On that I do one check — `git diff <merged head> 8ce47ae -- backend/quorum` is empty (the merge didn't alter issuer*.mjs) and the deployed issuer is from the merged head. Then #100 is fully closed. Everything here is sound.
```

## 5. Owner's final label (2026-10-25) — supersedes the "not a vetting" caveat in §3/§4

The project lead ruled that the demo issuer must not be framed as a shortcoming, placeholder or missing pipeline: open
registration with KYC to raise limits is a legitimate, ordinary model (the exchange model — an account is opened first,
KYC lifts limits). So the apologetic phrasing — "placeholder", "standard practice for a demo", "a full KYC decision
pipeline is outside the hackathon's scope", "not a defect/flaw", and "a demo credential attests the request, not a
vetting" — is dropped on every surface. The honest core is **kept, not softened**: the surfaces still say plainly that
the demo issuer registers any account that asks, rate-limited, and that the gate on the ledger is the real system — so
nothing claims a holder was vetted, and nothing calls the real gate "not KYC". Label of record on every surface:

> "KYC in demo mode: the credential gate every node enforces on the ledger is the real, working system; the issuer's
> admission policy in the demo is open registration — it registers any account that asks, rate-limited, the model an
> exchange uses where an account is opened first and KYC raises its limits; a production deployment runs its own issuer
> policy against the same gate."

This narrows §4's "keep 'attests the request, not a vetting' verbatim": the overclaim it guarded against (implying a
vetting happened) is instead prevented by stating the open-registration fact directly, which the CI over-claim check and
the honest-label rule both accept. §1–§4 above are left as the verbatim record of the audit's position at the time.

## 6. Re-confirm after the owner's label commit `7a4f7ad` (same day, verbatim): IMPL-PASS carries; the label is honest

```text
Both re-confirms done.

**1. Artifact identity — re-confirmed, IMPL-PASS carries to 7a4f7ad.** I diffed `8ce47ae → 7a4f7ad -- backend/quorum` firsthand: the only changes are issuer.mjs's header comment (lines 6–12) and the `POLICY` const string. `issuer-lib.mjs` is byte-identical (so `makeIssuer`'s one-queue flow, the tecDUPLICATE/tecNO_ENTRY belts, `rippledCode`, `judgeExisting`, `RateLimiter` are all intact), and `xrpl_`/`submit`/`readCredentials` in issuer.mjs are untouched. Non-functional diff ⇒ my F1/F2/F3 pass holds. **New anchor for the merged-head check is 7a4f7ad.** Point your SUBMISSION-length follow-up away from backend/quorum (it shouldn't need to touch it) so the anchor stays 7a4f7ad; if it does touch backend/quorum, send me that head and I re-diff.

**2. Honesty of the new label — honest, I carry it (it's the owner's §5).** It states the open-registration fact directly — "registers any account that asks, rate-limited" — which carries exactly what the dropped "not a vetting" clause guarded: anyone who asks gets the credential, so a demo credential is not a vetting. It makes no false claim about the pool, and NODE-OPERATOR keeps the "neither 'not KYC' nor vetted-anyone" guard. So dropping the explicit caveat is acceptable — the fact is stated, not inferred-only.

**The one over-read you flagged is real — my recommendation, non-blocking, owner's wording call.** "the exchange model where an account is opened first and KYC raises its limits" can be misread as THIS pool having KYC-tiered limits, which it does not (no limit layer exists here; the analogy is about the exchange). Grammatically it's the exchange's model, so it's not a false claim — but "KYC raises ITS limits" invites the bleed. Minimal, non-apologetic pin that keeps the owner's frame and can't read as the pool: say the limits are the exchange's, not the subject's — e.g. "the exchange model, where an account is opened first and KYC governs **the account's** limits" or "(as an exchange does)". That's a precision tweak, not a reframe, and it doesn't reintroduce any "out of scope"/apologetic tone the owner removed. Your call to pass the exact word to the owner; I don't need it changed to pass the code — it's a label-clarity note, and the owner owns the label.

So: code IMPL-PASS'd at 7a4f7ad (non-functional since 8ce47ae), label honest and carried, one optional precision note on the exchange-limits phrase. On merge I do the final check: `git diff <merged> 7a4f7ad -- backend/quorum` non-functional + deployed issuer from the merged head.
```

The dev session's follow-up `774309a` (SUBMISSION length) does not touch `backend/quorum`; the anchor stays `7a4f7ad`.

## 7. Final check on the merged head and the deployed service (same day, verbatim): the #100 audit is CLOSED

The code half, after the merge `7c4d401`:

```text
Merge verified firsthand — the code half of my final check PASSES. 7c4d401 is on main; `git diff 7a4f7ad 7c4d401 -- backend/quorum` empty (merged code == my anchor exactly); `git diff f9607f8 7c4d401 -- core backend/node` empty (the demo-issuer touched neither the pool core nor the node); merged issuer.mjs carries makeIssuer/rippledCode/disconnect. #100 is code-closed on main.

The deploy half is OWED, and it matters: your probe shows the live service is still f85a983 (old policy string), so the running endpoint lacks **F1 (my blocker — the cap race), F2, F3, and the nginx limit_req** (the three 200s). So right now the live demo endpoint is griefable on testnet — a burst can burn the issuer's testnet fees / lock its reserve and brick the demo issuer — until the owner restarts confidant-issuer from 7c4d401 AND the limit_req zone is in place. No real value, not an emergency, but worth the owner knowing before the demo leans on it; I've surfaced the restart as his deploy action.

When you re-probe green, confirm BOTH for my final close: (1) /issuer/health policy string == the merged POLICY const, and (2) the nginx request limit is actually in place (a 4th rapid GET → 429, or the owner confirms the zone). On both I do the final firsthand confirm and #100 closes. Code's sound and on main — good work.
```

The deploy half, after the owner's redeploy (the dev session's probe: `/issuer/health` policy string byte-identical to
the merged `POLICY`; all four pages byte-identical to `7c4d401`; eight rapid GETs on `/issuer/health` all 200, i.e. the
nginx zone not yet in place):

```text
Confirmed firsthand — I GET'd https://confidant.im/issuer/health myself and the policy string is byte-identical to 7c4d401's merged POLICY const (issuer rfSGoJ5…, 60/h). So the running service is the merged build: **F1 (my blocker), F2, F3 are LIVE.** Both halves of my final check are now done: code merged == anchor (verified), and deployed == merged (verified). **#100 audit is CLOSED.**

The nginx limit_req zone remaining does NOT hold the close: it was my hardening addition, not one of the findings, and with F1's issuance cap now live the residual is only read-flood grief on the issuer's RPC — no fund harm, testnet, LOW. It's the owner's ops config. Your light-probe tracking is the right call; tell me when a 429 appears or the owner confirms the zone and I'll note the hardening fully in place, but it's not blocking. Good work carrying this all the way through — F1 through the merge and onto the live endpoint.
```
