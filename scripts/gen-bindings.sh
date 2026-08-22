#!/usr/bin/env bash
#
# (Re)generate the UniFFI Swift binding committed under
# packages/flower-swift/uniffi-generated/ — or, with --check, verify that the
# committed binding is byte-for-byte what crates/flower-ffi produces.
#
# The binding is committed rather than built on demand because the Swift package
# is consumed by version from a bare git checkout (SwiftPM runs no generators),
# so a clone must build as-is. CI's `bindings` job runs the --check on every
# push; after changing the FFI surface, run this and commit what it writes.
#
# Usage: scripts/gen-bindings.sh [--check]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/packages/flower-swift/uniffi-generated"

echo "▸ Building flower-ffi (host) for bindgen introspection…"
cargo build --manifest-path "$ROOT/Cargo.toml" -p flower-ffi

# The host cdylib the generator introspects: .dylib on macOS, .so on Linux. The
# generated Swift is identical either way — it comes from the embedded UniFFI
# metadata, not from the machine code — which is what lets CI check on Linux.
LIB="$ROOT/target/debug/libflower_ffi.dylib"
[ -f "$LIB" ] || LIB="$ROOT/target/debug/libflower_ffi.so"
[ -f "$LIB" ] || { echo "gen-bindings: no libflower_ffi.{dylib,so} under target/debug" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

echo "▸ Generating UniFFI Swift binding + C module…"
mkdir -p "$STAGE/out/Sources/FlowerFFI" "$STAGE/out/headers"
# --no-format: the committed output must not depend on whether this machine
# happens to have swiftformat on PATH — CI's runner never does.
cargo run -q --manifest-path "$ROOT/Cargo.toml" -p flower-ffi --bin uniffi-bindgen -- \
  generate --library "$LIB" --language swift --no-format --out-dir "$STAGE/gen"
mv "$STAGE/gen/flower_ffi.swift" "$STAGE/out/Sources/FlowerFFI/flower_ffi.swift"
cp "$STAGE/gen/flower_ffiFFI.h" "$STAGE/out/headers/"
cp "$STAGE/gen/flower_ffiFFI.modulemap" "$STAGE/out/headers/module.modulemap"

if [ "${1:-}" = "--check" ]; then
  if diff -ru "$OUT" "$STAGE/out"; then
    echo "✓ committed binding matches crates/flower-ffi"
  else
    echo "✗ $OUT is stale — run scripts/gen-bindings.sh and commit the result" >&2
    exit 1
  fi
else
  rm -rf "$OUT"
  cp -R "$STAGE/out" "$OUT"
  echo "✓ regenerated $OUT"
fi
