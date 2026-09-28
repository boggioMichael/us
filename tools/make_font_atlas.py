#!/usr/bin/env python3
"""Rasterise DejaVu Sans into glyph atlases for Syrup's painter.

The painter (crates/syrup-paint) draws all of Syrup's text itself, the same on
every platform, from these atlases: one grey-level PNG per style and size,
and one JSON file with every glyph's box and advance.

    python tools/make_font_atlas.py [--fonts /usr/share/fonts/truetype/dejavu]

DejaVu fonts are free (Bitstream Vera licence, with DejaVu's changes in the
public domain); the licence is copied next to the atlases.
"""
import argparse
import json
import os

from PIL import Image, ImageDraw, ImageFont

STYLES = {
    "sans": "DejaVuSans.ttf",
    "sans-bold": "DejaVuSans-Bold.ttf",
}
SIZES = {
    "sans": [11, 13, 16, 20],
    "sans-bold": [11, 13, 16, 20, 26, 34, 48, 72],
}
# Printable ASCII, plus the few symbols Syrup's lines use.
CHARS = [chr(c) for c in range(32, 127)] + list("•…–—’“”°×·✓✗▶◀▲▼←→↑↓")


def build(font_path, size):
    font = ImageFont.truetype(font_path, size)
    ascent, descent = font.getmetrics()
    glyphs = []
    for ch in CHARS:
        try:
            x0, y0, x1, y1 = font.getbbox(ch, anchor="ls")
        except Exception:
            continue
        advance = font.getlength(ch)
        w, h = max(0, x1 - x0), max(0, y1 - y0)
        img = Image.new("L", (max(w, 1), max(h, 1)), 0)
        if w > 0 and h > 0:
            ImageDraw.Draw(img).text((-x0, -y0), ch, font=font, anchor="ls", fill=255)
        glyphs.append((ch, img, x0, y0, w, h, advance))
    # Shelf packing, 512 px wide.
    width = 512
    x = y = shelf = 0
    placed = []
    for ch, img, x0, y0, w, h, adv in sorted(glyphs, key=lambda g: -g[5]):
        if x + w + 1 > width:
            x, y, shelf = 0, y + shelf + 1, 0
        placed.append((ch, img, x, y, x0, y0, w, h, adv))
        x += w + 1
        shelf = max(shelf, h)
    height = y + shelf + 1
    atlas = Image.new("L", (width, height), 0)
    meta = {}
    for ch, img, x, y, x0, y0, w, h, adv in placed:
        if w > 0 and h > 0:
            atlas.paste(img, (x, y))
        meta[ch] = [x, y, w, h, x0, y0, round(adv, 3)]
    return atlas, {"size": size, "ascent": ascent, "descent": descent, "line_height": ascent + descent, "glyphs": meta}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fonts", default="/usr/share/fonts/truetype/dejavu")
    ap.add_argument("--out", default=os.path.join(os.path.dirname(__file__), "..", "assets", "syrup", "fonts"))
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    index = {}
    for style, file in STYLES.items():
        for size in SIZES[style]:
            atlas, meta = build(os.path.join(args.fonts, file), size)
            name = f"{style}-{size}"
            atlas.save(os.path.join(args.out, name + ".png"), optimize=True)
            index[name] = meta
    with open(os.path.join(args.out, "atlas.json"), "w", encoding="utf-8") as f:
        json.dump(index, f, ensure_ascii=False, separators=(",", ":"))
    print("wrote", len(index), "atlases to", os.path.normpath(args.out))


if __name__ == "__main__":
    main()
