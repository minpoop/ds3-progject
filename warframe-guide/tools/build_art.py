"""Regenerate assets/art.js: the item pictures embedded in the guide.

Reads every weapon, Warframe, mod, resource and currency name used in assets/data.js,
looks up its picture in the community @wfcd/items data, downloads it from
cdn.warframestat.us at full resolution (512px), converts it to WebP and writes them all as data: URIs,
so the guide shows pictures offline and inside sandboxed viewers.

Needs: python3 with Pillow, and curl.
Run from the warframe-guide folder:  python3 tools/build_art.py
"""
import base64
import concurrent.futures as cf
import io
import json
import re
import subprocess
import tempfile
import time
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "assets" / "data.js"
OUT = ROOT / "assets" / "art.js"
ITEMS = "https://cdn.jsdelivr.net/npm/@wfcd/items/data/json/{}.json"
IMG = "https://cdn.warframestat.us/img/{}"
CATEGORIES = ["Primary", "Secondary", "Melee", "Warframes", "Resources", "Misc", "Mods", "Arcanes"]
# Names the guide uses for art that are not item names of their own.
EXTRA = ["Archon Shard", "Intact Sentient Core", "Kuva", "Corrupted Holokey", "Primary Arcane Adapter",
         "Riven Sliver", "Arcane Energize", "Aya", "Blind Rage", "Intensify", "Vitus Essence", "Hunter Munitions"]
KNOWN = {"Forma": "Forma.png", "Kuva": "Kuva.png"}


def curl(url, dest, tries=5):
    # The image CDN rate-limits bursts (HTTP 429), so back off and retry.
    for attempt in range(tries):
        if subprocess.run(["curl", "-sSLf", "-o", str(dest), url]).returncode == 0:
            return
        time.sleep(2 ** attempt)
    raise RuntimeError(f"could not download {url}")


def names_in_guide(src):
    names = set(re.findall(r'^  "([^"]+)": \["(?:Primary|Secondary|Melee)"', src, re.M))
    names |= set(re.findall(r'^  \["([^"]+)", "(?:Early|Mid|Mid-Late|Late|Quest)"', src, re.M))
    for start, end in [("const MODS", "const BUILDS"), ("const CURRENCIES", "const FARM_TIPS")]:
        names |= set(re.findall(r', "([^"]+)"\],\n', src[src.index(start):src.index(end)]))
    names |= set(re.findall(r'^  \["([^"]+)",', src[src.index("const MATERIALS"):src.index("const CURRENCIES")], re.M))
    return names | set(EXTRA)


def main():
    tmp = Path(tempfile.mkdtemp())
    index = {}
    for cat in CATEGORIES:
        curl(ITEMS.format(cat), tmp / f"{cat}.json")
        for item in json.loads((tmp / f"{cat}.json").read_text()):
            if item.get("name") and item.get("imageName"):
                index.setdefault(item["name"], item["imageName"])
    index.update(KNOWN)
    wanted = sorted(n for n in names_in_guide(DATA.read_text()) if n in index)

    def fetch(name):
        path = tmp / re.sub(r"[^\w.-]", "_", index[name])
        try:
            curl(IMG.format(index[name]), path)
            im = Image.open(path).convert("RGBA")
        except Exception as exc:  # missing picture: the page falls back to a letter glyph
            print("skip", name, exc)
            return name, None
        buf = io.BytesIO()
        im.save(buf, "WEBP", quality=84, method=6)
        return name, "data:image/webp;base64," + base64.b64encode(buf.getvalue()).decode()

    with cf.ThreadPoolExecutor(4) as pool:
        art = {n: uri for n, uri in pool.map(fetch, wanted) if uri}
    OUT.write_text(
        "/* Item art at full resolution, as WebP, embedded so the guide works offline.\n"
        "   Source: cdn.warframestat.us (community @wfcd/items image set). Regenerate with tools/build_art.py. */\n"
        '"use strict";\nconst ART = ' + json.dumps(dict(sorted(art.items())), separators=(",", ":")) + ";\n")
    print(f"{len(art)} pictures, {OUT.stat().st_size // 1024} KB -> {OUT}")


if __name__ == "__main__":
    main()
