"""Regenerate assets/news.js: a snapshot of official Warframe news for the News chapter.

Two official sources:
  - the in-game news feed (Digital Extremes' world state, served as JSON by api.warframestat.us)
  - the official PC Update Notes RSS feed from forums.warframe.com

The page refreshes the in-game feed live when it can reach the internet; this snapshot is what it
shows offline, inside sandboxed viewers that block outside requests, and for the patch notes
(the forum feed can't be read from a browser page). News images are embedded so they show offline.

Needs: python3 with Pillow, and curl.
Run from the warframe-guide folder:  python3 tools/build_news.py
"""
import base64
import html
import io
import json
import re
import subprocess
import tempfile
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "news.js"
FEED = "https://api.warframestat.us/pc/news?language=en"
NOTES = "https://forums.warframe.com/forum/3-pc-update-notes.xml/"
PLACEHOLDER = "news-placeholder"
MAX_NOTES = 12


def fetch(url, dest):
    subprocess.run(["curl", "-sSLf", "-m", "60", "-A", "Mozilla/5.0", "-o", str(dest), url], check=True)
    return dest


def embed(url, tmp):
    if not url or PLACEHOLDER in url:
        return None
    try:
        im = Image.open(fetch(url, tmp / "img")).convert("RGB")
    except Exception as exc:  # a missing news image just shows the card without art
        print("no image", url, exc)
        return None
    im.thumbnail((900, 900), Image.LANCZOS)  # cards show these well under 900px wide
    buf = io.BytesIO()
    im.save(buf, "WEBP", quality=84, method=6)
    return "data:image/webp;base64," + base64.b64encode(buf.getvalue()).decode()


def kind(item):
    if item.get("update"):
        return "Update"
    if item.get("primeAccess"):
        return "Prime Access"
    if item.get("stream"):
        return "Stream"
    return "News"


def summary(desc, limit=260):
    text = html.unescape(re.sub(r"<[^>]+>", " ", desc or ""))
    text = re.sub(r"\s+", " ", text).strip()
    return text if len(text) <= limit else text[:limit].rsplit(" ", 1)[0] + "…"


def main():
    tmp = Path(tempfile.mkdtemp())
    feed = json.loads(fetch(FEED, tmp / "news.json").read_text())
    items = []
    for n in sorted(feed, key=lambda x: x.get("date", ""), reverse=True):
        if n.get("date", "").startswith("1970") or n.get("mobileOnly"):
            continue  # evergreen links (Discord, wiki, forums) with no real date
        items.append({"date": n["date"], "title": n["message"].strip(), "link": n["link"],
                      "kind": kind(n), "img": embed(n.get("imageLink"), tmp)})

    root = ET.parse(fetch(NOTES, tmp / "notes.xml")).getroot()
    notes = []
    for it in root.iter("item"):
        when = parsedate_to_datetime(it.findtext("pubDate")).astimezone(timezone.utc)
        notes.append({"date": when.isoformat(), "title": it.findtext("title").strip(),
                      "link": it.findtext("link"), "summary": summary(it.findtext("description"))})
    notes = sorted(notes, key=lambda x: x["date"], reverse=True)[:MAX_NOTES]

    snapshot = {"built": datetime.now(timezone.utc).isoformat(timespec="minutes"), "items": items, "notes": notes}
    OUT.write_text(
        "/* Snapshot of official Warframe news: the in-game news feed and the official PC Update Notes.\n"
        "   Regenerate with tools/build_news.py. The page refreshes the in-game feed live when online. */\n"
        '"use strict";\nconst NEWS = ' + json.dumps(snapshot, ensure_ascii=False, separators=(",", ":")) + ";\n")
    print(f"{len(items)} news items, {len(notes)} update notes, {OUT.stat().st_size // 1024} KB -> {OUT}")


if __name__ == "__main__":
    main()
