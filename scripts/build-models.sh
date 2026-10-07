#!/usr/bin/env bash
# Builds the original, script-made models (scripts/models/*.py) into assets/models/ with headless
# Blender. The outputs are committed; rerun this after changing a model script.
#
# Usage: scripts/build-models.sh [--preview DIR] [MODEL...]
#   MODEL is a script name without .py (default: all of them).
#   --preview DIR also writes turnaround and per-clip contact sheets (PNG) into DIR.
#
# Built with Blender 5.0.1. A rebuild reproduces the committed files byte for byte with the same
# Blender version, not across versions.
# BLENDER overrides the binary. It runs with a minimal PATH because Blender's embedded Python
# finds its standard library through PATH, and a user-installed python3.x (pyenv, uv) would
# shadow the one Blender was built against.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
blender="$(command -v "${BLENDER:-blender}")" || { echo "blender is needed (apt install blender)" >&2; exit 1; }

# Model script -> output path under assets/.
declare -A out=(
    [goon]=models/enemies/grunt.glb
    [bruiser]=models/enemies/enforcer.glb
    [gnasher]=models/enemies/slasher.glb
    [peeper]=models/enemies/drone.glb
    [warlord]=models/enemies/boss.glb
    [hand_cannon]=models/weapons/hand_cannon.glb
)

preview=()
models=()
while [ $# -gt 0 ]; do
    case "$1" in
        --preview) preview=(--preview "$(realpath -m "$2")"); shift 2 ;;
        *) models+=("$1"); shift ;;
    esac
done
[ ${#models[@]} -gt 0 ] || models=("${!out[@]}")

for m in "${models[@]}"; do
    [ -n "${out[$m]:-}" ] || { echo "unknown model: $m" >&2; exit 1; }
    echo "building $m -> assets/${out[$m]}"
    log="$(mktemp)"
    if ! PATH="$(dirname "$blender"):/usr/bin:/bin" "$blender" -b --factory-startup \
        --python-exit-code 1 --python "$root/scripts/models/$m.py" \
        -- "$root/assets/${out[$m]}" "${preview[@]}" >"$log" 2>&1; then
        cat "$log" >&2
        rm -f "$log"
        echo "$m: blender failed" >&2
        exit 1
    fi
    grep '^rrkit' "$log" || true
    rm -f "$log"
done
