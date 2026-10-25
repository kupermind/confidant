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
