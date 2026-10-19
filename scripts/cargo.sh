#!/usr/bin/env bash
# libghostty-vt-sys builds Ghostty with `zig build`, and Ghostty needs Zig 0.15.x
# exactly. This puts a local 0.15 toolchain ahead of any system zig, then runs cargo.
set -euo pipefail
ZIG_DIR="${CLAWSH_ZIG_DIR:-$HOME/.local/share/zig-0.15.2}"
if [[ ! -x "$ZIG_DIR/zig" ]]; then
  echo "zig 0.15.x not found at $ZIG_DIR (set CLAWSH_ZIG_DIR)" >&2
  exit 1
fi
export PATH="$ZIG_DIR:$PATH"
exec cargo "$@"
