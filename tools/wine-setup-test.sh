#!/usr/bin/env bash
# Runs the shipped Windows build of ashenmarine-setup.exe under Wine against a synthetic Space Marine 2 install
# (built from scratch, no game file) and checks the report, the pictures, the sample sounds, the prepared .wav files
# and that the install is byte-for-byte untouched.
#
#   A  probe against the synthetic install        B  probe without a game
#   C  prepare against the synthetic install      D  prepare without a game (exit code 2, nothing written)
#   E  prepare started from its own folder, game named in sm2-folder.txt (the double-click way)
#   F  bad command line (exit code 64)
#   G  an assets folder inside the game folder is refused (nothing is ever written there)
#
#   tools/wine-setup-test.sh            # needs: wine64, xvfb-run, mingw-w64, rust target x86_64-pc-windows-gnu, python3
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
# wavs_ok <folder> <count>: exactly <count> .wav files, each starting with RIFF....WAVE
wavs_ok() {
  local n=0 f
  for f in "$1"/*.wav; do
    [ -f "$f" ] || return 1
    [ "$(head -c 4 "$f")" = "RIFF" ] && [ "$(dd if="$f" bs=1 skip=8 count=4 2>/dev/null)" = "WAVE" ] || return 1
    n=$((n + 1))
  done
  [ "$n" = "$2" ]
}
# The synthetic install can make four of the sheet's sounds and holds 3 versions of chainsword_swing_1 and 1 of each of
# the others; the sheet (design/sheets/sounds.json) says how many takes are wanted, so the number of files that must come
# out is worked out from it.
SHEET="$ROOT/design/sheets/sounds.json"
EXPECT_FILES="$(python3 -I - "$SHEET" <<'PY'
import json, sys
rows = {r["id"]: r for r in json.load(open(sys.argv[1]))["rows"]}
versions = {"chainsword_swing_1": 3, "chainsword_swing_2": 1, "chainsword_idle": 1, "boltpistol_fire": 1}
print(sum(min(rows[slot]["takes"], n) for slot, n in versions.items()))
PY
)"
# prepared_ok <assets folder>: index.json and ready.json have the documented shape and agree with the files and the sheet
prepared_ok() {
  python3 -I - "$1" "$SHEET" <<'PY'
import json, os, sys
assets = sys.argv[1]
rows = {r["id"]: r for r in json.load(open(sys.argv[2]))["rows"]}
idx = json.load(open(os.path.join(assets, "sounds", "index.json")))
ready = json.load(open(os.path.join(assets, "ready.json")))
assert idx["format"] == 1 and idx["source"].startswith("Space Marine 2 "), idx
assert list(idx) == ["format", "source", "sounds"], list(idx)
slots = {s["slot"]: s for s in idx["sounds"]}
assert set(slots) == {"chainsword_swing_1", "chainsword_swing_2", "chainsword_idle", "boltpistol_fire"}, sorted(slots)
for s in idx["sounds"]:
    assert list(s) == ["slot", "event", "loop", "files", "seconds"], list(s)
    assert isinstance(s["loop"], bool) and len(s["files"]) == len(s["seconds"]) >= 1, s
for slot, s in slots.items():
    assert s["loop"] is rows[slot]["looped"] and s["event"] == rows[slot]["sm2_event"], (slot, s)
assert slots["chainsword_idle"]["loop"] is True and slots["chainsword_swing_1"]["loop"] is False
assert len(slots["chainsword_swing_1"]["files"]) == min(rows["chainsword_swing_1"]["takes"], 3), slots["chainsword_swing_1"]
files = [f for s in idx["sounds"] for f in s["files"]]
assert all(os.path.isfile(os.path.join(assets, "sounds", f)) for f in files), files
assert ready["format"] == 1 and ready["tool"].startswith("ashenmarine-setup "), ready
assert ready["slots"] == len(idx["sounds"]) and ready["files"] == len(files), (ready, len(files))
PY
}

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
check "A report names the weapon bank and both events" 'grep -q "weapon bank sounds/desktop/wpn.bnk" "$OUT/out/probe-report.txt" && grep -q "wpn_melee_chainsword_swing" "$OUT/out/probe-report.txt" && grep -q "wpn_firearm_shoot_2d_bolt_pistol" "$OUT/out/probe-report.txt"'
check "A report has no failed or crashed step" '! grep -q "STEP FAILED\|STEP CRASHED" "$OUT/out/probe-report.txt"'
check "A texture preview PNG was written and is a PNG" 'head -c 4 "$OUT/out/textures/wpn_chainsword_01.png" | grep -q PNG'
check "A the sound clips of the weapon events were kept" '[ "$(ls "$OUT"/out/samples/*.wem 2>/dev/null | wc -l)" = "2" ]'

echo; echo "== B: no Space Marine 2 -> clear message, exit code 2 =="
mkdir -p "$OUT/empty"
(cd "$OUT" && WINEDEBUG=-all timeout 60 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" probe --sm2 "$(winpath "$OUT/empty")" --out "$(winpath "$OUT/out-b")" > "$OUT/run-b.out" 2>&1; echo $? > "$OUT/run-b.rc")
check "B exit code 2" '[ "$(cat "$OUT/run-b.rc")" = "2" ]'
check "B says what is wrong" 'grep -q "no .pak files found" "$OUT/out-b/probe-report.txt"'

echo; echo "== C: prepare under Wine against the synthetic install =="
(cd "$OUT" && WINEDEBUG=-all timeout 180 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" prepare --sm2 "$(winpath "$OUT/Space Marine 2")" --out "$(winpath "$OUT/assets")" > "$OUT/run-c.out" 2>&1; echo $? > "$OUT/run-c.rc")
sed 's/^/    /' "$OUT/run-c.out" | head -70
check "C exit code 0" '[ "$(cat "$OUT/run-c.rc")" = "0" ]'
check "C the install is byte-identical afterwards" '[ "$(treehash "$OUT/Space Marine 2")" = "$BEFORE" ]'
check "C $EXPECT_FILES .wav files, each RIFF/WAVE" 'wavs_ok "$OUT/assets/sounds" "$EXPECT_FILES"'
check "C index.json and ready.json have the documented shape and match the files" 'prepared_ok "$OUT/assets"'
check "C ready.json is the last file written" '[ -z "$(find "$OUT/assets" -type f -newer "$OUT/assets/ready.json")" ]'
check "C the report is next to the assets folder and names the slots" 'grep -q "chainsword_swing_1" "$OUT/prepare-sm2/prepare-report.txt" && grep -q "boltpistol_fire" "$OUT/prepare-sm2/prepare-report.txt" && grep -q "^prepared 4 of [0-9]* slots, $EXPECT_FILES files" "$OUT/prepare-sm2/prepare-report.txt"'
check "C the report says what could not be made, and why" 'grep -q "could NOT be prepared" "$OUT/prepare-sm2/prepare-report.txt" && grep -q "has no event called" "$OUT/prepare-sm2/prepare-report.txt"'
check "C the report shows what an event reaches" 'grep -q "layer container 211: plays all of its 2 children" "$OUT/prepare-sm2/prepare-report.txt"'
check "C the console says it only reads the game" 'grep -q "only READS your Space Marine 2 files" "$OUT/run-c.out"'

echo; echo "== D: prepare without a Space Marine 2 -> clear message, exit code 2, nothing written =="
(cd "$OUT" && WINEDEBUG=-all timeout 60 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" prepare --sm2 "$(winpath "$OUT/empty")" --out "$(winpath "$OUT/d/assets")" > "$OUT/run-d.out" 2>&1; echo $? > "$OUT/run-d.rc")
sed 's/^/    /' "$OUT/run-d.out" | head -30
check "D exit code 2" '[ "$(cat "$OUT/run-d.rc")" = "2" ]'
check "D no ready.json (no assets folder at all)" '[ ! -e "$OUT/d/assets" ]'
check "D says what is wrong and what to do" 'grep -q "no .pak files found" "$OUT/d/prepare-sm2/prepare-report.txt" && grep -q "send me the report" "$OUT/d/prepare-sm2/prepare-report.txt"'

echo; echo "== E: prepare started from its own folder, game named in sm2-folder.txt =="
mkdir -p "$OUT/kit" "$OUT/cwd-e"
cp "$BIN/ashenmarine-setup.exe" "$OUT/kit/"
printf '"%s"\r\nthis second line is ignored\r\n' "$(winpath "$OUT/Space Marine 2")" > "$OUT/kit/sm2-folder.txt"
(cd "$OUT/cwd-e" && WINEDEBUG=-all timeout 180 xvfb-run -a "$WINE" "$(winpath "$OUT/kit/ashenmarine-setup.exe")" prepare > "$OUT/run-e.out" 2>&1; echo $? > "$OUT/run-e.rc")
sed 's/^/    /' "$OUT/run-e.out" | head -12
check "E exit code 0" '[ "$(cat "$OUT/run-e.rc")" = "0" ]'
check "E sounds and markers are written next to the program" 'wavs_ok "$OUT/kit/assets/sounds" "$EXPECT_FILES" && prepared_ok "$OUT/kit/assets" && [ -f "$OUT/kit/prepare-sm2/prepare-report.txt" ]'
check "E nothing is written in the working folder" '[ -z "$(ls -A "$OUT/cwd-e")" ]'
check "E the install is still byte-identical" '[ "$(treehash "$OUT/Space Marine 2")" = "$BEFORE" ]'
check "E the sounds are the same as in C" '[ "$(treehash "$OUT/kit/assets")" = "$(treehash "$OUT/assets")" ]'

echo; echo "== F: bad command line -> exit code 64 =="
(cd "$OUT" && WINEDEBUG=-all timeout 60 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" prepare --bogus > "$OUT/run-f.out" 2>&1; echo $? > "$OUT/run-f.rc")
check "F exit code 64" '[ "$(cat "$OUT/run-f.rc")" = "64" ]'
check "F shows how to use it" 'grep -q "ashenmarine-setup prepare" "$OUT/run-f.out"'

echo; echo "== G: an assets folder inside the game folder is refused =="
(cd "$OUT" && WINEDEBUG=-all timeout 60 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" prepare --sm2 "$(winpath "$OUT/Space Marine 2")" --out "$(winpath "$OUT/Space Marine 2/assets")" > "$OUT/run-g.out" 2>&1; echo $? > "$OUT/run-g.rc")
sed 's/^/    /' "$OUT/run-g.out" | head -12
check "G exit code 2" '[ "$(cat "$OUT/run-g.rc")" = "2" ]'
check "G says why" 'tr "\n" " " < "$OUT/run-g.out" | tr -s " " | grep -q "is inside your Space Marine 2 folder"'
check "G the install is still byte-identical (nothing was created in it)" '[ "$(treehash "$OUT/Space Marine 2")" = "$BEFORE" ]'

echo; if [ "$FAILS" -eq 0 ]; then echo "ALL SETUP TESTS PASSED"; else echo "$FAILS CHECK(S) FAILED"; fi
exit "$FAILS"
