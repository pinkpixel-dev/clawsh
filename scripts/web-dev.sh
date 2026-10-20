#!/usr/bin/env bash
# Runs a throwaway Claw$h for testing phone access. It builds the web client,
# then starts a release build with its own session file and one fresh tab, so
# it never resumes or overwrites the real session. Extra arguments go to clawsh.
#
# With Tailscale running, the server stays on localhost and `tailscale serve`
# does the rest. Without it, the server listens on every network so a phone
# on the same Wi-Fi can reach it.
set -euo pipefail
cd "$(dirname "$0")/.."

PORT="${CLAWSH_WEB_PORT:-7878}"
STATE=/tmp/clawsh-web-dev

(cd web && trunk build --release)

if tailscale status --json 2>/dev/null | grep -q '"BackendState": *"Running"'; then
  ADDR="127.0.0.1:$PORT"
  echo "Tailscale is up, so this stays on localhost. Scan the QR code from ctrl+b W."
else
  ADDR="0.0.0.0:$PORT"
  # The footer shows 0.0.0.0, which the phone can't use. This is the address it can.
  LAN_IP="$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for (i = 1; i < NF; i++) if ($i == "src") print $(i + 1)}')"
  echo "No Tailscale, so this listens on your network over plain HTTP."
  echo "On your phone, swap 0.0.0.0 for ${LAN_IP:-your LAN IP} in the link from ctrl+b W."
  echo "Anyone on this network with the link can drive Claude. Press ctrl+b W again to stop."
fi
sleep 2

exec env CLAWSH_WEB_ADDR="$ADDR" XDG_STATE_HOME="$STATE" \
  ./scripts/cargo.sh run --release -- --fresh "$@"
