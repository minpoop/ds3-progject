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
#   O  sm2-mesh-probe (the model report) against the synthetic install, and without a game
#
# Then the same for Dark Souls III against a synthetic install (made by the example make_fake_ds3: a program file with two
# throwaway test keys among junk, three encrypted archives with made-up item text and weapon model containers):
#
#   H  ds3-probe: report with key fingerprints (no keys), archives, path table, DCX/BND4 listing and a dry run
#   I  ds3-prepare: writes mod/msg/ENGLISH/item.msgbnd.dcx (a DCX holding the new names) and ashenmarine-msg.json
#   J  a game that renamed the item (ds3-prepare after I): exit code 2, nothing written, the override of I is removed
#   K  no archive key in the program file: exit code 2 and the plain-words message; the same install with --keys: exit code 0
#   L  ds3-prepare started from its own folder, game named in game-folder.txt (the double-click way)
#   M  a mod folder inside the game folder is refused (nothing is ever written there)
#   N  bad command lines for the Dark Souls III commands (exit code 64)
#   R  Data0.bhd is a plain (not encrypted) table of contents and holds the item text: no key and no saved table needed
#   Q  sm2-export-models (the optional model-file copy): exact copies of the two weapons' template files, nothing in the game
#   P  no key anywhere, but the plain tables of contents the running game held were saved (ashenmarine/cache/bhd5): exit code 0
#   S  the item text is stored under a name nobody expects: ds3-prepare finds it by what the files contain (and refuses a text
#      container that is not the English item text)
#   U  ds3-models (the weapon model swap) against both synthetic games: a trial run (report, pictures, candidates; nothing where the
#      game loads it) and an install run (the containers in the mod folder, listed in ashenmarine-models.json); a mod folder inside
#      a game is refused
#   T  ds3-export-models (the optional copy of the five weapon containers): exact copies, a report, nothing in the game; no key:
#      exit code 2 and nothing copied; an output folder inside the game is refused
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

# ======================================================================================================================
# Dark Souls III
# ======================================================================================================================
cargo build -p ashen-setup --example make_fake_ds3 2>&1 | grep -E "^(warning|error)" -A5
FAKE_DS3="$ROOT/target/debug/examples/make_fake_ds3"
D3="$OUT/ds3"
rm -rf "$D3"; mkdir -p "$D3/cwd"
GAME="$D3/DARK SOULS III"
"$FAKE_DS3" "$GAME" --write-keys "$D3/keys.pem" > /dev/null || { echo "cannot build the fake Dark Souls III install"; exit 1; }
"$FAKE_DS3" "$D3/no-key/DARK SOULS III" --no-exe-keys > /dev/null || { echo "cannot build the fake Dark Souls III install (no keys)"; exit 1; }
"$FAKE_DS3" "$D3/renamed/DARK SOULS III" --variant wrong-name > /dev/null || { echo "cannot build the fake Dark Souls III install (renamed)"; exit 1; }
D3_BEFORE="$(treehash "$GAME")"
D3_NOKEY_BEFORE="$(treehash "$D3/no-key/DARK SOULS III")"
D3_RENAMED_BEFORE="$(treehash "$D3/renamed/DARK SOULS III")"
mkdir -p "$D3/kit"; cp "$BIN/ashenmarine-setup.exe" "$D3/kit/"
# wine_ds3 <exe> <name> <args...>: runs the Windows program from the folder $D3/cwd; output in $D3/<name>.out, exit code in $D3/<name>.rc
wine_ds3() {
  local exe="$1" name="$2"; shift 2
  (cd "$D3/cwd" && WINEDEBUG=-all timeout 180 xvfb-run -a "$WINE" "$exe" "$@" > "$D3/$name.out" 2>&1; echo $? > "$D3/$name.rc")
}
# the output of a run with the line breaks of the console taken out, to look for a phrase
flat_out() { tr '\n' ' ' < "$1" | tr -s ' '; }
# ds3_manifest_ok <mod folder>: ashenmarine-msg.json has the documented shape and keys in the documented order
ds3_manifest_ok() {
  python3 -I - "$1" <<'PY'
import json, os, sys
mod = sys.argv[1]
m = json.load(open(os.path.join(mod, "ashenmarine-msg.json")))
assert list(m) == ["format", "tool", "source", "edits", "written"], list(m)
assert m["format"] == 1 and m["tool"].startswith("ashenmarine-setup "), m
assert list(m["source"]) == ["archive", "sha256_of_decoded_original"], m["source"]
assert m["source"]["archive"] == "Data0.bhd" and len(m["source"]["sha256_of_decoded_original"]) == 64, m["source"]
assert [(e["id"], e["old"], e["new"]) for e in m["edits"]] == [(2000000, "Shortsword", "Chainsword"), (14090000, "Avelyn", "Bolt Pistol"), (404000, "Standard Bolt", "Bolt Rounds")], m["edits"]
assert m["written"] == "msg/ENGLISH/item.msgbnd.dcx" and os.path.isfile(os.path.join(mod, m["written"])), m["written"]
PY
}
# ds3_override_ok <file>: a DCX (zlib, checksum verified here by python) holding a BND4 with the new names and without the old ones
ds3_override_ok() {
  python3 -I - "$1" <<'PY'
import struct, sys, zlib
b = open(sys.argv[1], "rb").read()
assert b[:4] == b"DCX\0" and b[0x28:0x2C] == b"DFLT" and b[0x44:0x48] == b"DCA\0", b[:8]
usize, csize = struct.unpack(">II", b[0x1C:0x24])
assert len(b) == 0x4C + csize and b[0x4C] == 0x78, (len(b), csize)
data = zlib.decompress(b[0x4C:0x4C + csize])
assert len(data) == usize and data[:4] == b"BND4", (len(data), usize)
u16 = lambda s: s.encode("utf-16-le")
for new in ("Chainsword", "Bolt Pistol", "Bolt Rounds", "A roaring chain-toothed blade of the Adeptus Astartes."):
    assert u16(new) in data, new
for old in ("Shortsword", "Avelyn", "Standard Bolt"):
    assert u16(old) not in data, old
assert u16("Light Crossbow") in data and u16("Repeating Crossbow") in data, "the other test weapons keep their names"
PY
}

echo; echo "== H: ds3-probe under Wine against the synthetic Dark Souls III install =="
wine_ds3 "$BIN/ashenmarine-setup.exe" h ds3-probe --ds3 "$(winpath "$GAME")" --out "$(winpath "$D3/out-h")"
sed 's/^/    /' "$D3/h.out" | head -40
check "H exit code 0" '[ "$(cat "$D3/h.rc")" = "0" ]'
check "H the install is byte-identical afterwards" '[ "$(treehash "$GAME")" = "$D3_BEFORE" ]'
check "H the report lists the keys by fingerprint and the archives" 'grep -q "archive keys found in the program file: 2 (87febfc8, b2969406)" "$D3/out-h/ds3-report.txt" && grep -q "key 87febfc8" "$D3/out-h/ds3-report.txt" && grep -q "key b2969406" "$D3/out-h/ds3-report.txt"'
check "H the report has the path table with the item text" 'grep -q "^  /msg/ENGLISH/item.msgbnd.dcx .*hash 50b424bf" "$D3/out-h/ds3-report.txt" && grep -q "9 of 50 paths exist" "$D3/out-h/ds3-report.txt"'
check "H the report lists the item text container and the texts at the test ids" 'grep -q "DCX variant DCX_DFLT_10000_44_9" "$D3/out-h/ds3-report.txt" && grep -q "id 2000000: WeaponName.fmg \"Shortsword\"" "$D3/out-h/ds3-report.txt"'
check "H the dry run passed" 'grep -q "\[ok\] the edits: 3 edits" "$D3/out-h/ds3-report.txt" && ! grep -q "FAILED\|PROBLEM\|CRASHED" "$D3/out-h/ds3-report.txt"'
check "H the report lists the weapon model containers" 'grep -q "wp_a_0200.flver" "$D3/out-h/ds3-report.txt" && grep -q "wp_a_1409.hkx" "$D3/out-h/ds3-report.txt"'
check "H the report describes the model and texture files inside the weapon containers" 'grep -q "wp_a_0200.flver: .* bytes; model with 2 meshes, 2 materials, 3 bones, 1 dummies; written again it is byte-identical to the game" "$D3/out-h/ds3-report.txt" && grep -q "wp_a_1409.tpf: .* bytes; 2 textures \[wp_a_9999_a, wp_a_9999_n\]; written again it is byte-identical" "$D3/out-h/ds3-report.txt" && grep -q "layout 0: 28 bytes per vertex: Position Float3, Normal Byte4C" "$D3/out-h/ds3-report.txt" && ! grep -q "NOT READABLE\|DIFFERS" "$D3/out-h/ds3-report.txt"'
check "H the report has no key and no folder name" '! grep -q "BEGIN RSA\|BEGIN PUBLIC" "$D3/out-h/ds3-report.txt" && ! grep -qi "wine-test\|/home/\|Z:" "$D3/out-h/ds3-report.txt"'
check "H only the report was written" '[ "$(find "$D3/out-h" -type f | wc -l)" = "1" ] && [ -f "$D3/out-h/ds3-report.txt" ]'

echo; echo "== I: ds3-prepare under Wine against the synthetic Dark Souls III install =="
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" i ds3-prepare --ds3 "$(winpath "$GAME")" --mod "$(winpath "$D3/mod")" --out "$(winpath "$D3/out-i")"
sed 's/^/    /' "$D3/i.out" | tail -22
check "I exit code 0" '[ "$(cat "$D3/i.rc")" = "0" ]'
check "I the install is byte-identical afterwards" '[ "$(treehash "$GAME")" = "$D3_BEFORE" ]'
check "I exactly the override and the manifest are in the mod folder" '[ "$(find "$D3/mod" -type f | wc -l)" = "2" ] && [ -f "$D3/mod/msg/ENGLISH/item.msgbnd.dcx" ] && [ -f "$D3/mod/ashenmarine-msg.json" ]'
check "I the manifest has the documented shape" 'ds3_manifest_ok "$D3/mod"'
check "I the override is a DCX holding the new names and not the old ones" 'ds3_override_ok "$D3/mod/msg/ENGLISH/item.msgbnd.dcx"'
check "I the console says it only reads the game" 'grep -q "only READS your Dark Souls III files" "$D3/i.out"'
check "I the report is in the report folder" 'grep -q "wrote msg.ENGLISH.item.msgbnd.dcx" "$D3/out-i/ds3-report.txt"'

echo; echo "== J: the game renamed an item -> exit code 2, nothing written, the override of I is removed =="
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" j ds3-prepare --ds3 "$(winpath "$D3/renamed/DARK SOULS III")" --mod "$(winpath "$D3/mod")" --out "$(winpath "$D3/out-j")"
sed 's/^/    /' "$D3/j.out" | tail -14
check "J exit code 2" '[ "$(cat "$D3/j.rc")" = "2" ]'
check "J the stale override and its manifest are gone" '[ ! -e "$D3/mod/msg/ENGLISH/item.msgbnd.dcx" ] && [ ! -e "$D3/mod/ashenmarine-msg.json" ]'
check "J says why in plain words" 'flat_out "$D3/j.out" | grep -q "no longer has \"Shortsword\" at id 2000000"'
check "J says nothing was changed" 'flat_out "$D3/j.out" | grep -q "Nothing was changed: the item names in the game stay as Dark Souls III has them"'
check "J the renamed install is untouched" '[ "$(treehash "$D3/renamed/DARK SOULS III")" = "$D3_RENAMED_BEFORE" ]'

echo; echo "== K: no archive key in the program file -> exit code 2; with --keys -> exit code 0 =="
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" k ds3-prepare --ds3 "$(winpath "$D3/no-key/DARK SOULS III")" --mod "$(winpath "$D3/mod-k")" --out "$(winpath "$D3/out-k")"
sed 's/^/    /' "$D3/k.out" | tail -10
check "K exit code 2" '[ "$(cat "$D3/k.rc")" = "2" ]'
check "K says what to do" 'flat_out "$D3/k.out" | grep -q "has not collected what it needs to read Dark Souls III.s archives yet" && flat_out "$D3/k.out" | grep -q "start the game once with Play-AshenMarine.bat and quit it again"'
check "K nothing was written (no mod folder at all)" '[ ! -e "$D3/mod-k" ]'
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" k2 ds3-prepare --ds3 "$(winpath "$D3/no-key/DARK SOULS III")" --keys "$(winpath "$D3/keys.pem")" --mod "$(winpath "$D3/mod-k")" --out "$(winpath "$D3/out-k2")"
check "K2 with --keys: exit code 0" '[ "$(cat "$D3/k2.rc")" = "0" ]'
check "K2 with --keys: the override is right" 'ds3_manifest_ok "$D3/mod-k" && ds3_override_ok "$D3/mod-k/msg/ENGLISH/item.msgbnd.dcx"'
check "K2 the report names the key file and not the keys" 'grep -q "keys from the key file keys.pem: 2 (87febfc8, b2969406)" "$D3/out-k2/ds3-report.txt" && ! grep -q "BEGIN RSA" "$D3/out-k2/ds3-report.txt"'
check "K the install without keys in its program file is untouched" '[ "$(treehash "$D3/no-key/DARK SOULS III")" = "$D3_NOKEY_BEFORE" ]'

echo; echo "== L: ds3-prepare started from its own folder, game named in game-folder.txt =="
mkdir -p "$D3/kit-l" "$D3/cwd-l"
cp "$BIN/ashenmarine-setup.exe" "$D3/kit-l/"
printf '"%s"\r\nthis second line is ignored\r\n' "$(winpath "$GAME")" > "$D3/kit-l/game-folder.txt"
(cd "$D3/cwd-l" && WINEDEBUG=-all timeout 180 xvfb-run -a "$WINE" "$(winpath "$D3/kit-l/ashenmarine-setup.exe")" ds3-prepare > "$D3/l.out" 2>&1; echo $? > "$D3/l.rc")
sed 's/^/    /' "$D3/l.out" | tail -8
check "L exit code 0" '[ "$(cat "$D3/l.rc")" = "0" ]'
check "L the override, the manifest and the report are written next to the program" 'ds3_manifest_ok "$D3/kit-l/mod" && ds3_override_ok "$D3/kit-l/mod/msg/ENGLISH/item.msgbnd.dcx" && [ -f "$D3/kit-l/ds3-prepare/ds3-report.txt" ]'
check "L nothing is written in the working folder" '[ -z "$(ls -A "$D3/cwd-l")" ]'
check "L the install is still byte-identical" '[ "$(treehash "$GAME")" = "$D3_BEFORE" ]'
check "L the override is the same as the one K2 made from the same archives" 'cmp -s "$D3/kit-l/mod/msg/ENGLISH/item.msgbnd.dcx" "$D3/mod-k/msg/ENGLISH/item.msgbnd.dcx"'

echo; echo "== M: a mod folder inside the game folder is refused =="
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" m ds3-prepare --ds3 "$(winpath "$GAME")" --mod "$(winpath "$GAME/Game/mod")" --out "$(winpath "$D3/out-m")"
sed 's/^/    /' "$D3/m.out" | head -8
check "M exit code 2" '[ "$(cat "$D3/m.rc")" = "2" ]'
check "M says why" 'flat_out "$D3/m.out" | grep -q "is inside your Dark Souls III folder"'
check "M the install is still byte-identical (nothing was created in it)" '[ "$(treehash "$GAME")" = "$D3_BEFORE" ]'
check "M no report was written either" '[ ! -e "$D3/out-m" ]'

echo; echo "== N: bad command lines for the Dark Souls III commands -> exit code 64 =="
for args in "ds3-prepare --bogus" "ds3-probe --mod x" "ds3-prepare --sm2 x" "ds3-prepare --keys"; do
  # shellcheck disable=SC2086
  wine_ds3 "$BIN/ashenmarine-setup.exe" n $args
  check "N '$args' gives exit code 64 and shows how to use it" '[ "$(cat "$D3/n.rc")" = "64" ] && grep -q "ashenmarine-setup ds3-prepare" "$D3/n.out"'
done

echo; echo "== P: tables of contents saved from the running game replace the keys =="
mkdir -p "$D3/kit-p"; cp "$BIN/ashenmarine-setup.exe" "$D3/kit-p/"
"$FAKE_DS3" "$D3/memory-only/DARK SOULS III" --no-exe-keys --save-headers "$D3/kit-p/cache/bhd5" > /dev/null || { echo "cannot build the fake Dark Souls III install (memory only)"; exit 1; }
D3_MEM_BEFORE="$(treehash "$D3/memory-only/DARK SOULS III")"
wine_ds3 "$(winpath "$D3/kit-p/ashenmarine-setup.exe")" p ds3-prepare --ds3 "$(winpath "$D3/memory-only/DARK SOULS III")" --mod "$(winpath "$D3/mod-p")" --out "$(winpath "$D3/out-p")"
sed 's/^/    /' "$D3/p.out" | sed -n 8,24p
check "P exit code 0" '[ "$(cat "$D3/p.rc")" = "0" ]'
check "P the report says where the archives' tables came from" 'grep -q "tables of contents saved from the running game (cache.bhd5): 3 (DLC1.bin, Data0.bin, Data1.bin)" "$D3/out-p/ds3-report.txt" && grep -q "header saved from the running game (Data0.bin)" "$D3/out-p/ds3-report.txt" && ! grep -q "PROBLEM" "$D3/out-p/ds3-report.txt"'
check "P the override is right" 'ds3_manifest_ok "$D3/mod-p" && ds3_override_ok "$D3/mod-p/msg/ENGLISH/item.msgbnd.dcx"'
check "P it is the same file the keys gave" 'cmp -s "$D3/mod-p/msg/ENGLISH/item.msgbnd.dcx" "$D3/mod-k/msg/ENGLISH/item.msgbnd.dcx"'
check "P the install is untouched" '[ "$(treehash "$D3/memory-only/DARK SOULS III")" = "$D3_MEM_BEFORE" ]'
rm -rf "$D3/kit-p/cache"
wine_ds3 "$(winpath "$D3/kit-p/ashenmarine-setup.exe")" p2 ds3-prepare --ds3 "$(winpath "$D3/memory-only/DARK SOULS III")" --mod "$(winpath "$D3/mod-p2")" --out "$(winpath "$D3/out-p2")"
check "P2 without the saved tables: exit code 2, nothing written, the plain-words hint" '[ "$(cat "$D3/p2.rc")" = "2" ] && [ ! -e "$D3/mod-p2" ] && flat_out "$D3/p2.out" | grep -q "start the game once with Play-AshenMarine.bat"'

echo; echo "== R: a plain Data0.bhd that holds the item text needs no key =="
"$FAKE_DS3" "$D3/plain-data0/DARK SOULS III" --no-exe-keys --plain-data0 > /dev/null || { echo "cannot build the fake Dark Souls III install (plain Data0)"; exit 1; }
D3_PLAIN_BEFORE="$(treehash "$D3/plain-data0/DARK SOULS III")"
mkdir -p "$D3/kit-r"; cp "$BIN/ashenmarine-setup.exe" "$D3/kit-r/"
wine_ds3 "$(winpath "$D3/kit-r/ashenmarine-setup.exe")" r ds3-prepare --ds3 "$(winpath "$D3/plain-data0/DARK SOULS III")" --mod "$(winpath "$D3/mod-r")" --out "$(winpath "$D3/out-r")"
sed 's/^/    /' "$D3/r.out" | sed -n 8,26p
check "R exit code 0" '[ "$(cat "$D3/r.rc")" = "0" ]'
check "R the report says Data0 was opened as a plain header and what the others need" 'grep -q "Data0 .*plain header (the .bhd is not encrypted)" "$D3/out-r/ds3-report.txt" && grep -q "NOTE: 2 of 3 archives could not be opened" "$D3/out-r/ds3-report.txt" && grep -q "whole 256-byte blocks" "$D3/out-r/ds3-report.txt"'
check "R the override is right and the same file the keys gave" 'ds3_manifest_ok "$D3/mod-r" && ds3_override_ok "$D3/mod-r/msg/ENGLISH/item.msgbnd.dcx" && cmp -s "$D3/mod-r/msg/ENGLISH/item.msgbnd.dcx" "$D3/mod-k/msg/ENGLISH/item.msgbnd.dcx"'
check "R the install is untouched" '[ "$(treehash "$D3/plain-data0/DARK SOULS III")" = "$D3_PLAIN_BEFORE" ]'
# a failed prepare writes the diagnosis (path table) into the same report
"$FAKE_DS3" "$D3/no-item/DARK SOULS III" --variant no-item > /dev/null || { echo "cannot build the fake Dark Souls III install (no item text)"; exit 1; }
wine_ds3 "$(winpath "$D3/kit-r/ashenmarine-setup.exe")" r2 ds3-prepare --ds3 "$(winpath "$D3/no-item/DARK SOULS III")" --mod "$(winpath "$D3/mod-r2")" --out "$(winpath "$D3/out-r2")"
check "R2 a missing item text: exit code 2, nothing written, and the report carries the path table for the diagnosis" '[ "$(cat "$D3/r2.rc")" = "2" ] && [ ! -e "$D3/mod-r2" ] && grep -q "Where the files are (for the diagnosis)" "$D3/out-r2/ds3-report.txt" && grep -q "/msg/ENGLISH/item.msgbnd.dcx .*hash 50b424bf" "$D3/out-r2/ds3-report.txt"'

echo; echo "== S: an item text stored under a name nobody expects is found by what it contains =="
"$FAKE_DS3" "$D3/unexpected/DARK SOULS III" --variant unexpected-path > /dev/null || { echo "cannot build the fake Dark Souls III install (unexpected path)"; exit 1; }
D3_UNEXP_BEFORE="$(treehash "$D3/unexpected/DARK SOULS III")"
wine_ds3 "$(winpath "$D3/kit-r/ashenmarine-setup.exe")" s ds3-prepare --ds3 "$(winpath "$D3/unexpected/DARK SOULS III")" --mod "$(winpath "$D3/mod-s")" --out "$(winpath "$D3/out-s")"
sed 's/^/    /' "$D3/s.out" | sed -n 8,30p
check "S exit code 0" '[ "$(cat "$D3/s.rc")" = "0" ]'
check "S the report says it looked at the files and recognised the text by its content" 'grep -q "Looking at the start of every file in the archives" "$D3/out-s/ds3-report.txt" && grep -q "recognised by its text" "$D3/out-s/ds3-report.txt" && ! grep -q "PROBLEM" "$D3/out-s/ds3-report.txt"'
check "S the override is right" 'ds3_manifest_ok "$D3/mod-s" && ds3_override_ok "$D3/mod-s/msg/ENGLISH/item.msgbnd.dcx"'
check "S the install is untouched" '[ "$(treehash "$D3/unexpected/DARK SOULS III")" = "$D3_UNEXP_BEFORE" ]'
"$FAKE_DS3" "$D3/unexpected-wrong/DARK SOULS III" --variant unexpected-wrong-name > /dev/null || { echo "cannot build the fake Dark Souls III install (unexpected path, wrong name)"; exit 1; }
wine_ds3 "$(winpath "$D3/kit-r/ashenmarine-setup.exe")" s2 ds3-prepare --ds3 "$(winpath "$D3/unexpected-wrong/DARK SOULS III")" --mod "$(winpath "$D3/mod-s2")" --out "$(winpath "$D3/out-s2")"
check "S2 a text container that is not the English item text: exit code 2 and nothing written" '[ "$(cat "$D3/s2.rc")" = "2" ] && [ ! -e "$D3/mod-s2" ] && grep -q "has the English item text" "$D3/out-s2/ds3-report.txt"'

echo; echo "== T: ds3-export-models under Wine against the synthetic Dark Souls III install =="
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" t ds3-export-models --ds3 "$(winpath "$GAME")" --out "$(winpath "$D3/out-t")"
sed 's/^/    /' "$D3/t.out" | tail -16
check "T exit code 0" '[ "$(cat "$D3/t.rc")" = "0" ]'
check "T the install is byte-identical afterwards" '[ "$(treehash "$GAME")" = "$D3_BEFORE" ]'
check "T the five weapon containers and the report were written, nothing else" '[ "$(find "$D3/out-t" -type f | wc -l)" = "6" ] && [ -f "$D3/out-t/wp_a_0200.partsbnd.dcx" ] && [ -f "$D3/out-t/wp_a_1409.partsbnd.dcx" ] && [ -f "$D3/out-t/ds3-export-report.txt" ]'
check "T each copy is a DCX file as the game stores it (and the two weapons differ)" '[ "$(head -c 4 "$D3/out-t/wp_a_0200.partsbnd.dcx")" = "DCX" ] && ! cmp -s "$D3/out-t/wp_a_0200.partsbnd.dcx" "$D3/out-t/wp_a_1409.partsbnd.dcx"'
check "T the report lists the copies and the files inside" 'grep -q "Copied 5 files" "$D3/out-t/ds3-export-report.txt" && grep -q "inside: wp_a_0200.flver" "$D3/out-t/ds3-export-report.txt" && ! grep -q "PROBLEM\|not copied\|BEGIN RSA" "$D3/out-t/ds3-export-report.txt"'
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" t2 ds3-export-models --ds3 "$(winpath "$D3/no-key/DARK SOULS III")" --out "$(winpath "$D3/out-t2")"
check "T2 no key: exit code 2, nothing copied" '[ "$(cat "$D3/t2.rc")" = "2" ] && [ ! -e "$D3/out-t2/wp_a_0200.partsbnd.dcx" ] && grep -q "Nothing was copied" "$D3/out-t2/ds3-export-report.txt"'
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" t3 ds3-export-models --ds3 "$(winpath "$GAME")" --out "$(winpath "$GAME/Game/copies")"
check "T3 an output folder inside the game is refused (exit code 2) and nothing is created there" '[ "$(cat "$D3/t3.rc")" = "2" ] && flat_out "$D3/t3.out" | grep -q "is inside your Dark Souls III folder" && [ ! -e "$GAME/Game/copies" ] && [ "$(treehash "$GAME")" = "$D3_BEFORE" ]'

echo; echo "== U: ds3-models under Wine against both synthetic games =="
"$ROOT/target/debug/examples/make_fake_sm2" "$D3/sm2-weapons/Space Marine 2" --weapons > /dev/null || { echo "cannot build the fake Space Marine 2 with weapons"; exit 1; }
D3_SM2W_BEFORE="$(treehash "$D3/sm2-weapons/Space Marine 2")"
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" u ds3-models --ds3 "$(winpath "$GAME")" --sm2 "$(winpath "$D3/sm2-weapons/Space Marine 2")" --mod "$(winpath "$D3/mod-u")" --out "$(winpath "$D3/out-u")"
sed 's/^/    /' "$D3/u.out" | tail -14
check "U exit code 0" '[ "$(cat "$D3/u.rc")" = "0" ]'
check "U both games are byte-identical afterwards" '[ "$(treehash "$GAME")" = "$D3_BEFORE" ] && [ "$(treehash "$D3/sm2-weapons/Space Marine 2")" = "$D3_SM2W_BEFORE" ]'
check "U a trial run puts nothing where the game loads it" '[ ! -e "$D3/mod-u" ]'
check "U the report, the pictures and the candidates are in the report folder" '[ -f "$D3/out-u/ds3-models-report.txt" ] && [ -f "$D3/out-u/wp_a_0200-overlay.png" ] && [ -f "$D3/out-u/candidates/wp_a_0200.partsbnd.dcx" ] && [ -f "$D3/out-u/candidates/wp_a_1409.partsbnd.dcx" ] && [ "$(head -c 4 "$D3/out-u/candidates/wp_a_1409.partsbnd.dcx")" = "DCX" ] && [ "$(head -c 4 "$D3/out-u/wp_a_1409-overlay.png" | tail -c 3)" = "PNG" ]'
check "U the report says it is a trial run and every check passed" 'grep -q "TRIAL RUN" "$D3/out-u/ds3-models-report.txt" && grep -q "the model written again is byte-identical to the game" "$D3/out-u/ds3-models-report.txt" && grep -q "check: packed as DCX" "$D3/out-u/ds3-models-report.txt" && grep -q "3 model file(s) made, 0 not made" "$D3/out-u/ds3-models-report.txt" && ! grep -q "PROBLEM\|NOT DONE\|BEGIN RSA" "$D3/out-u/ds3-models-report.txt"'
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" u2 ds3-models --install --ds3 "$(winpath "$GAME")" --sm2 "$(winpath "$D3/sm2-weapons/Space Marine 2")" --mod "$(winpath "$D3/mod-u")" --out "$(winpath "$D3/out-u2")"
check "U2 an install run: exit code 0, the containers are in mod/parts and listed" '[ "$(cat "$D3/u2.rc")" = "0" ] && [ -f "$D3/mod-u/parts/wp_a_0200.partsbnd.dcx" ] && [ -f "$D3/mod-u/parts/wp_a_1409.partsbnd.dcx" ] && [ -f "$D3/mod-u/ashenmarine-models.json" ] && cmp -s "$D3/mod-u/parts/wp_a_0200.partsbnd.dcx" "$D3/out-u/candidates/wp_a_0200.partsbnd.dcx"'
check "U2 the manifest lists each file with its size and hash" 'python3 -I - "$D3/mod-u" <<PY
import hashlib, json, os, sys
mod = sys.argv[1]
m = json.load(open(os.path.join(mod, "ashenmarine-models.json")))
assert m["format"] == 1 and len(m["files"]) == 3, m
for f in m["files"]:
    b = open(os.path.join(mod, f["path"]), "rb").read()
    assert f["bytes"] == len(b) and f["sha256"] == hashlib.sha256(b).hexdigest(), f
PY'
wine_ds3 "$(winpath "$D3/kit/ashenmarine-setup.exe")" u3 ds3-models --install --ds3 "$(winpath "$GAME")" --sm2 "$(winpath "$D3/sm2-weapons/Space Marine 2")" --mod "$(winpath "$GAME/Game/mods")" --out "$(winpath "$D3/out-u3")"
check "U3 a mod folder inside the game is refused (exit code 2) and nothing is created there" '[ "$(cat "$D3/u3.rc")" = "2" ] && flat_out "$D3/u3.out" | grep -q "is inside a game" && [ ! -e "$GAME/Game/mods" ] && [ "$(treehash "$GAME")" = "$D3_BEFORE" ]'

echo; echo "== O: sm2-mesh-probe under Wine against the synthetic Space Marine 2 =="
(cd "$OUT" && WINEDEBUG=-all timeout 120 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" sm2-mesh-probe --sm2 "$(winpath "$OUT/Space Marine 2")" --out "$(winpath "$OUT/out-o")" > "$OUT/run-o.out" 2>&1; echo $? > "$OUT/run-o.rc")
sed 's/^/    /' "$OUT/run-o.out" | head -24
check "O exit code 0" '[ "$(cat "$OUT/run-o.rc")" = "0" ]'
check "O the report describes the chainsword template folder" 'grep -q "template folder  tpl/wpn_chainsword_01.tpl" "$OUT/out-o/mesh-report.txt" && grep -q "Model files of the bolt_pistol" "$OUT/out-o/mesh-report.txt"'
check "O the report has no failed or crashed step" '! grep -q "STEP FAILED\|STEP CRASHED" "$OUT/out-o/mesh-report.txt"'
check "O the install is byte-identical afterwards" '[ "$(treehash "$OUT/Space Marine 2")" = "$BEFORE" ]'
(cd "$OUT" && WINEDEBUG=-all timeout 60 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" sm2-mesh-probe --sm2 "$(winpath "$OUT/empty")" --out "$(winpath "$OUT/out-o2")" > "$OUT/run-o2.out" 2>&1; echo $? > "$OUT/run-o2.rc")
check "O without a game: exit code 2 and a plain message" '[ "$(cat "$OUT/run-o2.rc")" = "2" ] && grep -q "PROBLEM" "$OUT/out-o2/mesh-report.txt"'

echo; echo "== Q: sm2-export-models under Wine against the synthetic Space Marine 2 =="
(cd "$OUT" && WINEDEBUG=-all timeout 120 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" sm2-export-models --sm2 "$(winpath "$OUT/Space Marine 2")" --out "$(winpath "$OUT/out-q")" > "$OUT/run-q.out" 2>&1; echo $? > "$OUT/run-q.rc")
sed 's/^/    /' "$OUT/run-q.out" | head -16
check "Q exit code 0" '[ "$(cat "$OUT/run-q.rc")" = "0" ]'
check "Q the template files of both weapons were copied" 'ls "$OUT"/out-q/wpn_chainsword_01.tpl/* >/dev/null 2>&1 && ls "$OUT"/out-q/wpn_bolt_pistol_01.tpl/* >/dev/null 2>&1 && [ -f "$OUT/out-q/export-report.txt" ]'
check "Q the report lists them with hashes and has no failed step" 'grep -q "template folder  tpl/wpn_chainsword_01.tpl" "$OUT/out-q/export-report.txt" && grep -q "sha256 " "$OUT/out-q/export-report.txt" && ! grep -q "STEP FAILED\|STEP CRASHED\|PROBLEM" "$OUT/out-q/export-report.txt"'
check "Q the install is byte-identical afterwards" '[ "$(treehash "$OUT/Space Marine 2")" = "$BEFORE" ]'
(cd "$OUT" && WINEDEBUG=-all timeout 60 xvfb-run -a "$WINE" "$BIN/ashenmarine-setup.exe" sm2-export-models --sm2 "$(winpath "$OUT/Space Marine 2")" --out "$(winpath "$OUT/Space Marine 2/client_pc/copies")" > "$OUT/run-q2.out" 2>&1; echo $? > "$OUT/run-q2.rc")
check "Q2 an output folder inside the game is refused (exit code 2) and nothing is created there" '[ "$(cat "$OUT/run-q2.rc")" = "2" ] && grep -q "is inside your Space Marine 2 folder" "$OUT/run-q2.out" && [ ! -e "$OUT/Space Marine 2/client_pc/copies" ] && [ "$(treehash "$OUT/Space Marine 2")" = "$BEFORE" ]'

echo; if [ "$FAILS" -eq 0 ]; then echo "ALL SETUP TESTS PASSED"; else echo "$FAILS CHECK(S) FAILED"; fi
exit "$FAILS"
