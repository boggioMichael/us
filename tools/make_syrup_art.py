#!/usr/bin/env python3
"""Builds Syrup's picture layers from the Maplesyrup mascot.

    python tools/make_syrup_art.py --frames <folder with frame_0001.png ...>

The frames are Maplesyrup's companion animation (ui/animations/idle/idle_01
in the Maplesyrup repository). From them this script writes, into
assets/syrup/:

- base/face-*.png   four faces of the dog (with its syrup cap), cropped and
                    scaled: talking, laughing, eyes closed, looking; and each
                    without the cap (face-*-bare.png), for the other hats;
- base/medallion.png  the S medallion Syrup always wears;
- hats/*.png        a hat for every kind of game, drawn to cover the cap;
- expressions/*.png small props (thinking dots, a magnifier, sparkles...);
- avatar.json       where each face's cap brim is, and where each hat sits.

Everything but the faces is drawn here, in Maplesyrup's style: warm fills,
a brown #57351F outline.
"""
import argparse
import json
import math
import os

from PIL import Image, ImageDraw, ImageFont

INK = (0x57, 0x35, 0x1F, 255)
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "syrup")
FACES = {"talk": 1, "laugh": 29, "closed": 53, "look": 77}
CROP = (364, 14, 918, 696)
SCALE = 0.5
SS = 4  # supersampling for the drawn layers
HAT_W, HAT_H = 340, 260
ANCHOR = (170, 190)  # where the cap's brim centre goes, in a hat image


def brim(frame):
    """Bottom of the dark band of the cap, at the face's centre column, and the cap's width there."""
    img = frame.convert("RGBA")
    x = 640
    ys = [y for y in range(150, 340) if img.getpixel((x, y))[3] > 200 and sum(img.getpixel((x, y))[:3]) / 3 < 70]
    y = ys[-1] if ys else 266
    a = img.getchannel("A")
    top = a.getbbox()[1]
    return y, top


class Pen:
    """Draws at SS times the size; `done` shrinks it back with antialiasing."""

    def __init__(self, w, h):
        self.img = Image.new("RGBA", (w * SS, h * SS), (0, 0, 0, 0))
        self.d = ImageDraw.Draw(self.img)
        self.w, self.h = w, h

    def s(self, pts):
        return [(x * SS, y * SS) for x, y in pts]

    def poly(self, pts, fill, outline=INK, width=3.5):
        self.d.polygon(self.s(pts), fill=fill)
        if outline:
            p = self.s(pts)
            self.d.line(p + [p[0]], fill=outline, width=int(width * SS), joint="curve")

    def ellipse(self, cx, cy, rx, ry, fill, outline=INK, width=3.5):
        box = [(cx - rx) * SS, (cy - ry) * SS, (cx + rx) * SS, (cy + ry) * SS]
        self.d.ellipse(box, fill=fill, outline=outline, width=int(width * SS) if outline else 0)

    def chord(self, cx, cy, rx, ry, start, end, fill, outline=INK, width=3.5):
        box = [(cx - rx) * SS, (cy - ry) * SS, (cx + rx) * SS, (cy + ry) * SS]
        self.d.chord(box, start, end, fill=fill, outline=outline, width=int(width * SS) if outline else 0)

    def line(self, pts, fill=INK, width=3.5):
        self.d.line(self.s(pts), fill=fill, width=int(width * SS), joint="curve")

    def rect(self, x0, y0, x1, y1, fill, outline=INK, width=3.5, r=0):
        box = [x0 * SS, y0 * SS, x1 * SS, y1 * SS]
        self.d.rounded_rectangle(box, radius=r * SS, fill=fill, outline=outline, width=int(width * SS) if outline else 0)

    def text(self, cx, cy, s, size, fill, font="DejaVuSans-Bold.ttf"):
        f = ImageFont.truetype(os.path.join(FONTS, font), int(size * SS))
        self.d.text((cx * SS, cy * SS), s, font=f, fill=fill, anchor="mm")

    def done(self):
        return self.img.resize((self.w, self.h), Image.LANCZOS)


FONTS = "/usr/share/fonts/truetype/dejavu"


def curve(p0, p1, p2, n=24):
    """A quadratic Bézier from p0 to p2, pulled towards p1."""
    out = []
    for i in range(n + 1):
        t = i / n
        a = (1 - t) ** 2
        b = 2 * (1 - t) * t
        c = t * t
        out.append((a * p0[0] + b * p1[0] + c * p2[0], a * p0[1] + b * p1[1] + c * p2[1]))
    return out


def star(cx, cy, r, inner=0.45, points=5, rot=-90):
    pts = []
    for i in range(points * 2):
        rr = r if i % 2 == 0 else r * inner
        a = math.radians(rot + i * 180 / points)
        pts.append((cx + rr * math.cos(a), cy + rr * math.sin(a)))
    return pts


# ---- hats: each covers the cap (about 236 px wide at the brim, 118 px tall above it) ----
AX, AY = ANCHOR


def wizard_hat(p):
    blue, dark = (86, 84, 196, 255), (60, 56, 150, 255)
    cone = [(AX - 96, AY - 30)] + curve((AX - 96, AY - 30), (AX - 40, AY - 120), (AX + 8, AY - 182)) + curve((AX + 8, AY - 182), (AX + 50, AY - 150), (AX + 98, AY - 30))[1:]
    p.poly(cone, blue)
    p.poly([(AX - 70, AY - 70), (AX - 20, AY - 78), (AX + 5, AY - 150), (AX - 25, AY - 118)], (110, 108, 220, 255), outline=None)
    p.rect(AX - 100, AY - 44, AX + 102, AY - 20, (240, 190, 70, 255), r=6)
    p.ellipse(AX, AY - 6, 132, 26, dark)
    p.chord(AX, AY - 8, 118, 18, 180, 360, blue, outline=None)
    for sx, sy, r in [(AX - 38, AY - 92, 11), (AX + 30, AY - 64, 8), (AX + 4, AY - 128, 7)]:
        p.poly(star(sx, sy, r), (255, 214, 90, 255), width=2)
    p.ellipse(AX + 10, AY - 186, 9, 9, (255, 214, 90, 255), width=2.5)


def knight_helmet(p):
    steel, shade = (190, 198, 210, 255), (140, 150, 166, 255)
    p.chord(AX, AY + 10, 124, 150, 180, 360, steel)
    p.chord(AX + 40, AY + 10, 70, 128, 250, 330, (220, 226, 236, 255), outline=None)
    p.rect(AX - 126, AY - 20, AX + 126, AY + 12, shade, r=8)
    for x in range(-100, 101, 50):
        p.ellipse(AX + x, AY - 4, 5, 5, (230, 234, 240, 255), width=2)
    p.rect(AX - 8, AY - 136, AX + 8, AY - 22, shade, r=4)
    plume = [(AX - 6, AY - 132)] + curve((AX - 6, AY - 132), (AX - 70, AY - 200), (AX + 20, AY - 214)) + curve((AX + 20, AY - 214), (AX + 60, AY - 170), (AX + 6, AY - 132))[1:]
    p.poly(plume, (214, 58, 64, 255))


def ranger_hood(p):
    green, dark = (82, 132, 74, 255), (58, 98, 52, 255)
    hood = [(AX - 132, AY + 22)] + curve((AX - 132, AY + 22), (AX - 140, AY - 150), (AX - 10, AY - 176)) + curve((AX - 10, AY - 176), (AX + 40, AY - 186), (AX + 64, AY - 206)) + curve((AX + 64, AY - 206), (AX + 150, AY - 110), (AX + 132, AY + 22))[1:]
    p.poly(hood, green)
    p.poly(curve((AX - 110, AY + 12), (AX, AY - 60), (AX + 110, AY + 12)) + [(AX + 110, AY + 22), (AX - 110, AY + 22)], dark, outline=None)
    p.line(curve((AX - 112, AY + 16), (AX, AY - 58), (AX + 112, AY + 16)), width=3)
    p.line(curve((AX - 60, AY - 120), (AX - 20, AY - 150), (AX + 40, AY - 160)), fill=(120, 170, 100, 255), width=4)


def racing_helmet(p):
    red = (214, 50, 50, 255)
    p.chord(AX, AY + 14, 126, 146, 180, 360, red)
    p.poly([(AX - 20, AY - 128), (AX + 20, AY - 128), (AX + 30, AY + 8), (AX - 30, AY + 8)], (250, 250, 250, 255), width=2.5)
    p.rect(AX - 124, AY - 26, AX + 124, AY + 12, (40, 50, 70, 255), r=10)
    p.rect(AX - 110, AY - 20, AX - 40, AY - 12, (110, 150, 200, 255), outline=None, r=3)
    p.ellipse(AX - 70, AY - 80, 22, 22, (255, 255, 255, 255), width=2.5)
    p.text(AX - 70, AY - 80, "1", 26, INK)


def tactical_helmet(p):
    olive, dark = (110, 118, 72, 255), (84, 90, 54, 255)
    p.chord(AX, AY + 16, 130, 148, 180, 360, olive)
    for i in range(6):
        x = AX - 90 + i * 36
        p.line([(x, AY - 110 + abs(i - 2.5) * 14), (x + 12, AY - 12)], fill=dark, width=3)
    p.rect(AX - 132, AY - 16, AX + 132, AY + 14, dark, r=8)
    p.rect(AX - 78, AY - 52, AX + 78, AY - 20, (60, 60, 60, 255), r=10)
    p.ellipse(AX - 38, AY - 36, 30, 14, (120, 200, 220, 255), width=3)
    p.ellipse(AX + 38, AY - 36, 30, 14, (120, 200, 220, 255), width=3)


def astronaut_helmet(p):
    white, grey = (246, 246, 250, 255), (200, 204, 214, 255)
    p.chord(AX, AY + 18, 130, 150, 180, 360, white)
    p.chord(AX + 30, AY + 18, 80, 128, 250, 320, (255, 255, 255, 255), outline=None)
    p.rect(AX - 132, AY - 22, AX + 132, AY + 14, grey, r=10)
    p.rect(AX - 96, AY - 84, AX + 96, AY - 30, (80, 150, 230, 255), r=18)
    p.rect(AX - 80, AY - 78, AX - 20, AY - 66, (170, 210, 255, 255), outline=None, r=6)
    p.line([(AX + 60, AY - 124), (AX + 86, AY - 176)], width=4)
    p.ellipse(AX + 88, AY - 180, 11, 11, (240, 70, 70, 255), width=3)


def pirate_hat(p):
    black, gold = (40, 36, 44, 255), (230, 180, 60, 255)
    shape = [(AX - 150, AY - 30)] + curve((AX - 150, AY - 30), (AX - 90, AY - 170), (AX, AY - 150)) + curve((AX, AY - 150), (AX + 90, AY - 170), (AX + 150, AY - 30))[1:] + curve((AX + 150, AY - 30), (AX, AY + 30), (AX - 150, AY - 30))[1:]
    p.poly(shape, black)
    p.line(curve((AX - 140, AY - 30), (AX, AY + 20), (AX + 140, AY - 30)), fill=gold, width=5)
    feather = curve((AX + 40, AY - 120), (AX + 120, AY - 230), (AX + 10, AY - 210)) + curve((AX + 10, AY - 210), (AX + 40, AY - 170), (AX + 40, AY - 120))[1:]
    p.poly(feather, (240, 90, 70, 255), width=2.5)
    p.ellipse(AX - 20, AY - 80, 22, 22, gold, width=3)
    p.text(AX - 20, AY - 80, "S", 26, INK)


def straw_hat(p):
    straw, dark = (238, 204, 120, 255), (200, 160, 80, 255)
    p.ellipse(AX, AY - 6, 160, 36, straw)
    for r in (140, 118):
        p.d.ellipse([(AX - r) * SS, (AY - 6 - r * 0.22) * SS, (AX + r) * SS, (AY - 6 + r * 0.22) * SS], outline=dark, width=2 * SS)
    p.chord(AX, AY - 10, 104, 118, 180, 360, straw)
    p.rect(AX - 104, AY - 40, AX + 104, AY - 12, (214, 70, 60, 255), r=4)
    for i in range(7):
        x = AX - 80 + i * 26
        p.line([(x, AY - 110 + abs(i - 3) * 8), (x + 6, AY - 44)], fill=dark, width=2)


def detective_cap(p):
    base, check = (156, 118, 78, 255), (120, 88, 56, 255)
    p.chord(AX, AY + 10, 126, 136, 180, 360, base)
    for i in range(-5, 6):
        x = AX + i * 22
        p.line([(x, AY - 120 + abs(i) * 6), (x, AY - 4)], fill=check, width=2.5)
    for j in range(5):
        y = AY - 110 + j * 24
        p.line([(AX - 110 + j * 4, y), (AX + 110 - j * 4, y)], fill=check, width=2.5)
    p.chord(AX, AY + 10, 126, 136, 180, 360, None)
    p.poly([(AX - 150, AY - 10), (AX - 60, AY - 30), (AX - 60, AY + 6), (AX - 150, AY + 14)], base)
    p.poly([(AX + 150, AY - 10), (AX + 60, AY - 30), (AX + 60, AY + 6), (AX + 150, AY + 14)], base)
    p.ellipse(AX, AY - 136, 12, 9, check, width=3)


def dealer_visor(p):
    black, green = (34, 34, 40, 255), (40, 150, 90, 255)
    p.ellipse(AX, AY - 4, 150, 30, black)
    p.rect(AX - 96, AY - 150, AX + 96, AY - 10, black, r=10)
    p.rect(AX - 96, AY - 44, AX + 96, AY - 16, green, r=4)
    p.rect(AX + 30, AY - 116, AX + 76, AY - 52, (250, 248, 240, 255), width=2.5, r=5)
    p.text(AX + 53, AY - 92, "A", 24, (200, 30, 40, 255))
    p.poly(star(AX + 53, AY - 66, 7, inner=0.5, points=4, rot=0), (200, 30, 40, 255), outline=None)
    p.rect(AX - 96, AY - 150, AX + 96, AY - 138, (60, 60, 70, 255), outline=None, r=6)


def sports_cap(p):
    blue, dark = (52, 110, 200, 255), (36, 80, 150, 255)
    p.chord(AX, AY + 6, 122, 134, 180, 360, blue)
    for a in (-60, 0, 60):
        p.line(curve((AX, AY - 128), (AX + a * 0.6, AY - 70), (AX + a * 1.6, AY - 4)), fill=dark, width=3)
    bill = curve((AX - 118, AY - 6), (AX - 40, AY + 44), (AX + 150, AY + 6)) + curve((AX + 150, AY + 6), (AX + 30, AY - 20), (AX - 118, AY - 6))[1:]
    p.poly(bill, dark)
    p.ellipse(AX, AY - 130, 12, 8, dark, width=3)
    p.ellipse(AX - 40, AY - 66, 24, 24, (255, 255, 255, 255), width=3)
    p.text(AX - 40, AY - 66, "S", 28, dark)


def lantern_hat(p):
    yellow, dark = (240, 196, 50, 255), (196, 150, 30, 255)
    p.chord(AX, AY + 12, 126, 142, 180, 360, yellow)
    p.rect(AX - 132, AY - 18, AX + 132, AY + 12, dark, r=8)
    for r, c in ((70, (246, 214, 96, 255)), (52, (250, 228, 128, 255)), (36, (253, 240, 160, 255))):
        p.ellipse(AX, AY - 84, r, r, c, outline=None)
    p.ellipse(AX, AY - 84, 30, 30, (90, 90, 96, 255))
    p.ellipse(AX, AY - 84, 20, 20, (255, 248, 190, 255), width=2.5)


HATS = {
    "wizard_hat": wizard_hat,
    "knight_helmet": knight_helmet,
    "ranger_hood": ranger_hood,
    "racing_helmet": racing_helmet,
    "tactical_helmet": tactical_helmet,
    "astronaut_helmet": astronaut_helmet,
    "pirate_hat": pirate_hat,
    "straw_hat": straw_hat,
    "detective_cap": detective_cap,
    "dealer_visor": dealer_visor,
    "sports_cap": sports_cap,
    "lantern_hat": lantern_hat,
}


def medallion():
    size = 96
    p = Pen(size, size)
    c = size / 2
    p.ellipse(c, c, 44, 44, (196, 128, 36, 255), width=4)
    p.ellipse(c, c, 36, 36, (246, 190, 70, 255), outline=(214, 150, 50, 255), width=3)
    p.chord(c, c, 30, 30, 200, 340, (255, 222, 130, 255), outline=None)
    p.text(c, c + 2, "S", 50, INK)
    return p.done()


def prop(name):
    p = Pen(96, 96)
    if name == "thinking":
        for (x, y, r) in [(24, 76, 7), (42, 58, 10), (66, 32, 16)]:
            p.ellipse(x, y, r, r, (255, 250, 240, 255), width=3)
    elif name == "researching":
        p.line([(58, 58), (84, 86)], width=10)
        p.ellipse(42, 42, 26, 26, (220, 240, 255, 200), width=5)
        p.chord(42, 42, 18, 18, 200, 260, (255, 255, 255, 255), outline=None)
    elif name == "excited":
        for (x, y, r) in [(30, 28, 16), (70, 48, 12), (40, 74, 9)]:
            p.poly(star(x, y, r, inner=0.35, points=4, rot=0), (255, 214, 80, 255), width=2.5)
    elif name == "proud":
        p.poly(star(48, 50, 38), (255, 208, 70, 255), width=4)
        p.chord(48, 50, 18, 18, 200, 320, (255, 236, 150, 255), outline=None)
    elif name == "warning":
        p.ellipse(48, 48, 40, 40, (240, 110, 40, 255), width=4)
        p.text(48, 50, "!", 54, (255, 255, 255, 255))
    elif name == "confused":
        p.ellipse(48, 46, 38, 36, (255, 250, 240, 255), width=4)
        p.poly([(30, 76), (22, 92), (44, 80)], (255, 250, 240, 255), width=3)
        p.text(48, 48, "?", 50, INK)
    elif name == "surprised":
        p.text(38, 48, "!", 60, (230, 80, 50, 255))
        p.text(62, 44, "?", 52, (230, 80, 50, 255))
    return p.done()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--frames", required=True, help="folder with Maplesyrup's frame_0001.png ... frame_0120.png")
    args = ap.parse_args()
    for d in ("base", "hats", "expressions"):
        os.makedirs(os.path.join(OUT, d), exist_ok=True)
    meta = {"scale": SCALE, "faces": {}, "hat_anchor": list(ANCHOR), "hat_size": [HAT_W, HAT_H]}
    for name, n in FACES.items():
        frame = Image.open(os.path.join(args.frames, f"frame_{n:04d}.png")).convert("RGBA")
        y, top = brim(frame)
        face = frame.crop(CROP)
        face = face.resize((round(face.width * SCALE), round(face.height * SCALE)), Image.LANCZOS)
        face.save(os.path.join(OUT, "base", f"face-{name}.png"), optimize=True)
        # The same face without its cap, for the other hats: everything above
        # the cap's brim goes (the hat covers the cut).
        bare = face.copy()
        cut = round((y - CROP[1]) * SCALE) + 3
        px = bare.load()
        for yy in range(min(cut, bare.height)):
            for xx in range(bare.width):
                px[xx, yy] = (0, 0, 0, 0)
        bare.save(os.path.join(OUT, "base", f"face-{name}-bare.png"), optimize=True)
        cx = (640 - CROP[0]) * SCALE
        meta["faces"][name] = {"size": [face.width, face.height], "brim": [round(cx, 1), round((y - CROP[1]) * SCALE, 1)], "cap_top": round((top - CROP[1]) * SCALE, 1)}
    medallion().save(os.path.join(OUT, "base", "medallion.png"), optimize=True)
    for name, draw in HATS.items():
        p = Pen(HAT_W, HAT_H)
        draw(p)
        p.done().save(os.path.join(OUT, "hats", f"{name}.png"), optimize=True)
    for name in ("thinking", "researching", "excited", "proud", "warning", "confused", "surprised"):
        prop(name).save(os.path.join(OUT, "expressions", f"{name}.png"), optimize=True)
    with open(os.path.join(OUT, "avatar.json"), "w") as f:
        json.dump(meta, f, indent=1)
    print("wrote Syrup's layers to", os.path.normpath(OUT))


if __name__ == "__main__":
    main()
