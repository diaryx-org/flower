#!/usr/bin/env bash
#
# Build FlowerFFI.xcframework from crates/flower-ffi and generate the Swift
# bindings alongside it — the distributable artifact a macOS/iOS app links to
# drive flower-core. (For day-to-day dev the app rebuilds the staticlib itself via
# a pre-build step; see apps/flower-editor/project.yml. This script is for a
# prebuilt, shippable framework.)
#
# Output (under packages/flower-swift/generated/, git-ignored):
#   FlowerFFI.xcframework/    the static libs for every built slice + C headers
#   Sources/FlowerFFI/        the generated Swift (flower_ffi.swift)
#
# fig-sys ships a prebuilt static lib for macos-arm64 today; the other Apple
# slices (macos-x64, ios, ios-sim) build fig from source via Zig cross-compiling.
# By default this builds only the slices whose Rust target is installed AND whose
# fig backend links, so it works out of the box on an arm64 Mac. Add more targets
# to APPLE_TARGETS once their fig build is wired up.
#
# Usage: scripts/build-xcframework.sh [--debug]   (default: release)
set -euo pipefail

PROFILE="release"
CARGO_PROFILE_FLAG="--release"
if [[ "${1:-}" == "--debug" ]]; then
  PROFILE="debug"
  CARGO_PROFILE_FLAG=""
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/packages/flower-swift/generated"
LIB_BASENAME="libflower_ffi.a"
TARGET_DIR="$ROOT/target"

# The macOS slice. Extend to universal / iOS once the fig cross-build is wired:
#   MACOS_ARCHES=(aarch64-apple-darwin x86_64-apple-darwin)
MACOS_ARCHES=(aarch64-apple-darwin)

echo "▸ Building flower-ffi staticlib for macOS ($PROFILE)…"
for target in "${MACOS_ARCHES[@]}"; do
  echo "  · $target"
  rustup target add "$target" 2>/dev/null || true
  cargo build -p flower-ffi $CARGO_PROFILE_FLAG --target "$target"
done

echo "▸ Generating Swift bindings…"
rm -rf "$OUT"
mkdir -p "$OUT/Sources/FlowerFFI" "$OUT/headers"
LIB_FOR_GEN="$TARGET_DIR/${MACOS_ARCHES[0]}/$PROFILE/$LIB_BASENAME"
cargo run -q -p flower-ffi --bin uniffi-bindgen -- \
  generate --library "$LIB_FOR_GEN" --language swift --out-dir "$OUT/gen-tmp"

mv "$OUT/gen-tmp/flower_ffi.swift" "$OUT/Sources/FlowerFFI/flower_ffi.swift"
cp "$OUT/gen-tmp"/flower_ffiFFI.h "$OUT/headers/"
cp "$OUT/gen-tmp"/flower_ffiFFI.modulemap "$OUT/headers/module.modulemap"
rm -rf "$OUT/gen-tmp"

echo "▸ Fattening the macOS slice with lipo…"
mkdir -p "$OUT/lipo/macos"
lipo -create -output "$OUT/lipo/macos/$LIB_BASENAME" \
  $(printf "$TARGET_DIR/%s/$PROFILE/$LIB_BASENAME " "${MACOS_ARCHES[@]}")

echo "▸ Assembling xcframework…"
rm -rf "$OUT/FlowerFFI.xcframework"
xcodebuild -create-xcframework \
  -library "$OUT/lipo/macos/$LIB_BASENAME" -headers "$OUT/headers" \
  -output "$OUT/FlowerFFI.xcframework"

rm -rf "$OUT/lipo" "$OUT/headers"
echo "✓ Done:"
echo "    $OUT/FlowerFFI.xcframework"
echo "    $OUT/Sources/FlowerFFI/flower_ffi.swift"
