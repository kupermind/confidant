#!/bin/bash
# One command for the Mastercard-track demo video — nothing to edit afterwards.
#
#   ./record-demo.sh            # records the whole screen (macOS screencapture) while it runs the live flow
#   NO_CAPTURE=1 ./record-demo.sh   # the same run without the recorder (use QuickTime yourself)
#   REPLAY=path/to/run.log ./record-demo.sh   # rehearsal: replays a saved run instead of paying — NOT for the real video
#   OVER_CAP=0x<channel> ./record-demo.sh     # after the run, the over-cap refusal on that (near-empty) channel
#   BROWSER_APP='Google Chrome' ./record-demo.sh   # open the explorer links in that browser instead of the default one
#
# What it does, in order: pre-flight (health, channel, balance, no pending run) → starts the recorder → the agent's real
# run, with a caption printed under each milestone and the explorer link opened in your browser as it appears → the
# receipt verified from public data → (optional) the over-cap refusal → the closing card → stops the recorder.
# Layout before you start: this terminal on the left (≥ 18 pt, ≤ 100 columns), the browser on the right; new tabs open
# there by themselves. macOS asks once for Screen Recording permission for the terminal app.
set -u
cd "$(dirname "$0")"
trap 'rc=$?; printf "exit %s at %s\n" "$rc" "$(date +%H:%M:%S)" > "$PWD/.last-run"' EXIT   # for whoever watches from outside
rm -f "$PWD/.last-run"
set -a; . ./.env 2>/dev/null; set +a
RELAY_URL="${RELAY_URL:-http://127.0.0.1:4309}"; RELAY_URL="${RELAY_URL%/}"
OUT="${OUT:-$PWD/demo-mc-$(date +%Y%m%d-%H%M).mov}"
CAP_MAX="${CAP_MAX:-270}"   # the recording's length: screencapture writes the file only when its own duration ends, so the
                            # script holds the closing card until then (trim the tail in QuickTime if you like)

B=$'\e[1m'; V=$'\e[38;5;99m'; G=$'\e[38;5;71m'; D=$'\e[2m'; R=$'\e[0m'
say() { printf '\n'; [ -n "$1" ] && printf '%s%s▌ %s%s\n' "$V" "$B" "$1" "$R"; [ -n "${2:-}" ] && printf '%s%s  %s%s\n' "$V" "$B" "$2" "$R"; printf '\n'; }
card() { printf '\n\n%s%s  %s%s\n' "$V" "$B" "$1" "$R"; shift; for l in "$@"; do printf '%s  %s%s\n' "$B" "$l" "$R"; done; printf '\n'; }
openurl() { [ -n "${NO_OPEN:-}" ] && return 0; command -v open >/dev/null || return 0; if [ -n "${BROWSER_APP:-}" ]; then open -a "$BROWSER_APP" "$1" >/dev/null 2>&1 || true; else open "$1" >/dev/null 2>&1 || true; fi; }
pause() { sleep "${1:-3}"; }

# ---- pre-flight: fail here, before anything is recorded or paid -----------------------------------------------------
node -e 'const [a,b]=process.versions.node.split(".").map(Number); process.exit(a>22||(a===22&&b>=13)?0:1)' 2>/dev/null || { echo "pre-flight: Node $(node -v 2>/dev/null || echo '(none)') on PATH — the AP4M kit needs Node ≥ 22.13 (nvm use 22 / put it first on PATH)"; exit 2; }
if [ -z "${REPLAY:-}" ]; then
  [ -f agent-state/pending.json ] && { echo "pre-flight: agent-state/pending.json exists — finish or set aside that run first (STAGE.md §1 step 4)"; exit 2; }
  [ -n "${CHANNEL_ID:-}" ] || { echo "pre-flight: CHANNEL_ID is empty — set the stage channel in .env (opening a new one is refused by the platform cap)"; exit 2; }
  curl -s --max-time 10 "$RELAY_URL/relay/health" | grep -q '"ok":true' || { echo "pre-flight: the relay at $RELAY_URL does not answer /relay/health"; exit 2; }
  BAL=$(node -e 'const {execFileSync}=require("child_process");const e=process.env;try{const r=JSON.parse(execFileSync(e.CLI||"../../target/release/examples/cli",["wallet","--file",e.WALLET_FILE||"agent-state/agent.wallet.json","balance",e.LOG_FILE||"agent-state/log.json"],{encoding:"utf8",stdio:["ignore","pipe","ignore"]}));console.log(r.balance)}catch{console.log(0)}')
  [ "${BAL:-0}" -ge 1000000 ] 2>/dev/null || { echo "pre-flight: the agent's private balance is ${BAL:-?} drops (< 1 XRP) — shield first (prepare-wallet)"; exit 2; }
fi
XRPL_ACCT=$(node -e 'try{console.log(JSON.parse(require("fs").readFileSync("agent-state/xrpl.json","utf8")).address)}catch{console.log("")}')

# ---- the recorder: probe it BEFORE anything is paid (macOS needs Screen Recording permission for the terminal app,
# and that permission only takes effect after the app is relaunched) -------------------------------------------------
CAP_PID=""
if [ -z "${NO_CAPTURE:-}" ] && [ -z "${REPLAY:-}" ] && command -v screencapture >/dev/null; then
  PROBE="$(mktemp -d)/probe.mov"
  screencapture -v -x -V 1 "$PROBE" >/dev/null 2>&1
  if [ ! -s "$PROBE" ] || [ "$(stat -f %z "$PROBE" 2>/dev/null || echo 0)" -lt 2000 ]; then
    echo "pre-flight: the screen recorder produced nothing — allow Screen Recording for this terminal app"
    echo "  (System Settings → Privacy & Security → Screen Recording), then QUIT and reopen the terminal app and run again."
    echo "  Nothing was recorded or paid. (NO_CAPTURE=1 runs the take without the recorder.)"
    rm -rf "$(dirname "$PROBE")"; exit 2
  fi
  rm -rf "$(dirname "$PROBE")"
  screencapture -v -x -V "$CAP_MAX" "$OUT" >/dev/null 2>&1 &
  CAP_PID=$!
  sleep 2
fi
clear
card "Confidant × AP4M — pay for privacy" "An agent with shielded funds on the XRP Ledger buys the relay of a private transfer." "It pays RLUSD on XRPL EVM through Mastercard's AP4M. Live on both testnets; nothing pre-recorded."
pause 4

# ---- the run ---------------------------------------------------------------------------------------------------------
RUN_MARK="$(mktemp)"   # receipts newer than this file belong to this run
say "Running the agent" "node agent.mjs — every line below is the live run"
pause 1
if [ -n "${REPLAY:-}" ]; then
  printf '%s  (REPLAY of %s — a rehearsal, not a live run)%s\n' "$D" "$REPLAY" "$R"
  SRC="while IFS= read -r l; do printf '%s\n' \"\$l\"; sleep 1.2; done < \"$REPLAY\""
else
  SRC="node agent.mjs 2>&1"
fi
eval "$SRC" | while IFS= read -r line; do
  case "$line" in *ExperimentalWarning*|*trace-warnings*) continue;; esac
  printf '%s\n' "$line"
  case "$line" in
    *"proof of funds built"*)      say "0 · Prove shielded funds" "Before it may pay, the agent proves it controls at least X XRP in the pool — to the quorum, not to the merchant.";;
    *"quorum wrote the credential"*) say "" "Every signer's node re-verified the proof; k of N wrote an expiring XLS-70 credential (ConfidantPoF) on the agent's XRPL account.";;
    *"credential accepted"*)       [ -n "$XRPL_ACCT" ] && openurl "https://testnet.xrpl.org/accounts/$XRPL_ACCT"; say "" "The agent accepted it with its own signature. Machine-to-machine underwriting, read from the ledger. (Explorer: the agent's account.)";;
    *"402 received"*)              say "1 · The relay answers 402" "A zero-knowledge transfer was built locally. The relay prices the relay in RLUSD.";;
    *"policy: "*"paying"*)         say "2 · Decision, no human" "The agent read the price and its channel budget and decided to pay.";;
    *"policy: "*"NOT paying"*)     say "Decision: refused" "Over the funded limit, the agent does not pay. The limit is enforced by the contract; the agent decides inside it.";;
    *"paid: claim"*)               u=$(printf '%s' "$line" | grep -oE 'https?://[^ ]+' | head -1); [ -n "$u" ] && openurl "$u"; say "3 · Paid — the metered payment is public" "An EIP-712 voucher, redeemed on the PaymentLedger: VoucherRedeemed(channel, claim). (Explorer: XRPL EVM.)";;
    *"relayed: anchored"*)         u=$(printf '%s' "$line" | grep -oE 'https?://[^ ]+' | head -1); [ -n "$u" ] && openurl "$u"; say "4 · Relayed and anchored — the settlement is private" "The relay checked the claim on the EVM and the credential on XRPL, forwarded once; the quorum anchored it. No sender, receiver or amount. (Explorer: the pool account.)";;
    *"receipt signed by the channel"*) say "5 · The receipt" "Signed by the merchant payout key the 402 named; countersigned by the agent with its channel key and its XRPL key.";;
    *"settled: payout"*)           u=$(printf '%s' "$line" | grep -oE 'https?://[^ ;]+' | head -1); [ -n "$u" ] && openurl "$u"; say "6 · Settled" "RLUSD reached the merchant payout address. (Explorer: XRPL EVM.)";;
  esac
done
pause 3

# ---- the verifier ----------------------------------------------------------------------------------------------------
RCPT=$(find agent-state/receipts -name '*.json' -newer "$RUN_MARK" 2>/dev/null | head -1); rm -f "$RUN_MARK"
if [ -n "$RCPT" ]; then
  say "7 · Anyone re-checks the receipt from public data" "node verify-receipt.mjs $RCPT — no key, no log, no trust in us"
  pause 2
  if [ -n "${REPLAY:-}" ]; then printf '%s  (replay: the verifier runs for real against the ledgers)%s\n' "$D" "$R"; fi
  node verify-receipt.mjs "$RCPT" 2>&1 | grep -vE 'ExperimentalWarning|trace-warnings'
else
  say "7 · Receipt" "this run produced no receipt — it did not reach the relay (see the lines above); nothing to verify"
fi
pause 4

# ---- optional: the over-cap refusal -----------------------------------------------------------------------------------
if [ -n "${OVER_CAP:-}" ] && [ -z "${REPLAY:-}" ]; then
  say "8 · Over the funded limit" "the same agent on a channel with 0.05 RLUSD left"
  pause 1
  OVER_CAP_DEMO=1 CHANNEL_ID="$OVER_CAP" node agent.mjs 2>&1 | grep -vE 'ExperimentalWarning|trace-warnings' | while IFS= read -r line; do
    printf '%s\n' "$line"
    case "$line" in *"NOT paying"*) say "" "Refused: nothing signed, nothing paid. The limit is funded and enforced by the contract.";; esac
  done
  rm -f agent-state/pending.json   # the refused run leaves an unpaid pending file; it is not a paid claim
  pause 3
fi

# ---- closing card ------------------------------------------------------------------------------------------------------
card "Two ledgers, three facts" "The agent proved its shielded funds. The metered payment is public. The settlement is private." "Proof of funds: demo-grade (the proof names the proven notes to the signers' nodes). Testnet only." "confidant.im · github.com/kupermind/confidant · hackathon/ap4m"
[ -n "$CAP_PID" ] && printf '%s  (the recording ends by itself at %s s — leave the windows as they are)%s\n' "$D" "$CAP_MAX" "$R"
pause 2
if [ -n "$CAP_PID" ]; then
  # Never signal screencapture: a signalled capture is discarded. Hold the closing card until its duration ends.
  while kill -0 "$CAP_PID" 2>/dev/null; do sleep 2; done
  for i in 1 2 3 4 5 6 7 8 9 10; do [ -s "$OUT" ] && break; sleep 1; done
  if [ -s "$OUT" ]; then printf '%s  recording saved: %s (%s)%s\n' "$G" "$OUT" "$(du -h "$OUT" | cut -f1)" "$R"; printf '%s\n' "$OUT" > "$PWD/.last-recording";
  else printf '%s  recording NOT found at %s%s\n' "$V" "$OUT" "$R"; fi
fi
