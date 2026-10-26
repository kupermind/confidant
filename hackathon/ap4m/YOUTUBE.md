# The Mastercard-track demo video — upload sheet (YouTube)

The take of 2026-10-25 (`npm run record`, the stage configuration, run 6 of the live stack), cut and ready to upload.
Everything the YouTube form asks for is below, in the order the form asks for it; copy the fenced blocks as they are.

## The file

| | |
|---|---|
| Upload this | `~/Downloads/confidant-mastercard-ap4m-demo-2026-10-25.mov` |
| Length | 3:00 (the raw take ran 4:30; the recorder holds the closing card until its fixed length ends, so the tail was cut) |
| Picture | 2880 × 1800, 60 fps, H.264, no re-encoding (stream copy) — YouTube will offer it at 1440p |
| Sound | none — the raw take has no audio track (`screencapture -v -x`); nothing to mute |
| Raw take | `hackathon/ap4m/demo-mc-20261025-1157.mov` (gitignored, kept locally) |

How the cut was made, for the record (the closing card with the links appears at 2:49; three seconds less would have
shown it for one second — and a YouTube chapter must be at least ten seconds long):

```sh
ffmpeg -i hackathon/ap4m/demo-mc-20261025-1157.mov -t 180 -an -c:v copy -movflags +faststart \
  ~/Downloads/confidant-mastercard-ap4m-demo-2026-10-25.mov
```

## Where the URL goes

- **Mastercard AP4M form, field 6 "Demo Video URL"** — this video.
- The main hackathon form's "Demo Video URL" took the product walkthrough; its sheet is `docs/YOUTUBE.md`.

## YouTube settings

| Field | Value |
|---|---|
| Visibility | **Unlisted** (reachable by link, not searchable; the forms only need a link) |
| Audience | No, it's not made for kids |
| Category | Science & Technology |
| Video language | English |
| Title language / subtitles | English; no captions file (the captions are burnt into the picture) |
| License | Standard YouTube licence |
| Allow embedding | On (the sponsor's portal may embed it) |
| Comments | Off (nobody is watching the thread during the event) |
| Thumbnail | The automatic one is fine (the opening card). Alternative: `docs/assets/logo.png`, the same image as the platform's logo field |
| Monetisation / ads | Off |
| Playlist | none |

## Title (≤ 100 characters)

```text
Confidant × Mastercard AP4M — an AI agent with shielded XRP pays for a private transfer (testnet)
```

## Description (≤ 5000 characters)

The "Chapters" block turns into YouTube chapters by itself (first stamp 0:00, ascending, each ≥ 10 s). Keep the links
on their own lines so YouTube makes them clickable.

```text
An autonomous agent holding shielded funds in Confidant, a compliant shielded pool on the XRP Ledger, buys the relay of a private transfer and pays for it in RLUSD on XRPL EVM through Mastercard's Agent Pay for Merchants (AP4M). One live run on both testnets, recorded in one take: the terminal on the left, the explorer links opening on the right as they appear. Silent by design; every step is captioned on screen.

What happens, in order
0. Before it may pay, the agent proves to the pool's k-of-N quorum that it controls at least 12.95 XRP of unspent shielded funds. Every signer's node re-verifies the proof; the quorum writes an expiring XLS-70 credential (ConfidantPoF) on the agent's XRPL account, and the agent accepts it with its own signature. Machine-to-machine underwriting, read from the ledger.
1. A zero-knowledge transfer of 1 XRP inside the pool is built locally. The relay answers HTTP 402 and prices the relay at 0.1 RLUSD.
2. The agent reads the price and its channel budget (price 0.1 ≤ max 0.25 RLUSD) and decides to pay. No human.
3. It pays with an EIP-712 voucher, redeemed on the AP4M PaymentLedger: VoucherRedeemed(channel, claim) on XRPL EVM testnet. The metered payment is public.
4. The relay checks the claim on the EVM and the credential on XRPL, forwards the bundle once; the quorum anchors it on XRPL L1. The anchor memo names no sender, receiver or amount.
5. The relay signs a receipt with the merchant payout key the 402 named; the agent countersigns with its channel key and its XRPL key.
7. Anyone re-checks the receipt from public data (node verify-receipt.mjs): both signatures, the channel's parties, the redeemed voucher, the pool's anchor naming the bundle hash, the credential on the agent's account. No key, no log, no trust in us. All checks pass.

Chapters
0:00 Confidant × AP4M — pay for privacy
0:20 0 · The agent proves its shielded funds; the quorum writes the credential
0:51 1–2 · The relay answers 402; the agent decides to pay
1:18 3 · Paid — VoucherRedeemed on XRPL EVM
1:47 4–5 · Relayed and anchored on XRPL; the receipt is signed
2:20 7 · Anyone re-checks the receipt from public data
2:49 Two ledgers, three facts

This take on the ledgers (2026-10-25)
The agent's XRPL account, with the ConfidantPoF credential:
https://testnet.xrpl.org/accounts/rAMxdUQz71EzKwJVy5QpsvVBUFQAyxiti
CredentialCreate by the pool's quorum:
https://testnet.xrpl.org/transactions/D5EABFB6DD72E13E861A30770C4BE97801EC90AFA19C6640CA31709A19D35BC6
CredentialAccept by the agent:
https://testnet.xrpl.org/transactions/D49CE9E65DA14290E609367FE17F05E21E96A71B4552320FEC3740E207518173
VoucherRedeemed on the PaymentLedger (XRPL EVM testnet):
https://explorer.testnet.xrplevm.org/tx/0x387bc29ceb0ca112abca4936d4a331d635f7811e9568737e5cdb585cb68c21b3
The anchor on XRPL L1 (the pool account's memo names the bundle hash):
https://testnet.xrpl.org/transactions/D8BD290733B04E6F1BE2535D46D6B8175D6786E5E0F57D816C27CDB3FEDD9533
Bundle hash: a9f2edccb1ff85c9d53c30d2bcfbc99a06f9b36253b153966a33d37cba48b970

Not in this take: the RLUSD payout to the merchant. AP4M's own settlement service executes it; at recording time its settle call answered with an error page, so the agent reports "settlement pending — it settles automatically" and there is no payout line. The payouts of earlier runs are linked in the repository (hackathon/ap4m/STAGE.md).

Honest labels
Testnet only. The pool account and the proof verification are federated: a k-of-N quorum of independent signers. Collusion of k signers can drain the pool account but cannot spend anyone's notes. The proof of funds is demo-grade: the proof names the proven notes to the signers' nodes, so it degrades the holder's privacy; the accepted design proves control of unspent notes without revealing them and is the committed next step. Shield and unshield amounts and addresses are public. The metered payment is fully public (channel, merchant, amount, time). Nothing here is pre-recorded output.

Links
Site: https://confidant.im
How it works, Mastercard section: https://confidant.im/how-it-works.html#ap4m
Code: https://github.com/kupermind/confidant — the Mastercard track is hackathon/ap4m (agent, relay, receipt verifier, the one-command recorder)

Built at the AI & Fintech XRPL Hackathon, New York, October 24–25, 2026. Track: Lending & Borrowing; Mastercard AP4M sponsor prize.
```

## Tags (≤ 500 characters in total)

```text
Confidant, XRPL, XRP Ledger, Mastercard, AP4M, Agent Pay for Merchants, agentic payments, x402, HTTP 402, RLUSD, XRPL EVM, zero-knowledge, shielded pool, proof of funds, XLS-70, credentials, Halo 2, Orchard, AI agent, privacy, hackathon
```

## After the upload

1. Open the link in a private window: it plays, the chapters show under the progress bar, the links in the
   description are clickable.
2. Paste the URL into the Mastercard form (field 6).
3. Keep the `.mov` in Downloads and the raw take in `hackathon/ap4m/`; neither is committed.

## Captions (added after upload; the link does not change)

The caption file is `hackathon/ap4m/demo-mc.en.srt` (a copy sits in `~/Downloads`). The picture already carries the
recorder's captions; this track adds the plain-language narration under it, for viewers who turn CC on.

1. YouTube Studio → Content → this video → Subtitles (left menu).
2. Language English if asked → Add language → English → Subtitles → Add → **Upload file** → **With timing** →
   choose `demo-mc.en.srt` → Continue → Publish.

Cue times come from the agent's own millisecond stamps: the agent's clock starts 7.5 s into the video (the
proof-of-funds line, stamped 13 323 ms, first appears at 0:21).

## Chapter stamps — how they were read

From the frames of the cut, one every ten seconds (the agent's own `[ms]` stamps start about seven seconds into the
recording): proof built at 0:20, credential written 0:37 and accepted 0:42, the 402 and the decision 0:51, paid 1:18,
anchored and the receipt 1:47, settlement pending 2:17, the verifier from 2:20, all checks passed by 2:40, the
closing card 2:49. Approximate to a second or two; chapters need no better.
