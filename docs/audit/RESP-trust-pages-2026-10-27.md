# RESP-1 · TRUST pages at /trust and /trust-technical (PR #114)

<!-- Committed verbatim by the dev session except three edits for this public repo's rules: the auditor's handle is replaced by its role, and two negative lists naming chips/vendors and private repositories are replaced by their description. Findings and verdict unchanged. -->

**Status:** DONE → IMPL-PASS (clean; 2 notes, no must-fix)
**Request:** REQ-1 · 2026-10-27
**Auditor session:** the audit session (security audit)
**PR:** kupermind/confidant#114 — branch `trust-page`, **reviewed at head `f485c4c`** (base `259b492`, the merge of #113). CI green on five jobs; manual e2e skipped.
**Scope reviewed:** the full PR diff at `f485c4c` — `backend/deploy/nginx/confidant-ui.conf`, `backend/deploy/nginx/confidant-ui-tls.conf`, `frontend/trust.html`, `frontend/trust-technical.html`, `frontend/how-it-works.html`, `frontend/test/browser-smoke.mjs`, `README.md`. The diff touches exactly these 7 files (+1540 / −1). Nothing under `core/`, `backend/node`, or `backend/quorum` is touched — confirmed by the diff, consistent with the request's "out of scope".

---

## Verdict: IMPL-PASS

The actual committed artifact at `f485c4c` was reviewed (not a design). All five asks pass. Two advisory **notes** below; neither blocks merge.

---

## 1. nginx — PASS

Both configs gain exactly two exact-match locations before `location /`:

```
location = /trust            { rewrite ^ /trust.html last; }
location = /trust-technical   { rewrite ^ /trust-technical.html last; }
```

- **No shadowing of `/api/`, `/wallet/`, `/node/<i>/…`.** These are exact-match (`=`) locations — the highest-precedence match in nginx, and they match *only* the literal URIs `/trust` and `/trust-technical`. They cannot affect the prefix locations `/api/`, `/wallet/` or the regex `^/node/(?<i>[0-9])/(pof/verify|state)$`, which are different URIs. `= /trust` also does not catch `/trust-technical` (exact match requires the whole URI). Verified against the full `confidant-ui.conf` (`:18-19` the new locations, `:30` `/api/`, `:37` `/wallet/`, `:45` `/node/…`).
- **Headers apply to the pages.** `rewrite ^ /trust.html last;` is an *internal* rewrite; the `last` flag re-runs location matching against `/trust.html`, which matches `location /` (`:20-28`). The response is therefore generated in `location /` context and inherits its `add_header` set: `Cache-Control: no-cache`, `Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Embedder-Policy: require-corp`. The `= /trust*` blocks carry no `add_header` of their own, so there is no nginx add-header-override that would strip the inherited set. Confirmed firsthand that the **TLS** conf's HTTPS-server `location /` carries the identical three headers (not assumed — read directly), so **both configs have the same effect** for the pages.
- **No redirect, no traversal, no proxy change.** The rewrite uses `last`, not `redirect`/`permanent` — the browser URL stays `/trust`, no 3xx. The target is a fixed literal (`/trust.html`); no request-controlled input is interpolated into a path, so there is no traversal surface. The `/api/`, `/wallet/`, `/node/…` proxy blocks are byte-unchanged by the diff. On the TLS setup, `http://host/trust` is 301'd to HTTPS by the port-80 redirect server's `location /` (path preserved via `$request_uri`), then served by the HTTPS server's `= /trust` — correct.

## 2. Static only — PASS

Reviewed both pages at `f485c4c`:

- **No active content.** `trust.html` and `trust-technical.html` each contain **zero** `<script>`, zero `<form>`, zero `<img>`, zero inline event handlers (`onclick`/`onload`/`onerror`), no `fetch(`/`XMLHttpRequest`, no `<iframe>`, and no tracker (no `gtag`/`google-analytics`/`googletagmanager`/`mixpanel`/`segment`/`hotjar`). Verified by grep over the full file contents.
- **Only Google Fonts loads from the network.** The sole external resource references are `fonts.googleapis.com` (preconnect + two `<link rel=stylesheet>`) and `fonts.gstatic.com` (preconnect `crossorigin`). Every other `https://` reference is an `<a href>` navigation target the user clicks (`ph18.io`, `xperp.fi`, `confidant.im`, `github.com/77ph`, `github.com/kupermind`, `x.com/ph8n1x`, `x.com/xperpfi`) — not a resource load. Google Fonts serves `Cross-Origin-Resource-Policy: cross-origin` on both the CSS and the font files, so it satisfies the site's `require-corp`; the `fonts.gstatic.com` preconnect correctly carries `crossorigin`. This is the known-working Fonts-under-COEP pattern and matches the request's stated expectation.

## 3. Honest labels against the hard constraints — PASS (1 note)

- **"Trustless" appears only as the do-not-say rule.** One occurrence per page, both negating the word: `trust.html` "…mainnet and full privacy are in development. 'Trustless' is [the wrong word]…"; `trust-technical.html` "'Trustless' is wrong — the truth is specific." No page claims trustlessness.
- **No forbidden over-claims.** No "untraceable", "fully anonymous", or "private from the operator" anywhere in either page (grep-confirmed absent).
- **Every status is testnet / in development.** "All applications run on testnet today; mainnet and full privacy are in development"; status rows read "In development", "Designed, mathematically grounded, not yet live", "Mainnet launch and hardening of operator independence". **No "live on mainnet" claim** — every `mainnet` mention is roadmap/in-development or the pre-mainnet-gating note "findings resolved or gated pre-mainnet". The security-maturity line is honestly scoped: "20+ internal adversarial security-review rounds (REQ/RESP)" with no third-party-audit claim.
- **TEE stays a production footnote.** The only entry point is a single link in `how-it-works.html` under the closing section, `<a href="trust.html" target="_blank" rel="noopener">` — `rel="noopener"` is correct hygiene for `target=_blank`. The app and demo pages are otherwise unchanged. README states "none of it is part of the demo."
- **Chip/vendor names: none.** Neither page names a TEE chip or vendor — grep-confirmed against the list of chip, vendor and platform names. `trust.html` names a few industry building blocks (threshold Schnorr, Taproot, multisig); `trust-technical.html` names more (FROST, Schnorr, MuSig, P2WSH, P2TR, Taproot, SignerList, "threshold signature"). **Ruling on acceptability (the request asked me to rule):** naming industry-standard cryptographic primitives and Bitcoin script types on the public site is **ACCEPTABLE**. These are public, textbook building blocks; they disclose nothing proprietary, no chip/vendor, and no attack surface specific to the deployment. Threshold signatures are an *integrity* mechanism, not a secret. The page's own framing — "industry-standard building blocks; specifics are current as of the testnet stage" — is the correct honest framing. The line that would be a problem (a chip or vendor name) is absent.

**Note 3a (not must-fix):** `trust-technical.html` describes the margin engine as "operators can't front-run or see positions." This is the legitimate TEE property (an operator cannot read the enclave's state) and is correctly distinct from the forbidden privacy over-claim "private from the operator" (which is about graph privacy of the shielded pool). It is acceptable as written. Recommendation only: prefer the framing "operators cannot read the state inside the enclave" across the stack so the wording never drifts toward the forbidden phrase.

## 4. Public-repo hygiene — PASS

- **No real names.** Neither page contains a personal name or email (grep-confirmed). Attribution is GitHub handles only: `github.com/77ph`, `github.com/kupermind`, plus X handles `@ph8n1x`, `@xperpfi` — consistent with the existing decks.
- **No private-repo or private-file references.** No references to private repositories, organisations or file paths (grep-confirmed against the list of private names). The GitHub links are user/org pages, which expose only public repos to the public.

## 5. Tests — PASS (1 note)

`browser-smoke.mjs` adds both pages to the no-horizontal-overflow check at 320 px and 390 px, the same coverage the other static pages get (`index`, `how-it-works`, `balances`, `lender`). This is **adequate for two static pages**: layout/overflow is the regression-prone property for static content, and the commit under review (`f485c4c`, "the header pill wraps on narrow phones… 6 px past 320") is itself a layout fix the smoke test is designed to catch. The e2e skip is acceptable — the change is two static pages plus a config addition, with no backend-behavior path to exercise.

**Note 5a (not must-fix):** the static-only property (no `<script>`/form/tracker) is verified here but is not regression-locked by a test. If cheap, a one-line assertion in the smoke test that `trust.html`/`trust-technical.html` contain no `<script` would keep a future edit from silently adding active content to these public pages. Optional.

---

## Findings summary

| # | Severity | Status |
|---|----------|--------|
| 3a | note | TEE wording: prefer "operators cannot read the state in the enclave" over "can't see positions" (acceptable as-is) |
| 5a | note | optional smoke-test assertion to regression-lock "no `<script>` on the TRUST pages" |

No must-fix findings.

## Artifact identity

This IMPL-PASS binds to **`f485c4c`**. If the PR head moves before merge, re-confirm the diff is byte-identical on the 7 files (especially the two nginx confs and the two HTML pages) before relying on this ruling. The dev session commits this verbatim as `docs/audit/RESP-trust-pages-2026-10-27.md`, indexed in `docs/audit/INDEX.md`, before merge.
