#!/usr/bin/env bash
#
# Run the FlowerUI renderer unit tests (macOS host). The peer of `check-swift.sh`,
# but this compiles and *runs* an XCTest bundle rather than just type-checking.
#
# The tests build `RowView`/`DocView` fixtures in pure Swift, so they need no Rust
# runtime — but the `FlowerFFI` module they import still references the FFI
# symbols, so the test binary must link the Rust staticlib. We force-load it (the
# same way the app does in apps/flower-editor/project.yml).
#
# Usage: scripts/test-swift.sh [extra `swift test` args…]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "▸ Building flower-ffi (host) staticlib…"
cargo build -p flower-ffi --manifest-path "$ROOT/Cargo.toml" >/dev/null
STATIC="$ROOT/target/debug/libflower_ffi.a"
[ -f "$STATIC" ] || { echo "missing $STATIC"; exit 1; }

echo "▸ swift test (FlowerUI)…"
# The package manifest lives at the repo root (see Package.swift).
swift test --package-path "$ROOT" \
  -Xlinker -force_load -Xlinker "$STATIC" "$@"
