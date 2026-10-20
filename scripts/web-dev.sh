#!/usr/bin/env bash
# Runs a throwaway Claw$h for testing phone access from a phone on the same
# Wi-Fi. It builds the web client, then starts a release build with its own
# session file and one fresh tab, so it never resumes or overwrites the real
# session. Extra arguments go to clawsh.
set -euo pipefail
cd "$(dirname "$0")/.."

PORT="${CLAWSH_WEB_PORT:-7878}"
STATE=/tmp/clawsh-web-dev

(cd web && trunk build --release)

# The footer shows 0.0.0.0, which the phone can't use. This is the address it can.
LAN_IP="$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for (i = 1; i < NF; i++) if ($i == "src") print $(i + 1)}')"
echo "On your phone, swap 0.0.0.0 for ${LAN_IP:-your LAN IP} in the link from ctrl+b W."
echo "Anyone on this network with the link can drive Claude. Press ctrl+b W again to stop."
sleep 2

exec env CLAWSH_WEB_ADDR="0.0.0.0:$PORT" XDG_STATE_HOME="$STATE" \
  ./scripts/cargo.sh run --release -- --fresh "$@"
