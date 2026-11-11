#!/usr/bin/env bash
# Install a development build of Flower for this user, on Linux: the binary,
# the sample it opens on, its icon and its desktop entry.
#
#     scripts/install-linux.sh              # build (debug), then install
#     scripts/install-linux.sh --release    # the release profile
#     scripts/install-linux.sh --no-build   # install what target/ already holds
#
# What goes where, under $PREFIX (default ~/.local):
#
#   share/flower/Flower            the binary, stripped
#   share/flower/resources/        Sample.md and its pictures, side by side,
#                                  where the app looks for them: beside itself
#   bin/flower-app                 a link to the binary. Not `flower`, which
#                                  is the terminal data editor's name
#   share/icons/hicolor/scalable/apps/org.diaryx.flower.svg     the leaf
#   share/applications/org.diaryx.flower.desktop
#
# The desktop entry is named for the app id every window carries
# (`platform::APP_ID` in apps/flower), which is how a dock finds the icon for
# a window. Run again to update: every file is replaced.
#
# The build goes through scripts/cargo.sh: corgi where it is installed.
set -euo pipefail

cd "$(dirname "$0")/.."

profile=debug
build=1
for arg in "$@"; do
    case $arg in
    --release) profile=release ;;
    --no-build) build=0 ;;
    -h | --help)
        sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'
        exit 0
        ;;
    *)
        echo "install-linux.sh: unknown argument $arg" >&2
        exit 2
        ;;
    esac
done

prefix=${PREFIX:-$HOME/.local}
id=org.diaryx.flower

if ((build)); then
    flags=()
    [[ $profile == release ]] && flags+=(--release)
    echo "▸ Building Flower ($profile)…"
    scripts/cargo.sh build -p flower-app "${flags[@]}"
fi

binary=target/$profile/Flower
[[ -x $binary ]] || {
    echo "install-linux.sh: no $binary; build first, or drop --no-build" >&2
    exit 1
}

home=$prefix/share/flower
echo "▸ Installing to $prefix…"
rm -rf "$home/resources"
install -d "$home/resources" "$prefix/bin" \
    "$prefix/share/icons/hicolor/scalable/apps" "$prefix/share/applications"
# To a new name and renamed over, so a Flower that is running keeps its file.
install -s -m755 "$binary" "$home/Flower.new"
mv -f "$home/Flower.new" "$home/Flower"
install -m644 apps/flower/resources/Sample.md apps/flower/resources/Media/* "$home/resources/"
ln -sfn "$home/Flower" "$prefix/bin/flower-app"
install -m644 apps/flower/resources/leaf.icon/Assets/leaf.svg \
    "$prefix/share/icons/hicolor/scalable/apps/$id.svg"

cat >"$prefix/share/applications/$id.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Flower
GenericName=Document and Data Editor
Comment=Edit Markdown, Djot and HTML documents, and JSON, YAML and TOML data
Exec=$prefix/bin/flower-app %F
Icon=$id
Terminal=false
Categories=Office;TextEditor;
Keywords=markdown;djot;html;json;yaml;toml;frontmatter;editor;
MimeType=text/markdown;text/x-markdown;application/json;application/yaml;application/x-yaml;application/toml;
StartupWMClass=$id
DESKTOP

# The desktop's indexes, where it keeps them.
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q "$prefix/share/applications" || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1 \
    && [[ -f $prefix/share/icons/hicolor/index.theme ]]; then
    gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" || true
fi

echo "✓ Flower $profile build installed: $prefix/bin/flower-app"
case ":$PATH:" in
*":$prefix/bin:"*) ;;
*) echo "  ($prefix/bin is not on PATH)" ;;
esac
