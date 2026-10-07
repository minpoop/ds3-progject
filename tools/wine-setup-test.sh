#!/usr/bin/env bash
# Runs the shipped Windows build of ashenmarine-setup.exe under Wine against a synthetic Space Marine 2 install
# (built from scratch, no game file) and checks the report, the pictures, the sample sounds and that the install
# is byte-for-byte untouched.
#
#   tools/wine-setup-test.sh            # needs: wine64, xvfb-run, mingw-w64, rust target x86_64-pc-windows-gnu
set -u
cd "$(dirname "$0")/.."
ROOT="$PWD"
OUT="${ASHEN_TEST_ROOT:-$ROOT/target/wine-test}/setup"
BIN="$ROOT/target/x86_64-pc-windows-gnu/release"
WINE="${WINE:-/usr/lib/wine/wine64}"
FAILS=0
winpath() { printf 'Z:%s' "${1//\//\\}"; }
ok()   { echo "  PASS $1"; }
bad()  { echo "  FAIL $1"; FAILS=$((FAILS + 1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1  [$2]"; fi; }
treehash() { (cd "$1" 2>/dev/null && find . -type f -print0 | sort -z | xargs -0 -r sha256sum | sha256sum) || echo none; }

echo "== build =="
cargo build --release --target x86_64-pc-windows-gnu --workspace 2>&1 | grep -E "^(warning|error)" -A5
[ -f "$BIN/ashenmarine-setup.exe" ] || { echo "build failed"; exit 1; }
cargo build -p ashen-setup --example make_fake_sm2 2>&1 | grep -E "^(warning|error)" -A5

rm -rf "$OUT"; mkdir -p "$OUT"
"$ROOT/target/debug/examples/make_fake_sm2" "$OUT/Space Marine 2" > /dev/null || { echo "cannot build the fake install"; exit 1; }
BEFORE="$(treehash "$OUT/Space Marine 2")"

echo; echo "== A: probe under Wine against the synthetic install =="
(cd "$OUT" && WINEDEBUG=-all timeout 120 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" probe --sm2 "$(winpath "$OUT/Space Marine 2")" --out "$(winpath "$OUT/out")" > "$OUT/run.out" 2>&1; echo $? > "$OUT/run.rc")
sed 's/^/    /' "$OUT/run.out" | head -60
check "A exit code 0" '[ "$(cat "$OUT/run.rc")" = "0" ]'
check "A the install is byte-identical afterwards" '[ "$(treehash "$OUT/Space Marine 2")" = "$BEFORE" ]'
check "A report names the weapon bank and both events" 'grep -q "weapon bank sounds/desktop/wpn.bnk" "$OUT/out/probe-report.txt" && grep -q "play_chainsword_swing" "$OUT/out/probe-report.txt" && grep -q "play_bolt_pistol_fire" "$OUT/out/probe-report.txt"'
check "A report has no failed or crashed step" '! grep -q "STEP FAILED\|STEP CRASHED" "$OUT/out/probe-report.txt"'
check "A texture preview PNG was written and is a PNG" '[ "$(head -c 8 "$OUT/out/textures/wpn_chainsword_01_d_0.png" | od -An -c | tr -d " \n")" = "211PNG\r\n032\n" ] || head -c 4 "$OUT/out/textures/wpn_chainsword_01_d_0.png" | grep -q PNG'
check "A two sample sounds decoded to .wav" '[ "$(ls "$OUT"/out/sounds/*.wav 2>/dev/null | wc -l)" = "2" ]'

echo; echo "== B: no Space Marine 2 -> clear message, exit code 2 =="
mkdir -p "$OUT/empty"
(cd "$OUT" && WINEDEBUG=-all timeout 60 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" probe --sm2 "$(winpath "$OUT/empty")" --out "$(winpath "$OUT/out-b")" > "$OUT/run-b.out" 2>&1; echo $? > "$OUT/run-b.rc")
check "B exit code 2" '[ "$(cat "$OUT/run-b.rc")" = "2" ]'
check "B says what is wrong" 'grep -q "no .pak files found" "$OUT/out-b/probe-report.txt"'

echo; if [ "$FAILS" -eq 0 ]; then echo "ALL SETUP TESTS PASSED"; else echo "$FAILS CHECK(S) FAILED"; fi
exit "$FAILS"
