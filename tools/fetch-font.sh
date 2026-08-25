#!/usr/bin/env bash
#
# Fetches the Japanese font Kanban's glyph atlas is baked from.
#
#   tools/fetch-font.sh
#   cargo run -p fete-glyph-atlas
#
# Noto Sans CJK JP, Light. Light because the characters are drawn as lit tube:
# the shader thickens the strokes itself, and a weight that arrives already
# heavy closes up the inside of a dense character — 繁, 舞, 薬 — the moment it
# is asked to glow. It is also the weight whose stroke width is closest to
# constant across the set, which is what neon actually is.
#
# The font is not committed and nothing at runtime reads it: what ships is
# `visuals/fete-visual-kanban/src/glyphs.png`, an image of distances baked from
# it once. Noto is under the SIL Open Font License 1.1; the licence travels with
# the font file, which this script leaves next to the tool, gitignored.
#
# 16 MB, one file, a few seconds.

set -euo pipefail

DEST="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/glyph-atlas/NotoSansCJKjp-Light.otf"
URL="https://github.com/notofonts/noto-cjk/raw/main/Sans/OTF/Japanese/NotoSansCJKjp-Light.otf"

if [[ -f "$DEST" ]]; then
    echo "already have $DEST"
    exit 0
fi

echo "fetching Noto Sans CJK JP Light..."
curl -fsSL --retry 3 -o "$DEST.part" "$URL"
mv "$DEST.part" "$DEST"
echo "wrote $DEST"
