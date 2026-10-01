#!/usr/bin/env bash
#
# Print the Homebrew cask for Flower.app — `Casks/flower-editor.rb` in
# diaryx-org/homebrew-tap — for one release:
#
#   scripts/render-cask.sh <version> <sha256 of Flower-<version>-aarch64.dmg>
#
# mac-app.yml runs this on a release tag and pushes the result to the tap; the
# tap is written by CI and by nothing else. It is a script rather than a
# heredoc in the workflow so that the cask can be rendered and `brew audit`ed
# on a Mac before a tag depends on it.
#
# `flower-editor`, not `flower`: `flower` is the tap's formula for the TUI
# (homebrew.yml), as Leaf's app is `leaf-editor` beside the `leaf` formula.
#
# The app is sandboxed, so what it keeps lives in its container, not in
# ~/Library/Preferences or ~/Library/Caches.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $0 <version> <sha256>" >&2
  exit 2
fi
version="$1"
sha256="$2"

cat <<RUBY
cask "flower-editor" do
  version "$version"
  sha256 "$sha256"

  url "https://github.com/diaryx-org/flower/releases/download/v#{version}/Flower-#{version}-aarch64.dmg"
  name "Flower"
  desc "Structural editor for JSON, YAML, TOML and fig config files"
  homepage "https://github.com/diaryx-org/flower"

  depends_on arch: :arm64
  depends_on macos: :ventura

  app "Flower.app"

  zap trash: [
    "~/Library/Application Scripts/org.diaryx.flower",
    "~/Library/Containers/org.diaryx.flower",
  ]
end
RUBY
