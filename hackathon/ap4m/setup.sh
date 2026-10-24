#!/usr/bin/env bash
# Fetches the hackathon-only AP4M kit next to the scripts (nothing is vendored into git).
set -euo pipefail
cd "$(dirname "$0")"
API="${AP4M_API_URL:-https://hackathon.ap4m.build}"
npm install --no-audit --no-fund ethers@6.17.0 "$API/sdk/ap4m-sdk.tgz"
curl -fsSL -o ap4m-hackathon-sdk.cjs "$API/sdk/ap4m-hackathon-sdk.cjs"
curl -fsSL -o ap4m-x402.mjs "$API/sdk/ap4m-x402.mjs"
node --check relay.mjs && node --check agent.mjs
echo "ok — fill .env (copy env.example) and run: node relay.mjs · node agent.mjs"
