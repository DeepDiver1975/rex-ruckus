#!/usr/bin/env bash
# Renders the app icon and the installer wizard images from assets/icon/*.svg.
# Outputs are committed; rerun this after editing the SVGs (never edit the outputs).
#
#   scripts/build-icons.sh [--preview DIR]
#
# Needs rsvg-convert (librsvg), icotool (icoutils) and ImageMagick 7 (magick).
# Writes:
#   assets/icon/rex-ruckus-256.png      window icon (embedded in the exe) and Linux icon
#   installer/rex-ruckus.ico            exe, setup and shortcut icon (16-256 px)
#   installer/wizard/large-*.png        Inno Setup WizardImageFile, one per DPI scale
#   installer/wizard/small-*.png        Inno Setup WizardSmallImageFile, one per DPI scale
# --preview DIR also writes DIR/icon-sheet.png and DIR/wizard-sheet.png for review.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
preview=""
if [ "${1:-}" = "--preview" ]; then
    preview=${2:?--preview needs a directory}
    mkdir -p "$preview"
fi
for tool in rsvg-convert icotool magick; do
    command -v "$tool" >/dev/null || { echo "build-icons: $tool not found" >&2; exit 1; }
done

full="$root/assets/icon/rex-ruckus.svg"
small="$root/assets/icon/rex-ruckus-small.svg"
font="$root/assets/fonts/kenney_future.ttf"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Icon sizes: the simplified art up to 32 px, the full art above.
sizes=(16 20 24 32 40 48 64 96 128 256)
for s in "${sizes[@]}"; do
    src=$full
    [ "$s" -le 32 ] && src=$small
    rsvg-convert -w "$s" -h "$s" "$src" -o "$work/icon-$s.png"
done
cp "$work/icon-256.png" "$root/assets/icon/rex-ruckus-256.png"
ico_args=()
for s in "${sizes[@]}"; do
    if [ "$s" -eq 256 ]; then ico_args+=(-r "$work/icon-$s.png"); else ico_args+=("$work/icon-$s.png"); fi
done
icotool -c -o "$root/installer/rex-ruckus.ico" "${ico_args[@]}"

# Wizard images at the Inno Setup 6.6 image-area sizes (100, 150, 200 and 250 %).
mkdir -p "$root/installer/wizard"
large_sizes=("202x386" "336x643" "430x824" "534x1022")
small_sizes=(58 97 124 159)
for i in "${!large_sizes[@]}"; do
    pct=$((100 + 50 * i))
    w=${large_sizes[$i]%x*}
    h=${large_sizes[$i]#*x}
    badge=$((w * 70 / 100))
    stripe=$((h * 4 / 100))
    top=$((h * 9 / 100))
    text_y=$((top + badge + h * 6 / 100))
    rsvg-convert -w "$badge" -h "$badge" "$full" -o "$work/badge.png"
    # Hazard stripes: a diagonal band tiled along the bottom edge.
    t=$((stripe * 2))
    magick -size "${t}x${t}" xc:'#1b0b05' -fill '#ffd84a' \
        -draw "polygon 0,$t $((t / 2)),$t $t,0 $((t / 2)),0" "$work/tile.png"
    magick -size "${w}x${stripe}" tile:"$work/tile.png" "$work/stripes.png"
    # Titles are rendered to fit 84 % of the width.
    magick -background none -font "$font" -fill '#ffd84a' -size "$((w * 84 / 100))x" \
        label:'REX RUCKUS' "$work/title.png"
    magick -background none -font "$font" -fill '#ff8a00' -size "$((w * 66 / 100))x" \
        label:'MELTDOWN' "$work/sub.png"
    title_h=$(magick identify -format '%h' "$work/title.png")
    magick -size "${w}x${h}" gradient:'#2a0e05-#7a1e06' \
        "$work/badge.png" -gravity north -geometry "+0+$top" -compose over -composite \
        "$work/title.png" -gravity north -geometry "+0+$text_y" -compose over -composite \
        "$work/sub.png" -gravity north -geometry "+0+$((text_y + title_h + h / 100))" \
        -compose over -composite \
        "$work/stripes.png" -gravity south -geometry +0+0 -compose over -composite \
        -strip -define png:color-type=2 "$root/installer/wizard/large-$pct.png"
done
for i in "${!small_sizes[@]}"; do
    pct=$((100 + 50 * i))
    s=${small_sizes[$i]}
    rsvg-convert -w "$s" -h "$s" "$full" -o "$work/small.png"
    # The page header is white; flatten onto it so the PNG needs no alpha support.
    magick "$work/small.png" -background white -flatten -strip -define png:color-type=2 \
        "$root/installer/wizard/small-$pct.png"
done

if [ -n "$preview" ]; then
    for bg in '#f2f2f2' '#202020'; do
        row=()
        for s in "${sizes[@]}"; do
            magick "$work/icon-$s.png" -background "$bg" -gravity center -extent 264x264 "$work/p$s.png"
            row+=("$work/p$s.png")
        done
        magick "${row[@]}" -background "$bg" +append "$work/row-1x-${bg#\#}.png"
        row=()
        for s in 16 24 32 48; do
            magick "$work/icon-$s.png" -filter point -resize 400% -background "$bg" \
                -gravity center -extent 264x264 "$work/z$s.png"
            row+=("$work/z$s.png")
        done
        magick "${row[@]}" -background "$bg" +append "$work/row-4x-${bg#\#}.png"
    done
    magick "$work"/row-1x-f2f2f2.png "$work"/row-1x-202020.png \
        "$work"/row-4x-f2f2f2.png "$work"/row-4x-202020.png -background '#808080' -append "$preview/icon-sheet.png"
    magick "$root/installer/wizard/large-100.png" \
        \( "$root/installer/wizard/small-100.png" -bordercolor '#cccccc' -border 1 \) \
        -background '#808080' -gravity north +append "$preview/wizard-sheet.png"
    echo "preview: $preview/icon-sheet.png $preview/wizard-sheet.png"
fi
