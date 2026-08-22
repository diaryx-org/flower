#!/usr/bin/env bash
#
# Build FlowerFFI.xcframework from crates/flower-ffi — the distributable
# prebuilt binary for a consumer who doesn't build Rust. (For day-to-day dev the
# app rebuilds the staticlib itself via a pre-build step; see
# apps/flower-editor/project.yml.) The Swift package itself no longer involves
# it: the committed binding under packages/flower-swift/uniffi-generated/ (kept
# fresh here via gen-bindings.sh) is what the root Package.swift compiles.
#
# Output (under target/xcframework/, git-ignored):
#   FlowerFFI.xcframework/    the static libs for every built slice + C headers
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
OUT="$ROOT/target/xcframework"
GEN="$ROOT/packages/flower-swift/uniffi-generated"
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

# Refresh the committed binding so the xcframework's headers and the package's
# Swift are one generator run — a drift here is what CI's `bindings` job catches.
"$ROOT/scripts/gen-bindings.sh"
rm -rf "$OUT"
mkdir -p "$OUT"

echo "▸ Fattening the macOS slice with lipo…"
mkdir -p "$OUT/lipo/macos"
lipo -create -output "$OUT/lipo/macos/$LIB_BASENAME" \
  $(printf "$TARGET_DIR/%s/$PROFILE/$LIB_BASENAME " "${MACOS_ARCHES[@]}")

echo "▸ Assembling xcframework…"
rm -rf "$OUT/FlowerFFI.xcframework"
xcodebuild -create-xcframework \
  -library "$OUT/lipo/macos/$LIB_BASENAME" -headers "$GEN/headers" \
  -output "$OUT/FlowerFFI.xcframework"

rm -rf "$OUT/lipo"
echo "✓ Done:"
echo "    $OUT/FlowerFFI.xcframework"
echo "  The Swift package (root Package.swift) is consumed separately; the"
echo "  xcframework is only for consumers who don't build the Rust staticlib."
