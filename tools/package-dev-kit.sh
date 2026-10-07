#!/usr/bin/env bash
# Build a PRIVATE test kit for the user's PC (not a Melty release): our launcher + hook DLL, a trimmed copy of
# ModEngine2 (MIT), one-click run/log scripts and plain instructions.
#
#   ME2_DIR=/path/to/ModEngine-2.1.0.0-win64 tools/package-dev-kit.sh
#
# Output: dist/AshenMarine-dev-<version>/  and  dist/AshenMarine-dev-<version>.zip  (+ SHA-256 list)
set -euo pipefail
cd "$(dirname "$0")/.."
ME2_DIR="${ME2_DIR:?set ME2_DIR to the extracted ModEngine-2.1.0.0-win64 folder}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
BIN="target/x86_64-pc-windows-gnu/release"
NAME="AshenMarine-dev-$VERSION"
OUT="dist/$NAME"

python3 tools/preflight.py --milestone 1 >/dev/null || { echo "preflight is not clean: fix the sheets first"; exit 1; }
python3 tools/gen.py --check
cargo build --release --target x86_64-pc-windows-gnu -p ashen-launcher -p ashen-hook
for f in ashenmarine-launcher.exe ashenmarine_hook.dll; do [ -f "$BIN/$f" ] || { echo "missing $BIN/$f"; exit 1; }; done

rm -rf "$OUT" "dist/$NAME.zip"
mkdir -p "$OUT/ashenmarine" "$OUT/modengine2/modengine2/bin" "$OUT/modengine2/modengine2/crashpad" "$OUT/modengine2/modengine2/tools/scyllahide"

cp "$BIN/ashenmarine-launcher.exe" "$BIN/ashenmarine_hook.dll" "$OUT/ashenmarine/"
cp packaging/dev-kit/Play-AshenMarine.bat packaging/dev-kit/Send-Logs.bat packaging/dev-kit/README-FIRST.txt "$OUT/"
cp THIRD_PARTY_NOTICES.md "$OUT/THIRD_PARTY_NOTICES.txt"

# ModEngine2: only what it needs to run (no debug-menu assets, no developer headers)
cp "$ME2_DIR/modengine2_launcher.exe" "$OUT/modengine2/"
cp "$ME2_DIR"/modengine2/bin/* "$OUT/modengine2/modengine2/bin/"
cp "$ME2_DIR"/modengine2/crashpad/* "$OUT/modengine2/modengine2/crashpad/"
for f in HookLibraryx64.dll InjectorCLIx64.exe scylla_hide.ini; do cp "$ME2_DIR/modengine2/tools/scyllahide/$f" "$OUT/modengine2/modengine2/tools/scyllahide/"; done
cp packaging/third-party/ModEngine2-LICENSE-MIT.txt "$OUT/modengine2/LICENSE-MIT.txt"

( cd "$OUT" && find . -type f \( -name '*.exe' -o -name '*.dll' \) -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS.txt )

python3 -I - "$OUT" "dist/$NAME.zip" "$NAME" <<'EOF'
import os, sys, zipfile
src, dst, top = sys.argv[1:4]
with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for root, _, files in os.walk(src):
        for f in sorted(files):
            p = os.path.join(root, f)
            z.write(p, os.path.join(top, os.path.relpath(p, src)))
EOF
echo; echo "== $NAME.zip =="; ls -l "dist/$NAME.zip"; sha256sum "dist/$NAME.zip"
echo; echo "== contents hashes =="; cat "$OUT/SHA256SUMS.txt"
