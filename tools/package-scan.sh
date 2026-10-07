#!/usr/bin/env bash
# Builds dist/AshenMarine-scan.zip (part 1) and dist/AshenMarine-scan2.zip (part 2): a double-click .bat plus the
# PowerShell script, with Windows line endings.
set -euo pipefail
cd "$(dirname "$0")/.."
build() { # <zip name> <bat> <ps1>
  local name="$1" bat="$2" ps1="$3" out="dist/$1"
  rm -rf "$out" "dist/$name.zip"; mkdir -p "$out"
  for f in "$bat" "$ps1"; do sed 's/\r$//; s/$/\r/' "$f" > "$out/$(basename "$f")"; done
  python3 -I - "$out" "dist/$name.zip" "$name" <<'PY'
import os, sys, zipfile
src, dst, top = sys.argv[1:4]
with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for f in sorted(os.listdir(src)):
        z.write(os.path.join(src, f), top + "/" + f)
PY
  ls -l "dist/$name.zip"; sha256sum "dist/$name.zip"
}
build AshenMarine-scan  packaging/scan/Run-Scan.bat  tools/scan-sm2.ps1
build AshenMarine-scan2 packaging/scan/Run-Scan2.bat tools/scan2.ps1
