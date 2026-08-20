#!/usr/bin/env bash
#
# Type-check the Swift renderers against the real generated FlowerFFI binding,
# without an Xcode project — the Swift peer of `cargo check`. Builds the host
# dylib, generates the UniFFI Swift, emits a FlowerFFI .swiftmodule, then
# `-typecheck`s the two source targets against it. macOS only.
#
# FlowerPagesUI is checked *first and alone*, with neither the binding nor its
# headers on the search path. That ordering is the test: the page editor is
# meant to have no FFI dependency, and the only way to keep it that way is to
# compile it somewhere an FFI import would not resolve.
#
# Usage: scripts/check-swift.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="${TMPDIR:-/tmp}/flower-swift-check"
SDK="$(xcrun --show-sdk-path)"
# The floor Package.swift promises. Without it swiftc assumes the *host* OS, and
# the check disagrees with the build: an API deprecated after our floor warns
# here and not there, and one introduced after it compiles here and not there.
TARGET="arm64-apple-macosx13.0"

echo "▸ Building flower-ffi (host) + generating Swift binding…"
cargo build -p flower-ffi --manifest-path "$ROOT/Cargo.toml" >/dev/null
DYLIB="$ROOT/target/debug/libflower_ffi.dylib"

rm -rf "$WORK" && mkdir -p "$WORK/headers" "$WORK/gen"
cargo run -q -p flower-ffi --manifest-path "$ROOT/Cargo.toml" --bin uniffi-bindgen -- \
  generate --library "$DYLIB" --language swift --out-dir "$WORK/gen" 2>&1 \
  | grep -vi swiftformat || true

cp "$WORK/gen/flower_ffiFFI.h" "$WORK/headers/"
cp "$WORK/gen/flower_ffiFFI.modulemap" "$WORK/headers/module.modulemap"

echo "▸ Emitting FlowerFFI.swiftmodule…"
swiftc -emit-module -module-name FlowerFFI \
  -emit-module-path "$WORK/FlowerFFI.swiftmodule" \
  "$WORK/gen/flower_ffi.swift" \
  -sdk "$SDK" -target "$TARGET" \
  -I "$WORK/headers" -Xcc -fmodule-map-file="$WORK/headers/module.modulemap"

echo "▸ Type-checking FlowerPagesUI (no binding in scope)…"
swiftc -emit-module -module-name FlowerPagesUI \
  -emit-module-path "$WORK/FlowerPagesUI.swiftmodule" \
  "$ROOT"/packages/flower-swift/Sources/FlowerPagesUI/*.swift \
  -sdk "$SDK" -target "$TARGET"
echo "  ✓ FFI-free"

echo "▸ Type-checking FlowerUI (macOS)…"
swiftc -typecheck -module-name FlowerUI \
  "$ROOT"/packages/flower-swift/Sources/FlowerUI/*.swift \
  -sdk "$SDK" -target "$TARGET" \
  -I "$WORK" \
  -I "$WORK/headers" -Xcc -fmodule-map-file="$WORK/headers/module.modulemap"
echo "  ✓ macOS"

echo "✓ Both targets type-check, and the page editor does so without a binding."
