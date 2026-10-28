"""Regenerate assets/portraits.js: the six companions' character portraits.

Downloads each character's official render from their game's fan wiki, crops a
4:5 head-and-shoulders bust, puts it on a dark glow in the character's colour,
and embeds it as a small WebP data: URI. If a portrait is missing, the page
falls back to the drawn SVG bust in companions.js.

Needs: python3 with Pillow, and curl.
Run from the warframe-guide folder:  python3 tools/build_portraits.py
"""
import base64
import io
import json
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "portraits.js"
WIKI = "https://static.wikia.nocookie.net/"
# id: (image path on the wiki CDN, centre x, top y, bottom y as fractions of the image, glow colour)
SPEC = {
    "james": ("silent/images/6/62/James_Sunderland.webp/revision/latest", 0.39, 0.0, 0.30, (59, 63, 42)),
    "simon": ("cry-of-fear/images/9/94/Cof_simon.png/revision/latest", 0.43, 0.013, 0.31, (42, 50, 64)),
    "dante": ("devilmaycry/images/c/c2/Dante_DMC5.png/revision/latest", 0.44, 0.03, 0.30, (74, 20, 20)),
    "vergil": ("devilmaycry/images/4/4e/Vergil_DMC5.png/revision/latest", 0.48, 0.12, 0.40, (22, 36, 63)),
    "leon": ("residentevil/images/7/7d/RE9_-_Leon_Render.png/revision/latest", 0.39, 0.03, 0.30, (61, 44, 27)),
    "torque": ("thesuffering/images/8/8a/Torgue%27s_Mugshoot.png/revision/latest", 0.27, 0.18, 0.70, (61, 29, 16)),
}
SIZE = (640, 800)


def bust(path, cx, top, bottom, glow):
    im = Image.open(path).convert("RGBA")
    w, h = im.size
    ch = (bottom - top) * h
    cw = ch * SIZE[0] / SIZE[1]
    x0 = cx * w - cw / 2
    crop = im.crop((round(x0), round(top * h), round(x0 + cw), round(bottom * h))).resize(SIZE, Image.LANCZOS)
    bg = Image.new("RGBA", SIZE, (8, 6, 5, 255))
    draw = ImageDraw.Draw(bg)
    for i in range(60, 0, -1):  # radial glow behind the figure, darkest at the edges
        t = i / 60
        colour = tuple(int(base + (c - base) * (1 - t)) for c, base in zip(glow, (8, 6, 5))) + (255,)
        cx, cy, r = SIZE[0] / 2, SIZE[1] * 0.375, SIZE[0] * 0.94 * t
        draw.ellipse((cx - r, cy - r, cx + r, cy + r), fill=colour)
    bg = bg.filter(ImageFilter.GaussianBlur(12))
    bg.alpha_composite(crop)
    buf = io.BytesIO()
    bg.convert("RGB").save(buf, "WEBP", quality=86, method=6)
    return "data:image/webp;base64," + base64.b64encode(buf.getvalue()).decode()


def main():
    tmp = Path(tempfile.mkdtemp())
    out = {}
    for cid, (src, cx, top, bottom, glow) in SPEC.items():
        path = tmp / cid
        subprocess.run(["curl", "-sSLf", "-A", "Mozilla/5.0", "-o", str(path), WIKI + src], check=True)
        out[cid] = bust(path, cx, top, bottom, glow)
    OUT.write_text(
        "/* Companion portraits: official character renders from each game's fan wiki, cropped to busts\n"
        "   and embedded as WebP. Regenerate with tools/build_portraits.py. */\n"
        '"use strict";\nconst PORTRAITS = ' + json.dumps(out, separators=(",", ":")) + ";\n")
    print(f"{len(out)} portraits, {OUT.stat().st_size // 1024} KB -> {OUT}")


if __name__ == "__main__":
    main()
