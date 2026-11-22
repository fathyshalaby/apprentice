#!/bin/sh
# Regenerates every Apprentice icon from the SVG sources in assets/. Requires
# macOS (Swift + AppKit) and python3; the SVG sources are the only files to edit.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
renderer="$root/scripts/render-apprentice-icon.swift"
glyph="$root/assets/apprentice-glyph.svg"
recording="$root/assets/apprentice-glyph-recording.svg"
mark="$root/assets/apprentice-mark.svg"

render() {
  case $4 in
    /*) destination=$4 ;;
    *) destination=$root/$4 ;;
  esac
  swift "$renderer" "$1" "$2" "$3" "$destination" "${5:-#ffffff}" "${6:-png}" \
    "${7:-}" "${8:-}" "${9:-}" "${10:-}"
}

# Icon Composer's layer: the white glyph on its transparent template canvas.
render "$glyph" 1450 1450 app/AppIcon.icon/Assets/Image.png "#ffffff" canvas 1488 1672

# Windows executable, taskbar, and shortcut icons.
for size in 32 64 128; do
  render "$mark" "$size" "$size" "resources/windows/icon-$size.png"
done

# Windows tray: raw white RGBA; src/desktop/tray_icon.rs tints it for light bars.
render "$glyph" 32 32 resources/windows/tray-32.rgba "#ffffff" raw

# Linux hicolor theme icon. The file name stays anomaly.hex.png because it is
# the installed app id, not a brand name; only the pixels change.
for size in 16 24 32 48 64 128 256 512; do
  render "$mark" "$size" "$size" "packaging/linux/icons/hicolor/${size}x${size}/apps/anomaly.hex.png"
done

# Site favicon and the 1024 px social-card icon.
render "$mark" 1024 1024 site/scripts/og/hex-icon.png
sed 's/__INK__/#ffffff/g' "$mark" > "$root/site/public/favicon.svg"

# macOS menu bar: 18 pt @2x template images, black with alpha.
render "$glyph" 36 36 assets/icons/menubar-idle@2x.png "#000000"
render "$recording" 36 36 assets/icons/menubar-recording@2x.png "#000000"

# ICO containers with PNG entries (Vista+), packed without external tools.
# A 256 px frame needs its own 256 px image: a larger PNG labelled 256 is
# rejected by some browsers.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
render "$mark" 256 256 "$tmp/favicon-256.png"
python3 - "$root" "$tmp/favicon-256.png" <<'PY'
import pathlib, struct, sys
root = pathlib.Path(sys.argv[1])
large = pathlib.Path(sys.argv[2])

def pack(out, entries):
    directory, payload = [], bytearray()
    offset = 6 + 16 * len(entries)
    for source, size in entries:
        image = pathlib.Path(source).read_bytes()
        dimension = size if size < 256 else 0
        directory.append(struct.pack("<BBBBHHII", dimension, dimension, 0, 0, 1, 32, len(image), offset))
        payload += image
        offset += len(image)
    (root / out).write_bytes(struct.pack("<HHH", 0, 1, len(entries)) + b"".join(directory) + bytes(payload))

pack("resources/windows/hex.ico", [
    (root / "resources/windows/icon-32.png", 32),
    (root / "resources/windows/icon-64.png", 64),
    (root / "resources/windows/icon-128.png", 128),
    (large, 256),
])
pack("site/public/favicon.ico", [
    (root / "resources/windows/icon-32.png", 32),
    (root / "resources/windows/icon-64.png", 64),
    (root / "resources/windows/icon-128.png", 128),
    (large, 256),
])
PY

echo "Apprentice icons regenerated."
