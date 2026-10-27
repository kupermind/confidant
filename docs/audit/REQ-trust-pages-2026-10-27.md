<!-- Copied from the shared request queue (REQ-1, 2026-10-27); the status line reflects the ruling. -->

## REQ-1 · 2026-10-27 · Audit: Confidant PR #114 — TRUST pages at /trust and /trust-technical

**Status:** DONE → RESP-1-trust-pages-2026-10-27.md (security-audit session, 2026-10-27) — IMPL-PASS at head `f485c4c`, 2 notes, no must-fix
**From:** the dev session for this PR, for the project lead
**PR:** https://github.com/kupermind/confidant/pull/114 — branch `trust-page`, head `f485c4c`, two commits on `main` @ `259b492`. CI is green on all five jobs; the manual e2e was skipped.

**What changed**
- `frontend/trust.html` and `frontend/trust-technical.html` are new static pages about TRUST, the platform the pool is planned to run on in production.
  - They are generated from canonical TRUST pages kept outside the repo: comments are stripped and the cross-links renamed.
  - The bridge overview is embedded in both as a declarative shadow root, shown when the fragment is `#bridge`.
- `frontend/how-it-works.html`: one added line under "Where it goes next", linking to `trust.html` in a new tab.
- `backend/deploy/nginx/confidant-ui.conf` and `confidant-ui-tls.conf` each gain `location = /trust { rewrite ^ /trust.html last; }` and the same for `/trust-technical`.
- `frontend/test/browser-smoke.mjs`: both pages are added to the no-horizontal-overflow check at 320 and 390 px.
- `README.md`: the two pages are named next to the overview.

**Why it needs you:** the repo's audit cycle requires a slice ruling for every change under `backend/` before merge, and the nginx change is under `backend/deploy/`. The pages are also a new public surface.

**Ask — please rule on**
1. **nginx.**
   - The two exact-match locations cannot shadow `/api/`, `/wallet/` or `/node/<i>/…`.
   - The internal rewrite lands in `location /`, so the COOP/COEP and `Cache-Control` headers still apply to the pages.
   - There is no redirect or traversal surface, and no proxy path changes. Both configs have the same effect.
2. **Static only.**
   - Confirm that neither page has a `<script>`, a form or a tracker.
   - Confirm that nothing loads from the network except Google Fonts, which is served with `Cross-Origin-Resource-Policy: cross-origin` and so works under the site's `require-corp`.
3. **Honest labels against the repo's hard constraints.**
   - TEE stays a production footnote: the app and demo pages are unchanged, and the only entry point is one link from "Where it goes next".
   - Every status on the pages says testnet or in development.
   - There are no over-claims. The pages' line "'Trustless' is the wrong word" is the do-not-say rule itself.
   - `trust.html` names no chip or vendor. `trust-technical.html` names industry building blocks: threshold-signature schemes and Bitcoin script types. Rule whether that is acceptable on the public site.
4. **Public-repo hygiene.** No real names; GitHub handles only, as on the existing decks. No references to private repositories or files.
5. **Tests.** Is the smoke-test addition adequate for two static pages?

**Out of scope:** `core/`, `backend/node`, `backend/quorum`; none of it is touched.

**Deliverable:** `RESP-1-trust-pages-2026-10-27.md` here, in the repo's audit format.
- A verdict: DESIGN-PASS, IMPL-PASS, CONDITIONAL or FAIL.
- Findings marked must-fix or note, pinned to the commit you reviewed.
- The dev session commits it verbatim as `docs/audit/RESP-trust-pages-2026-10-27.md`, indexed in `docs/audit/INDEX.md`, before merge.
