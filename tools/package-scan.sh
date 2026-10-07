#!/usr/bin/env bash
# Builds dist/AshenMarine-scan.zip: Run-Scan.bat (double-click) + scan-sm2.ps1, with Windows line endings.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=dist/AshenMarine-scan
rm -rf "$OUT" dist/AshenMarine-scan.zip; mkdir -p "$OUT"
for f in packaging/scan/Run-Scan.bat tools/scan-sm2.ps1; do sed 's/\r$//; s/$/\r/' "$f" > "$OUT/$(basename "$f")"; done
python3 -I - "$OUT" dist/AshenMarine-scan.zip <<'PY'
import os, sys, zipfile
src, dst = sys.argv[1:3]
with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for f in sorted(os.listdir(src)):
        z.write(os.path.join(src, f), "AshenMarine-scan/" + f)
PY
ls -l dist/AshenMarine-scan.zip; sha256sum dist/AshenMarine-scan.zip
