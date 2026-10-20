#!/usr/bin/env bash
# Publishes clawsh-tui to crates.io with a freshly built phone client inside.
# `web-dist` is gitignored, so Cargo.toml's `include` list is what packages it;
# building it here means the published crate never carries a stale or missing
# client. Extra arguments go to `cargo publish` (try --dry-run first).
set -euo pipefail
cd "$(dirname "$0")/.."

rm -rf web-dist
(cd web && trunk build --release)
test -f web-dist/index.html

exec ./scripts/cargo.sh publish "$@"
