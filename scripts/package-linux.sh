#!/usr/bin/env bash
# Packages target/release/rex-ruckus into dist/rex-ruckus-VERSION-linux-x86_64.tar.gz.
# Builds nothing: run `cargo build -p rr-game --release` first.
set -euo pipefail

version=${1:?usage: package-linux.sh VERSION}
root=$(cd "$(dirname "$0")/.." && pwd)
name="rex-ruckus-$version-linux-x86_64"
stage="$root/dist/$name"

[ -x "$root/target/release/rex-ruckus" ] || {
    echo "target/release/rex-ruckus not found; build it first" >&2
    exit 1
}

rm -rf "$stage" "$root/dist/$name.tar.gz"
mkdir -p "$stage"
cp "$root/target/release/rex-ruckus" "$stage/"
cp -r "$root/assets" "$stage/assets"
cp "$root/LICENSE" "$root/CREDITS.md" "$root/README.md" "$stage/"
cat > "$stage/rex-ruckus.sh" <<'LAUNCHER'
#!/bin/sh
# Runs the game from wherever this folder was unpacked.
here=$(dirname "$(readlink -f "$0")")
exec "$here/rex-ruckus" "$@"
LAUNCHER
cp "$root/assets/icon/rex-ruckus-256.png" "$stage/rex-ruckus.png"
cat > "$stage/rex-ruckus.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Rex Ruckus: Meltdown
Comment=Fast 90s-style 3D shooter
Exec=rex-ruckus.sh
Icon=rex-ruckus
Terminal=false
Categories=Game;ActionGame;
StartupWMClass=rex-ruckus
DESKTOP
cat > "$stage/install-desktop-entry.sh" <<'INSTALL'
#!/bin/sh
# Adds Rex Ruckus to your desktop's application menu, pointing at this folder.
# Rerun it after moving the folder; delete the two files it names to undo.
set -eu
here=$(dirname "$(readlink -f "$0")")
apps=${XDG_DATA_HOME:-$HOME/.local/share}/applications
icons=${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps
mkdir -p "$apps" "$icons"
cp "$here/rex-ruckus.png" "$icons/rex-ruckus.png"
sed "s|^Exec=.*|Exec=\"$here/rex-ruckus.sh\"|" "$here/rex-ruckus.desktop" > "$apps/rex-ruckus.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$apps" 2>/dev/null || true
echo "installed $apps/rex-ruckus.desktop and $icons/rex-ruckus.png"
INSTALL
chmod +x "$stage/rex-ruckus.sh" "$stage/rex-ruckus" "$stage/install-desktop-entry.sh"

tar -C "$root/dist" -czf "$root/dist/$name.tar.gz" "$name"
echo "$root/dist/$name.tar.gz"
