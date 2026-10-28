"""Regenerate assets/keyart.js: the official key art behind the page and in the home-page hero.

Downloads two pieces of official Digital Extremes key art from the official Warframe wiki,
colour-grades them toward the site's dark blood-red palette, and embeds them as WebP:
  - hero: "Whispers in the Walls" update key art (Albrecht Entrati under the giant statue face)
  - room: "Albrecht's Laboratories" promo art, used as the fixed page background

Needs: python3 with Pillow, and curl.
Run from the warframe-guide folder:  python3 tools/build_keyart.py
"""
import base64
import io
import json
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageEnhance, ImageOps

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "keyart.js"
WIKI = "https://wiki.warframe.com/images/"
# id: (wiki file, how much original colour to keep, brightness)
ART = {
    "hero": ("WhispersintheWalls.png", 0.45, 0.95),
    "room": ("Albrecht%27s_Labs_promo.jpg", 0.30, 0.75),
}
SIZE = (1920, 1080)


def grade(path, keep, brightness):
    im = ImageOps.fit(Image.open(path).convert("RGB"), SIZE, Image.LANCZOS)
    # duotone from near-black through blood red to warm bone, then blend some original colour back in
    duo = ImageOps.colorize(ImageOps.grayscale(im), black=(10, 6, 5), mid=(120, 34, 24), white=(236, 214, 190), midpoint=110)
    out = Image.blend(duo, im, keep)
    out = ImageEnhance.Contrast(ImageEnhance.Brightness(out).enhance(brightness)).enhance(1.12)
    buf = io.BytesIO()
    out.save(buf, "WEBP", quality=82, method=6)
    return "data:image/webp;base64," + base64.b64encode(buf.getvalue()).decode()


def main():
    tmp = Path(tempfile.mkdtemp())
    out = {}
    for key, (name, keep, brightness) in ART.items():
        path = tmp / key
        subprocess.run(["curl", "-sSLf", "-m", "120", "-A", "Mozilla/5.0", "-o", str(path), WIKI + name], check=True)
        out[key] = grade(path, keep, brightness)
    OUT.write_text(
        "/* Official Warframe key art (Whispers in the Walls; Albrecht's Laboratories), colour-graded and embedded.\n"
        "   Regenerate with tools/build_keyart.py. */\n"
        '"use strict";\nconst KEYART = ' + json.dumps(out, separators=(",", ":")) + ";\n")
    print(f"{len(out)} images, {OUT.stat().st_size // 1024} KB -> {OUT}")


if __name__ == "__main__":
    main()
