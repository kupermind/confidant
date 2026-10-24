# UI rule — Occam's razor on every element

Set by the project lead on 2026-10-25 after using the *Prove funds* tab. It binds every page under
`frontend/` and every Claude session that edits one (see `CLAUDE.md`). It is a hard rule, not a style note.

## The rule

**Every link, button, term, field and input requirement a user meets must survive one question: can we
say, in one sentence, why the user has to do this here?** If we cannot, the element goes: prefill it,
derive it, move it to the other party, or drop it. The pages are for a **non-expert**. The ideal is the
Japanese one: **one button**.

Consequences, each one a check before a page ships:

1. **A field the user cannot know how to fill is not a field.** "Lender id — from the lender" is the
   canonical failure: nobody knows what to type. Such a value arrives in a link the counterparty
   generates, or is derived by the page, or is not needed.
2. **Defaults over choices.** Validity, units, the account to use: pick the sane default and apply it.
   A choice is shown only behind *Advanced*, and only if someone can say who needs it.
3. **One action per state.** At any moment the screen has one primary button, and its label says what
   happens when pressed ("Prove ≥ 10 XRP to Acme Loans"), not what the mechanism is.
4. **A term that needs a glossary is the wrong term.** Say what it does, not what it is called in the
   protocol. If a protocol word must appear, it is in a tooltip, never on the button or the label.
5. **A message is one sentence and one next step.** Never a paragraph with three buttons inside it.
   Secondary actions are a quiet line below, or live on the other party's page.
6. **The explanation is on the page, before the input, not in the error after it.**
7. **Walk it as a first-time user** with nobody explaining: every pause, every "what do they want
   here?", is a bug to fix before the PR is opened. Say in the PR description that this walk was done.
8. **Cut before you add.** A new feature that adds a field or a button must show which existing one it
   removed, or why none could be.

Honest labels (`SPEC.md`) stay as they are: the rule removes *noise*, never the *truth*. A one-sentence
honest limit next to the one button is correct; a paragraph of caveats is not.

## Worked example — Prove funds, as it is and as it should be

**As it is (2026-10-25):** the holder is asked for an amount, a borrower account, a "lender id", a
validity, then gets a paragraph that names four nodes, a ledger range and three different actions
(copy or download the package, rotate the notes, record on the ledger) with the buttons inline.

**As it should be:**
- The **lender's page** produces one link: `/?prove=<lender-id>:<amount>:<hours>` (plus the lender's
  display name). The lender sends that link to the borrower. Everything the holder was typing is in it.
- The holder opens the link and sees **one line and one button**: "Acme Loans asks you to prove you hold
  at least 10 XRP in the pool. Nothing moves, nothing is revealed but the amount." — **[Prove]**.
  Borrower = the wallet's own account (prefilled, shown, not asked). Validity = the default in the link.
- After it: one line, one button — "Verified by the pool's nodes. Valid for 24 hours." —
  **[Send the proof to Acme Loans]** (copies the package, or opens the lender's page with it). A quiet
  secondary line: "Record it on the XRP Ledger instead →" (the credential path), and a one-sentence
  honest limit: "This shows what you held at this moment; it does not lock anything."
- "Rotate your notes" is not a user decision: the wallet does the self-transfer after a proof by default
  (or the hint lives in the wallet card, not in the proof result).
- The technical facts (which nodes, which ledger, the root) are a *Details* fold, not the message.

The same walk applies to the lender page, the issuer/provider page and the Compliance row.
