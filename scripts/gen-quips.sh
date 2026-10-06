#!/usr/bin/env bash
# Generates Rex's voice lines: assets/quips/<id>.ogg for every quip in assets/quips/quips.ron.
#
# Voice:   Piper en_US-norman-medium (rhasspy/piper-voices), trained from scratch on LibriVox
#          recordings, public domain. Downloaded on first use into $PIPER_VOICES
#          (default ~/.local/share/piper-voices). Never use a voice with a research-only licence.
# Needs:   piper (https://github.com/OHF-Voice/piper1-gpl), sox with Vorbis support, curl, and a
#          built rr-tools (cargo run -p rr-tools -- quips-list prints the `id<TAB>text` lines).
# Usage:   scripts/gen-quips.sh [--only ID] [--length-scale N]
#   Regenerates every file (idempotent); --only regenerates one. --length-scale (default 1.0)
#   below 1 speeds speech up, e.g. 0.9 to shorten a line that runs long.
# The sox chain after synthesis gives a gruff action-hero tone: 4 dB of headroom (gain -4),
# pitch down, overdrive, low and presence EQ, compression, peak normalisation, then 0.1 s of
# trailing silence. Mono, Vorbis quality 4.
# The voice files are pinned by sha256; a mismatch aborts the run.
set -euo pipefail

cd "$(dirname "$0")/.."
only=""
scale=1.0
while [ $# -gt 0 ]; do
    case $1 in
        --only) only=${2:?--only needs an id}; shift 2 ;;
        --length-scale) scale=${2:?--length-scale needs a number}; shift 2 ;;
        *) echo "usage: $0 [--only ID] [--length-scale N]" >&2; exit 2 ;;
    esac
done

voices=${PIPER_VOICES:-$HOME/.local/share/piper-voices}
name=en_US-norman-medium
base=https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/norman/medium
mkdir -p "$voices"
for f in "$name.onnx" "$name.onnx.json"; do
    [ -s "$voices/$f" ] || { curl -fL --retry 3 -o "$voices/$f.part" "$base/$f" && mv "$voices/$f.part" "$voices/$f"; }
done
(cd "$voices" && sha256sum -c --quiet - <<SUMS
b9739443232a80a59c7d18810dd856899bf16a7964725f5ab81ea49b1351cb71  $name.onnx
6c2db7f558a4a8deb9fe822583c1c5105f6c4e834dd0f9de8ad17a888ee9fe1d  $name.onnx.json
SUMS
) || { echo "gen-quips: voice files in $voices fail their sha256 check; delete them and retry" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p assets/quips

# Capture first: a failing quips-list must abort the script, not yield an empty loop.
list=$(cargo run -q -p rr-tools -- quips-list assets/quips/quips.ron)
[ -n "$list" ] || { echo "gen-quips: quips-list printed nothing" >&2; exit 1; }

for ogg in assets/quips/*.ogg; do
    [ -e "$ogg" ] || continue
    id=$(basename "$ogg" .ogg)
    cut -f1 <<<"$list" | grep -qx "$id" || echo "gen-quips: warning: stale $ogg has no quip id" >&2
done

found=0
while IFS=$'\t' read -r id text; do
    [ -n "$only" ] && [ "$only" != "$id" ] && continue
    found=1
    printf '%s\n' "$text" | piper -m "$voices/$name.onnx" -c "$voices/$name.onnx.json" \
        --length-scale "$scale" -f "$work/$id.wav" >/dev/null 2>"$work/piper.err" \
        || { cat "$work/piper.err" >&2; echo "gen-quips: piper failed for $id" >&2; exit 1; }
    sox --temp "$work" "$work/$id.wav" -c 1 -C 4 "assets/quips/$id.ogg" \
        gain -4 pitch -300 overdrive 6 equalizer 120 1q +3 equalizer 3000 1q +2 \
        compand 0.02,0.2 -60,-60,-30,-15,-20,-10,0,-5 -5 norm -1 pad 0 0.1
    echo "$id.ogg $(soxi -D "assets/quips/$id.ogg")s"
done <<<"$list"

if [ -n "$only" ] && [ "$found" = 0 ]; then
    echo "gen-quips: no quip with id '$only'" >&2
    exit 1
fi
