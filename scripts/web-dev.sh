#!/usr/bin/env bash
# Runs a throwaway Claw$h for testing phone access. It builds the web client,
# then starts a release build with its own session file and one fresh tab, so
# it never resumes or overwrites the real session. Extra arguments go to clawsh.
#
# Phone access only works through Tailscale, so this checks for it first.
set -euo pipefail
cd "$(dirname "$0")/.."

STATE=/tmp/clawsh-web-dev

if ! tailscale status --json 2>/dev/null | grep -q '"BackendState": *"Running"'; then
  echo "Tailscale isn't running. Phone access needs it:" >&2
  echo "  sudo systemctl enable --now tailscaled && sudo tailscale up" >&2
  exit 1
fi

(cd web && trunk build --release)
echo "Press ctrl+b W and scan the QR code. The phone needs Tailscale on, signed into the same tailnet."
sleep 2

exec env XDG_STATE_HOME="$STATE" ./scripts/cargo.sh run --release -- --fresh "$@"
