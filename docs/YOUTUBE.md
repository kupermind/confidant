# The main demo video — upload sheet (YouTube)

The walk-through of the live site recorded on 2026-10-25 (Shift+Cmd+5, the Chrome window on `confidant.im`), for the
hackathon form's "Demo Video URL". The Mastercard-track video has its own sheet, `hackathon/ap4m/YOUTUBE.md`.

## The file

| | |
|---|---|
| Upload this | `~/Downloads/confidant-pitch-demo-2026-10-25.mov` (5:10, the full take) |
| Or the trimmed one (preferred) | `~/Downloads/confidant-pitch-demo-2026-10-25-trimmed.mov` — 4:05, the same take cut where the app walk-through ends; the last minute of the full take is a blank browser tab. Stream copy: `ffmpeg -i <full> -t 245 -c:v copy -movflags +faststart <trimmed>` |
| Picture | 3104 × 1964 (the Chrome window with its shadow), H.264 |
| Sound | none — the microphone was off; the pages are self-explanatory and judges watch muted |

## Where the URL goes

The hackathon form, "Demo Video URL". The pitch video (180 s, vertical, spoken) is a separate field; this is not it.

## YouTube settings

Visibility **Unlisted** · not made for kids · category Science & Technology · language English · comments off ·
embedding on · the automatic thumbnail, or `docs/assets/logo.png`.

## Title (≤ 100 characters)

```text
Confidant — a compliant shielded pool on the XRP Ledger: shield, prove funds, audit (live, testnet)
```

## Description (≤ 5000 characters)

The "Chapters" block becomes YouTube chapters by itself. Keep the links on their own lines.

```text
Confidant is a compliant shielded pool on the XRP Ledger. Value goes in, moves privately, and comes out, without exposing amounts or who paid whom; an auditor can still open it with the holder's viewing key, and no single operator can betray it. This is the live site, https://confidant.im, on the XRPL testnet, recorded in one take with nothing pre-recorded.

What the video shows
How it works: on a public ledger every payment is a disclosure; Confidant hides the graph. Value enters the pool as a private note (a Zcash Orchard commitment, proven with zero knowledge, no trusted setup); inside the pool notes move by proof alone; on the ledger you see only an anchor, never a link. The Mastercard section: an agent with shielded funds pays for the relay of a shielded transfer through Mastercard's AP4M.
Balances: the pool account's balance on the ledger and what the notes are worth, with every drop of the difference accounted for, live from the ledger.
The app, in the browser: your XRPL account and your private wallet side by side; the entry credential (XLS-70) the pool's gate requires, issued in demo mode.
Shield: 10 XRP moves from the account into the pool as a private note. The proof is built in the browser, the deposit is a Payment with a memo, and 2 of 3 signers anchor it.
Unshield: a note becomes a payout to any XRPL address.
Prove funds, one button: the browser proves to the pool's quorum that the account controls at least its whole private balance; every signer re-checks the proof on its own node and the quorum writes an expiring ConfidantPoF credential on the XRPL account. A lender checks it on the lender page by the address alone; the explorer shows the credential and the holder's own acceptance. Then the app refreshes the notes so the proof cannot be linked to the next payment.
Audit: the holder hands an auditor a viewing key; the auditor sees the full history of that wallet, nothing about anyone else, read-only.
Off screen, from 4:07: the walkthrough moved to a private browser window, which the window recording did not capture. It showed a first-time visitor: a new wallet created in the browser, the new XRPL account funded from the faucet, and on the first shield the entry credential issued by the demo issuer and confirmed with the account's own signature. The captions describe it; anyone can repeat it at confidant.im in a private window.

Chapters
0:00 How it works — the graph hidden, lending on shielded capital, the Mastercard track
0:28 Balances — the pool account and the notes, reconciled from the ledger
0:55 The app — the public account, the private wallet, the entry credential
1:05 Shield — 10 XRP becomes a private note, anchored by 2 of 3 signers
1:53 On the explorer — the deposit and the anchor
2:08 Transfer and unshield
2:28 Prove funds — one button; the quorum writes a credential on the account
3:09 The lender's page — check a borrower by address
3:20 On the explorer — the credential, its acceptance, the anchor
3:43 Notes refreshed — the proof cannot be linked to the next payment
3:55 Audit — a viewing key opens the full history to an auditor
4:07 Off screen — a new account, the faucet, the entry credential on the first shield

What was built when
The shielded pool itself (the Rust core, the reused Orchard circuit, the quorum of verifier nodes, the XRPL integration on testnet) existed before the hackathon and is tagged as the baseline. Built on site, October 24–25, 2026: the credential gate with the demo issuer, the proof of funds with the quorum-signed on-ledger credential and the lender page, the one-button app pages, the balances and how-it-works pages, and the Mastercard AP4M relay and agent with signed receipts anyone can re-check.

Honest labels
Testnet only. Notes are non-custodial. The pool account and the proof verification are federated: a k-of-N quorum of independent signers, because today's XRPL has no on-ledger verifier for this proof system. Collusion of k signers can drain the pool account but cannot spend anyone's notes. The state is reconstructible from the ledger. Shield and unshield amounts and addresses are public; an empty pool has no privacy. The credential gate in the demo is the real, working system with an open admission policy ("KYC in demo mode"). The proof of funds is demo-grade: the proof names the proven notes to the signers' nodes, so it degrades the holder's privacy; the accepted design proves control of unspent notes without revealing them and is the committed next step.

Links
Site: https://confidant.im
How it works: https://confidant.im/how-it-works.html
Code and design of record: https://github.com/kupermind/confidant
The Mastercard-track demo video is a separate upload.

Built at the AI & Fintech XRPL Hackathon, New York, October 24–25, 2026. Track: Lending & Borrowing; Mastercard AP4M sponsor prize.
```

## Tags (≤ 500 characters in total)

```text
Confidant, XRPL, XRP Ledger, shielded pool, zero-knowledge, Zcash Orchard, Halo 2, privacy, compliance, viewing key, selective disclosure, proof of funds, XLS-70, credentials, lending, DeFi, hackathon, testnet, Mastercard, AP4M
```

## Captions (added after upload; the link does not change)

The caption file is `docs/confidant-demo.en.srt` (a copy sits in `~/Downloads`). It is timed to the full take and fits
the trimmed file too: the cues past 4:05 simply never show there.

1. YouTube Studio → Content → this video → Subtitles (left menu).
2. Language English if asked → Add language → English → Subtitles → Add → **Upload file** → **With timing** →
   choose `confidant-demo.en.srt` → Continue → Publish.

If the trimmed file was uploaded, delete the last chapter line (4:07) from the description: a chapter past the end
makes YouTube drop all of them.

## Chapter stamps — how they were read

From one frame every five seconds of the take and the page-change times: the app for two seconds, how-it-works from
0:03 (its lending section 0:13, Mastercard 0:19), balances 0:28, the app 0:55, shield pressed 1:12 and done 1:47, the
explorer 1:53 (the deposit) and 2:02 (the anchor), transfer 2:08, unshield 2:18, prove funds 2:28, its "Done" by 3:05,
the lender page 3:09, the explorer 3:23 (CredentialCreate), 3:30 (CredentialAccept) and 3:38 (the anchor), notes
refreshed 3:43, audit 3:55, the browser menu and a blank tab from 4:07 to the end at 5:10.
