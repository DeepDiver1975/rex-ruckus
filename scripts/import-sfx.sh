#!/usr/bin/env bash
# Imports a third-party (CC0) recording as a game sound: mono OGG Vorbis at the source sample
# rate, quality 4, leading silence trimmed, peak normalised to -1 dBFS.
#
# Usage: scripts/import-sfx.sh SRC DEST [sox effects...]
#   SRC   any format sox (or ffmpeg) reads: wav, ogg, flac, mp3, ...
#   DEST  the .ogg to write, e.g. assets/sounds/cc0/door_start.ogg
#   The optional sox effects run after the trim and before the normalisation, so an enemy kind
#   can reuse a recording with its own character, e.g. `pitch -300 overdrive 6` or `speed 1.2`.
#
# Environment: CHANNELS=2 keeps two channels (music); NO_TRIM=1 keeps leading silence (a track
# that already loops); LOOP="START LEN" makes a seamless loop (needs sox): it cuts LEN seconds
# from START (after the effects) and crossfades the 0.25 s that follow the cut into its head,
# so the end runs straight into the start. LOOP implies NO_TRIM.
# Needs sox with Vorbis support. Without it, falls back to ffmpeg, which takes no extra effects
# and normalises loudness (true peak -1 dB) instead of the peak.
set -euo pipefail

if [ $# -lt 2 ]; then
    sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
fi

src=$1
dest=$2
shift 2
channels=${CHANNELS:-1}
mkdir -p "$(dirname "$dest")"

# Leading silence below -50 dBFS is cut; everything after the first sound is kept. The effects
# get 6 dB of headroom so they never clip; `norm` restores the level.
trim=(silence 1 0.002 -50d)
[ "${NO_TRIM:-0}" = 1 ] && trim=()

if [ -n "${LOOP:-}" ]; then
    read -r start len <<<"$LOOP"
    xf=0.25
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    sox "$src" -c "$channels" -e floating-point "$work/seg.wav" gain -6 "$@" \
        trim "$start" "$(awk "BEGIN { print $len + $xf }")"
    # Head fades in while the audio just past the cut fades out over it.
    sox "$work/seg.wav" "$work/head.wav" trim 0 "$xf" fade q "$xf"
    sox "$work/seg.wav" "$work/tail.wav" trim "$len" "$xf" fade q 0 "$xf" "$xf"
    sox "$work/seg.wav" "$work/body.wav" trim "$xf" "$(awk "BEGIN { print $len - $xf }")"
    sox -m -v 1 "$work/head.wav" -v 1 "$work/tail.wav" "$work/xfade.wav"
    sox "$work/xfade.wav" "$work/body.wav" "$work/loop.wav"
    src=$work/loop.wav
    trim=()
    set --
fi

if sox -h 2>/dev/null | grep -qiw vorbis; then
    sox --temp "${TMPDIR:-/tmp}" "$src" -c "$channels" -C 4 "$dest" ${trim[@]+"${trim[@]}"} gain -6 "$@" norm -1
else
    if [ $# -gt 0 ]; then
        echo "import-sfx: sox lacks Vorbis support; ffmpeg fallback cannot apply: $*" >&2
        exit 1
    fi
    filters="loudnorm=I=-16:TP=-1"
    [ "${NO_TRIM:-0}" = 1 ] || filters="silenceremove=start_periods=1:start_threshold=-50dB,$filters"
    ffmpeg -loglevel error -y -i "$src" -ac "$channels" -af "$filters" -c:a libvorbis -q:a 4 "$dest"
fi
