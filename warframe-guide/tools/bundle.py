"""Bundle the guide into one self-contained HTML file.

index.html loads assets/style.css and four scripts. This inlines them all into
tenno-field-manual.html, which you can copy to a phone and open offline
(fonts fall back to built-in ones without a connection).

Run from the warframe-guide folder:  python3 tools/bundle.py
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "tenno-field-manual.html"


def bundle():
    html = (ROOT / "index.html").read_text(encoding="utf-8")
    css = (ROOT / "assets" / "style.css").read_text(encoding="utf-8")
    html = html.replace('<link rel="stylesheet" href="assets/style.css">', "<style>\n" + css + "</style>")
    for name in ["art", "data", "companions", "app"]:
        js = (ROOT / "assets" / f"{name}.js").read_text(encoding="utf-8").replace("</script", "<\\/script")
        html = html.replace(f'<script src="assets/{name}.js"></script>', "<script>\n" + js + "</script>")
    assert "assets/" not in html, "an asset reference was not inlined"
    return html


if __name__ == "__main__":
    OUT.write_text(bundle(), encoding="utf-8")
    print(f"{OUT.name}: {OUT.stat().st_size // 1024} KB")
