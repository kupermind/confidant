# REQ — pool total in `GET /fees` (`outstandingBacking`), shown by the UI as public

Sent 2026-10-21 from the implementation session to the audit session, verbatim (branch `pool-total`).

```text
REQ (impl, read-only field): expose the pool's backing in GET /fees and show it in the UI — branch
pool-total. Owner's request 2026-10-21: the page shows only the holder's own shielded balance; show
the whole pool's total too.

== What changes ==
backend/node/src/fees.rs: FeeReport gains `outstanding_backing: u64` (#[serde(default)], wire name
outstandingBacking) — the value of every note the pool still backs, in drops.
backend/node/src/node.rs: Node::fees() fills it from the existing Node::outstanding_backing()
(shields in minus unshields out, from the public value balances of the accepted bundles; the same
figure the sweep conservation check already uses, now reported in the clear).
frontend/index.html: GET /api/fees (coordinator pass-through, unchanged); the figure is shown in the
status bar ("holds: N XRP") and as a row "Pool total — N XRP (public)" under the holder's private
balance, with a tooltip stating why it is public. Demo mode mocks it.
Docs in the same branch: API-BOUNDARY (EN/RU) §2 + changelog, SPEC/SPEC.ru fee paragraph, ROADMAP
§5, DEMO.md talking point.

== Why it is safe to show ==
Public by construction: every shield and every unshield is a Payment on the ledger, so the pool's
total is already readable by anyone (the pool account's balance, up to unswept fees and reserve).
The field adds no information; it only saves the reader the subtraction. Only the split among holders
and the internal graph are private, and nothing in the field depends on them.

== No trust-path change ==
Nothing signs, applies or refuses differently. The field is read from state the node already keeps;
no new ingress, no new write. The coordinator passes /fees through unchanged.

== Tests ==
backend/node/tests/node.rs protocol_fees_accrue_net_and_sweep_verified_on_ledger: after 99.75 XRP
in and 40 out, GET /fees reports outstanding_backing == 59_750_000 and serializes it under
"outstandingBacking".

== Mutation table ==
Run alone against the fee test; files restored after each row.
M1 Node::fees() fills outstanding_backing with 0 instead of self.outstanding_backing() → killed
   (assert "GET /fees reports the backing").
M2 serde rename dropped (field serialized as outstanding_backing) → killed (assert "wire name").

== Coverage ==
cargo llvm-cov -p confidant-node --all-targets --ignore-filename-regex '(main|rpc)\.rs'
--fail-under-lines 95: see the PR.

== Asked ==
IMPL-PASS for the field as a read-only report; confirm the honest label ("public by construction;
only the split is private") is sufficient on the UI, or say what more it needs.
```
