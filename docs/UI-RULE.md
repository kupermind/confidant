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

## Worked example — Prove funds, as it was and as it is

**As it was (2026-10-25, morning):** the holder was asked for an amount, a borrower account, a "lender id", a
validity, then got a paragraph that named four nodes, a ledger range and three different actions (copy or
download the package, rotate the notes, record on the ledger) with the buttons inline. The lender page had a
"draft the request" section with an id nobody recognised, a paste box, a file picker and a "verify with every
node" button, then a ledger section nobody understood.

**As it is (2026-10-25, after the lead's review):**
- **No field.** The app fills everything itself: the amount is the whole private balance (it always attests the
  maximum), the borrower is the wallet's own XRPL account, the "lender id" is a request id the app generates, the
  validity is 24 hours. The button says what it will do: **[Prove I hold 25 XRP]**.
- **One press does the whole thing.** Entry credential if the account lacks it (issued from the issuer key in this
  browser, confirmed by the account) → Prove → the quorum writes the credential → the account's signature confirms it
  → the wallet refreshes its notes by itself. "Rotate your notes" was never a user decision, and neither was "go to
  the issuer page and press Issue". The former issuer page is now the *under the hood* explanation (what each step
  wrote on the ledger, what the gate is mechanically) with the team's key fold at the bottom — the one page a judge
  opens to ask "what just happened"; the app links to it next to **Download the receipt**.
- **One line after it:** "controlled at least 25 XRP as of ledger L, confirmed by your signature, expires …", with
  the lender's link. No package to copy or download — the hand-off is the borrower's address. The technical facts
  (request id, window, signers, URI, bundle hash) are a *Details* fold, not the message.
- **The lender page is one field and one button:** the borrower's address (prefilled from the link, checked by
  itself) and **[Check]**, reading the credential from the ledger; then one line and a short "told / not told / not a
  lock" card. The lender never has to ask the borrower for anything.

The first sketch had the lender produce a link with an amount and an id for the holder to open; the credential
path made that unnecessary, so it was dropped rather than built.

The same walk applies to the issuer/provider page and the Compliance row.
