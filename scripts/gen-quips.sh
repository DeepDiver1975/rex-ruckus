#!/usr/bin/env bash
# Generates Rex's voice lines: assets/quips/<id>.ogg for every quip in assets/quips/quips.ron.
#
# Voice:   Kokoro-82M (hexgrad/Kokoro-82M, Apache-2.0 weights), voice am_onyx, rendered by
#          scripts/quips_kokoro.py. The model revision is pinned and the weights and voice are
#          checked by sha256. See CREDITS.md for the model's training-data notes.
# Needs:   Python 3.10-3.12 ($QUIPS_PYTHON, default python3), sox with Vorbis support, espeak-ng
#          (system library preferred; see ESPEAK_LIB/ESPEAK_DATA in quips_kokoro.py) and a built
#          rr-tools (cargo run -p rr-tools -- quips-list prints the `id<TAB>text` lines).
#          The pinned packages (scripts/requirements-quips.txt, ~1.5 GB with CPU torch) are
#          installed on first use into $QUIPS_VENV (default target/quips-venv); the model goes
#          to $HF_HOME (default target/hf).
# Usage:   scripts/gen-quips.sh [--only ID] [--speed N]
#   Regenerates every file; --only regenerates one. --speed (default 0.95) above 1 speaks
#   faster, e.g. 1.05 to shorten a line that runs long.
# Kokoro's output varies slightly between runs, so the committed OGGs are the source of truth.
# The sox step only normalises the peak to -1 dBFS and adds 0.1 s of trailing silence.
# Mono, Vorbis quality 4.
set -euo pipefail

cd "$(dirname "$0")/.."
only=""
speed=0.95
while [ $# -gt 0 ]; do
    case $1 in
        --only) only=${2:?--only needs an id}; shift 2 ;;
        --speed) speed=${2:?--speed needs a number}; shift 2 ;;
        *) echo "usage: $0 [--only ID] [--speed N]" >&2; exit 2 ;;
    esac
done

venv=${QUIPS_VENV:-target/quips-venv}
export HF_HOME=${HF_HOME:-$PWD/target/hf}
if [ ! -x "$venv/bin/python" ]; then
    "${QUIPS_PYTHON:-python3}" -m venv "$venv"
    "$venv/bin/pip" install -q --upgrade pip
    "$venv/bin/pip" install -q -r scripts/requirements-quips.txt
fi

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

if [ -n "$only" ]; then
    list=$(awk -F'\t' -v id="$only" '$1 == id' <<<"$list")
    [ -n "$list" ] || { echo "gen-quips: no quip with id '$only'" >&2; exit 1; }
fi

# One Python run renders every line, so the model loads once.
"$venv/bin/python" scripts/quips_kokoro.py "$work" "$speed" <<<"$list"

while IFS=$'\t' read -r id _; do
    sox --temp "$work" "$work/$id.wav" -c 1 -C 4 "assets/quips/$id.ogg" norm -1 pad 0 0.1
    echo "$id.ogg $(soxi -D "assets/quips/$id.ogg")s"
done <<<"$list"
