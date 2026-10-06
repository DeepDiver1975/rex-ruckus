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
chmod +x "$stage/rex-ruckus.sh" "$stage/rex-ruckus"

tar -C "$root/dist" -czf "$root/dist/$name.tar.gz" "$name"
echo "$root/dist/$name.tar.gz"
