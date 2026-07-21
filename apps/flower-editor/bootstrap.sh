#!/usr/bin/env bash
#
# First-time setup for the flower editor app (and whenever the Rust *API* changes):
#   1. build the flower-ffi Rust lib on the host (so uniffi-bindgen can introspect it)
#   2. generate the UniFFI Swift binding + C module into packages/flower-swift/generated/
#      (what Package.swift compiles: generated/Sources/FlowerFFI + generated/headers)
#   3. run `xcodegen generate` to (re)create FlowerEditorApp.xcodeproj
#
# The Rust *staticlib* for mac/simulator/device is NOT built here — the Xcode
# project's pre-build script (see project.yml) does that on every build, so
# ordinary Rust edits need only ⌘R in Xcode. Re-run this script only after
# changing the Rust API surface (new/renamed FFI methods).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"        # repo root
OUT="$ROOT/packages/flower-swift/generated"

echo "▸ Building flower-ffi (host) for bindgen introspection…"
cargo build --manifest-path "$ROOT/Cargo.toml" -p flower-ffi

echo "▸ Generating UniFFI Swift binding + C module…"
rm -rf "$OUT" && mkdir -p "$OUT/Sources/FlowerFFI" "$OUT/headers" "$OUT/tmp"
cargo run -q --manifest-path "$ROOT/Cargo.toml" -p flower-ffi --bin uniffi-bindgen -- \
  generate --library "$ROOT/target/debug/libflower_ffi.dylib" \
  --language swift --out-dir "$OUT/tmp" 2>&1 | grep -vi swiftformat || true
mv "$OUT/tmp/flower_ffi.swift" "$OUT/Sources/FlowerFFI/flower_ffi.swift"
cp "$OUT/tmp/flower_ffiFFI.h" "$OUT/headers/"
cp "$OUT/tmp/flower_ffiFFI.modulemap" "$OUT/headers/module.modulemap"
rm -rf "$OUT/tmp"

echo "▸ Generating Xcode project…"
cd "$HERE" && xcodegen generate

echo "✓ Ready."
echo "  Run on macOS:"
echo "    xcodebuild -project $HERE/FlowerEditorApp.xcodeproj -scheme FlowerEditorApp \\"
echo "      -destination 'platform=macOS' -derivedDataPath build/DD build"
