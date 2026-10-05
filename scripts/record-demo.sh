#!/usr/bin/env bash
# Records the README demo: replays assets/demo/arsenal_depot.ron, saves every frame, then
# encodes docs/media/demo.mp4 and docs/media/demo.gif. Needs a display and ffmpeg.
# Usage: scripts/record-demo.sh [SCRIPT] [LEVEL]
set -euo pipefail

cd "$(dirname "$0")/.."
script=${1:-assets/demo/arsenal_depot.ron}
level=${2:-arsenal_depot.ron}
out=docs/media
# The first frames are black while the renderer warms up.
skip=3

frames=$(mktemp -d)
trap 'rm -rf "$frames"' EXIT

cargo run -p rr-game --release -- "$level" --demo "$script" --record "$frames"

mkdir -p "$out"
ffmpeg -loglevel error -y -framerate 30 -start_number "$skip" -i "$frames/frame_%05d.png" \
    -c:v libx264 -preset slow -crf 26 -pix_fmt yuv420p -movflags +faststart "$out/demo.mp4"
# GitHub renders GIFs inline but not videos from the repo; keep this one under ~8 MB.
ffmpeg -loglevel error -y -i "$out/demo.mp4" \
    -vf "fps=12,scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=64:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
    "$out/demo.gif"
ls -lh "$out/demo.mp4" "$out/demo.gif"
