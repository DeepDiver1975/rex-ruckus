#!/usr/bin/env bash
# Downloads the third-party (CC0) glTF models into assets/models/, unmodified, and checks each
# file against its pinned sha256. Every row has a matching entry in CREDITS.md.
#
# Usage: scripts/import-models.sh [ASSETS_DIR]   (default: assets)
#   Re-running is safe: a file that already matches its checksum is skipped.
#
# Sources are poly.pizza model pages (https://poly.pizza/m/ID, licence checked per model: CC0 1.0);
# the file itself is served from https://static.poly.pizza/UUID.glb.
# The jetpack is only published as .blend (OpenGameArt, CC0): its pinned source is converted with
# Blender by scripts/blend_to_glb.py. That step runs only when the .glb is missing or FORCE=1, and
# needs `blender` on PATH (the export is not byte-reproducible across Blender versions).
# Needs curl and sha256sum.
set -euo pipefail

assets=${1:-assets}
out="$assets/models"

# dest (under assets/models)   poly.pizza id   static file   sha256
manifest=$(cat <<'EOF'
enemies/barrel.glb           1orHe0kCc1 dbb58868-50d4-4ef0-945a-12b07222fb69.glb 730a8da4cb342533ddc20fdb8f52a47f3befd20d0bafa44b668eac066252f2f3
weapons/pistol.glb           J3i9KDQ3kt f5a88c73-af97-49ca-8650-4bde579d2f80.glb 4622ab2909aa0f4e88b74a13f52f9e28183a6ff5fca5896fc7e98d44008f2148
weapons/shotgun.glb          29FXKu7G91 9a6ee0ee-068b-4774-8b0f-679c3cef0b6e.glb b5d9d6ef843eea412e6422cbed55ac5325e5bd1f8a688c702d51f5cd88dea190
weapons/chaingun.glb         Bgvuu4CUMV 9a0e478c-de82-4773-9b70-a0219bb0057c.glb a6b90047927c65d0026bb19b5d4625b5ed3270e4ea5e39619cc50f0770aa6259
weapons/rocket_launcher.glb  eJNzLpBsEt 613e3b1b-d07c-496b-94a1-7c85b507bac4.glb 812aef31db0dd6920be57f39513a28f59ee4e109c76f4ab3a50ba9a9398023a0
weapons/pipe_bomb.glb        YWhHlmKOtx 03fa7f5b-4df5-45d6-86fb-87e8590f28d7.glb 1eb6c9f693a0519d8366dc11d39f6d6812b43ec59ababbb1d22bc53a0890700c
weapons/detonator.glb        TPqvwkyWdV 7ed34c76-5ef7-4b70-b7d8-ce06e8128b07.glb 9e5c7934f44ee538446e6f54cbe9c09d4e49a7e78ee2e7f16d4cc7483c43e710
pickups/pistol_ammo.glb      1dh0EFL5gl 11ede42b-11d5-445f-a145-9c9f89ae9b3f.glb 3428e5339fe362eebf463f12ec7321951e4972e501fc953c02e4dd74c3e77b52
pickups/shotgun_shells.glb   btpe2U9ODn 64d1eaa5-1905-4539-8985-f9bac5e3866f.glb aaf26d935ee23787e61017a94c22f4a50ae7a8812425cd1311860d9d3b656561
pickups/rockets.glb          BPdc4pA3tv a822b178-9faa-4303-8813-4e99c6802076.glb 41be3510e06a03a53c9395138a7eaf64cf1b234a6b4dc4e7f7fec14cf2727c58
pickups/health_small.glb     cPLNFLykkf 0b1e0ceb-8b34-4f23-b92c-52dfecfd5f00.glb 65446f5a8a9f84e265eb71323a0bb94965fde51c1dd0d53f882d13aab922360b
pickups/medkit.glb           Hp80p6148W 41249676-0965-40df-8dd7-eee79dd9e6cf.glb 6d665861ec5f79224ed1c99e5eb685015e053b3f73b1216def773a1afa9fd816
pickups/armour.glb           TMUoxILh9w 60ccfcdb-6aa7-4caf-a688-9b16a2a5f300.glb f584937911ef122d874fe463912f534cdb153e63fabb05fc0c10d2f629216af0
pickups/keycard.glb          EDvCEBvs8k 1dd9cceb-d8ce-48bc-b747-fc0420ae58d3.glb 51ed8a7045f018bf1a45fb57e07bfa6d5ce98087bfd0f58eb679ce906a6a36bc
pickups/atom.glb             1xtAc12dmv 6a161332-261b-480c-aec2-8464ae78055f.glb 6ef9dd5963101a757174865cf49a3e6f5c5e3f9db23db238d7045983aa4d3b8a
pickups/night_vision.glb     PLmfHOiB08 2c2a1082-c325-47c0-bf23-93b0effa6392.glb 515918afbab1162f9b2fb2f88bdd44e74be7113a4a8df14cceebfac045a0d691
props/toilet.glb             ZGtHqPsLv2 68e03d0b-fd3e-4954-9852-8b3cf63c536d.glb 4311471a6685e182f1c7643de16d7e3d32e3beed8182887e5806cfb4e2daf6e1
EOF
)

sum() { sha256sum "$1" | cut -d' ' -f1; }

while read -r dest id file sha; do
    [ -z "$dest" ] && continue
    path="$out/$dest"
    if [ -f "$path" ] && [ "$(sum "$path")" = "$sha" ]; then
        continue
    fi
    mkdir -p "$(dirname "$path")"
    echo "fetching $dest (https://poly.pizza/m/$id)"
    curl -fsSL "https://static.poly.pizza/$file" -o "$path.part"
    got=$(sum "$path.part")
    if [ "$got" != "$sha" ]; then
        rm -f "$path.part"
        echo "checksum mismatch for $dest: got $got, want $sha" >&2
        exit 1
    fi
    mv "$path.part" "$path"
done <<<"$manifest"

# Jetpack: OpenGameArt "Lowpoly Jetpack" by snabisch, .blend only.
jetpack="$out/pickups/jetpack.glb"
blend_url=https://opengameart.org/sites/default/files/Jetpacks.blend
blend_sha=da8855ef32deff0310bcc43a54fc178591f59e8417a6204b2807b98538ee3d5f
if [ ! -f "$jetpack" ] || [ "${FORCE:-0}" = 1 ]; then
    command -v blender >/dev/null || { echo "blender is needed to convert $blend_url" >&2; exit 1; }
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    echo "fetching pickups/jetpack.glb (https://opengameart.org/content/lowpoly-jetpack)"
    curl -fsSL "$blend_url" -o "$work/Jetpacks.blend"
    got=$(sum "$work/Jetpacks.blend")
    if [ "$got" != "$blend_sha" ]; then
        echo "checksum mismatch for Jetpacks.blend: got $got, want $blend_sha" >&2
        exit 1
    fi
    mkdir -p "$(dirname "$jetpack")"
    blender -b "$work/Jetpacks.blend" --python "$(dirname "$0")/blend_to_glb.py" \
        -- "Collection 1" "$(realpath -m "$jetpack")" >/dev/null
    [ -f "$jetpack" ] || { echo "blender did not write $jetpack" >&2; exit 1; }
fi
echo "models up to date in $out"
