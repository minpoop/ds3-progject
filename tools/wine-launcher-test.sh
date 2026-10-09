#!/usr/bin/env bash
# End-to-end test of the launcher under Wine. The harness stands in for ModEngine2's launcher and for Dark
# Souls III; everything else (launcher, hook DLL, configs, backups, verification) is the real code.
#
#   E  happy path: the "game" plays; real save byte-identical, private copy updated, title marked, network refused
#   H  second Play reuses the private copy (the character persists) and the real save is still untouched
#   F  protection silently fails (game never loads the hook): the launcher detects the change and restores it
#   G  the hook closes the game (private save vanished): the launcher reports why; real save untouched
#   I  Dark Souls III already running: the launcher refuses to start
#   L  --probe: the hook's read-only game probe starts, copes with a game it does not know, and scans for text tables
#   M4 --rename: the new names are written over the old ones in the game's memory (a stand-in game that keeps name strings the way the real one does)
set -u
cd "$(dirname "$0")/.."
ROOT="$PWD"
OUT="${ASHEN_TEST_ROOT:-$ROOT/target/wine-test}/launcher"
BIN="$ROOT/target/x86_64-pc-windows-gnu/release"
WINE="${WINE:-/usr/lib/wine/wine64}"
FAILS=0
LAUNCHER_ARGS=""
winpath() { printf 'Z:%s' "${1//\//\\}"; }
ok()   { echo "  PASS $1"; }
bad()  { echo "  FAIL $1"; FAILS=$((FAILS + 1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1  [$2]"; fi; }
treehash() { (cd "$1" 2>/dev/null && find . -type f -print0 | sort -z | xargs -0 -r sha256sum | sha256sum) || echo none; }

echo "== build =="
python3 tools/gen.py --check || exit 1
cargo build --release --target x86_64-pc-windows-gnu --workspace 2>&1 | grep -E "^(warning|error)" -A5
for f in ashenmarine-launcher.exe ashenmarine_hook.dll ashen-harness.exe; do [ -f "$BIN/$f" ] || { echo "missing $f"; exit 1; }; done

make_world() { # <name>
  W="$OUT/$1"; rm -rf "$W"
  mkdir -p "$W/appdata/DarkSoulsIII/76561198000000000" "$W/game/Game" "$W/me2/modengine2/bin" "$W/ashen"
  printf 'MY REAL CHARACTER' > "$W/appdata/DarkSoulsIII/76561198000000000/DS30000.sl2"
  printf '<graphics/>' > "$W/appdata/DarkSoulsIII/GraphicsConfig.xml"
  cp "$BIN/ashen-harness.exe" "$W/game/Game/DarkSoulsIII.exe"
  cp "$BIN/ashen-harness.exe" "$W/me2/modengine2_launcher.exe"
  echo "stub" > "$W/me2/modengine2/bin/modengine2.dll"
  cp "$BIN/ashenmarine-launcher.exe" "$BIN/ashenmarine_hook.dll" "$W/ashen/"
}
launch() { # <world> [env assignments...]  -> runs the launcher, stores rc
  local w="$1"; shift
  ( cd "$w" && env "$@" \
      ASHEN_FAKE_REAL="$(winpath "$w/appdata/DarkSoulsIII")" ASHEN_FAKE_LOG="$(winpath "$w/ashen/logs/hook.log")" ASHEN_FAKE_RESULT="$(winpath "$w/result.txt")" \
      WINEDEBUG=-all timeout 150 xvfb-run -a "$WINE" "$w/ashen/ashenmarine-launcher.exe" \
        --game "$(winpath "$w/game")" --me2 "$(winpath "$w/me2")" --appdata "$(winpath "$w/appdata")" --silent $LAUNCHER_ARGS > "$w/launcher.out" 2>&1; echo $? > "$w/launcher.rc" )
}

echo; echo "== E: happy path =="
make_world E; REAL0="$(treehash "$W/appdata")"
launch "$W"
sed 's/^/    /' "$W/ashen/logs/launcher.log"; echo "    -- result.txt:"; sed 's/^/    /' "$W/result.txt" 2>/dev/null
check "E launcher exit code 0" '[ "$(cat "$W/launcher.rc")" = "0" ]'
check "E real save byte-identical afterwards" '[ "$(treehash "$W/appdata")" = "$REAL0" ]'
check "E the game wrote its save into the private copy" 'grep -q "^PLAYED" "$W/ashen/save/DarkSoulsIII/76561198000000000/DS30000.sl2"'
check "E the private copy started as a copy of the real character (graphics config carried over)" '[ "$(cat "$W/ashen/save/DarkSoulsIII/GraphicsConfig.xml")" = "<graphics/>" ]'
check "E permanent first backup holds the original character" '[ "$(cat "$W/ashen/backups/original/76561198000000000/DS30000.sl2")" = "MY REAL CHARACTER" ]'
check "E a session backup exists" 'ls -d "$W"/ashen/backups/session-* >/dev/null 2>&1'
check "E the game process loaded the hook" 'grep -q "^dll=loaded" "$W/result.txt"'
check "E the window title carries the marker" 'grep -q "title=DARK SOULS III - Ashen Marine (offline copy)" "$W/result.txt"'
check "E the game could not reach the network (WSAECONNREFUSED)" 'grep -q "net_connect_rc=-1 err=10061" "$W/result.txt"'
check "E launcher log says VERIFIED" 'grep -q "VERIFIED" "$W/ashen/logs/launcher.log"'
check "E hook log says READY" 'grep -q "READY:" "$W/ashen/logs/hook.log"'
check "E ModEngine2 config names our DLL" 'grep -q "ashenmarine_hook.dll" "$W/ashen/config/config_ashenmarine.toml"'

echo; echo "== H: second Play reuses the private copy =="
SAVED="$(cat "$W/ashen/save/DarkSoulsIII/76561198000000000/DS30000.sl2")"
launch "$W"
check "H launcher exit code 0" '[ "$(cat "$W/launcher.rc")" = "0" ]'
check "H the private copy was reused, not recreated" 'grep -q "using the existing private save copy" "$W/ashen/logs/launcher.log"'
check "H real save still byte-identical" '[ "$(treehash "$W/appdata")" = "$REAL0" ]'
check "H the character persisted into the second session" 'grep -q "^PLAYED" "$W/ashen/save/DarkSoulsIII/76561198000000000/DS30000.sl2"'

echo; echo "== F: protection silently fails; the launcher must catch it =="
make_world F; REAL0="$(treehash "$W/appdata")"
launch "$W" ASHEN_FAKE_BEHAVIOR=no-dll
sed 's/^/    /' "$W/ashen/logs/launcher.log" | tail -8
check "F launcher exit code 4 (real save changed)" '[ "$(cat "$W/launcher.rc")" = "4" ]'
check "F the real save was restored byte-for-byte" '[ "$(treehash "$W/appdata")" = "$REAL0" ]'
check "F launcher log records the problem" 'grep -q "PROBLEM" "$W/ashen/logs/launcher.log"'

echo; echo "== G: the hook closes the game; the launcher explains =="
make_world G; REAL0="$(treehash "$W/appdata")"
launch "$W" ASHEN_FAKE_SABOTAGE=delete-sandbox
sed 's/^/    /' "$W/ashen/logs/launcher.log" | tail -8
check "G launcher exit code 3 (game closed by the hook)" '[ "$(cat "$W/launcher.rc")" = "3" ]'
check "G the reason was surfaced" 'grep -q "closed Dark Souls III because its protections" "$W/ashen/logs/launcher.log"'
check "G real save untouched" '[ "$(treehash "$W/appdata")" = "$REAL0" ]'

echo; echo "== I: game already running =="
make_world I; REAL0="$(treehash "$W/appdata")"
( cd "$W" && ASHEN_FAKE_BEHAVIOR=sleep WINEDEBUG=-all timeout 60 xvfb-run -a "$WINE" "$W/game/Game/DarkSoulsIII.exe" > /dev/null 2>&1 & )
sleep 6
launch "$W"
check "I launcher refused (exit code 2)" '[ "$(cat "$W/launcher.rc")" = "2" ]'
check "I it said why" 'grep -q "already running" "$W/ashen/logs/launcher.log"'
check "I nothing was created or changed" '[ "$(treehash "$W/appdata")" = "$REAL0" ] && [ ! -d "$W/ashen/save" ]'
sleep 22

echo; echo "== J: pressing Play twice =="
make_world J; REAL0="$(treehash "$W/appdata")"
( launch "$W" ASHEN_FAKE_BEHAVIOR=sleep ; cp "$W/launcher.rc" "$W/first.rc" ) &
sleep 5
cp "$W/ashen/logs/launcher.log" "$W/first-so-far.log" 2>/dev/null
( cd "$W" && WINEDEBUG=-all timeout 60 xvfb-run -a "$WINE" "$W/ashen/ashenmarine-launcher.exe" --game "$(winpath "$W/game")" --me2 "$(winpath "$W/me2")" --appdata "$(winpath "$W/appdata")" --silent > "$W/second.out" 2>&1; echo $? > "$W/second.rc" )
check "J the second launcher refused (exit code 2)" '[ "$(cat "$W/second.rc")" = "2" ]'
check "J it said why" 'grep -q "already starting or running" "$W/ashen/logs/launcher.log"'
wait
# the sleeping stand-in game never loads the hook, so the first launcher rightly reports "no protection ran" (exit code 5)
check "J the first launcher finished and noticed that no protection ran inside that game (exit code 5)" '[ "$(cat "$W/first.rc")" = "5" ]'
check "J real save untouched" '[ "$(treehash "$W/appdata")" = "$REAL0" ]'

echo; echo "== L: --probe (read-only game probe) in a game it does not know =="
make_world L; REAL0="$(treehash "$W/appdata")"
LAUNCHER_ARGS="--probe" launch "$W" ASHEN_FAKE_HOLD_SECS=34
sed 's/^/    /' "$W/ashen/logs/probe-ds3.txt" 2>/dev/null | cut -c1-200 | head -14
check "L launcher exit code 0" '[ "$(cat "$W/launcher.rc")" = "0" ]'
check "L the probe started and said it is read-only" 'grep -q "DS3 probe v.* starting (read-only" "$W/ashen/logs/probe-ds3.txt"'
check "L it noticed it cannot read this game version and did not touch game memory structures" 'grep -q "cannot read the game version" "$W/ashen/logs/probe-ds3.txt" && grep -q "in-game probe skipped" "$W/ashen/logs/probe-ds3.txt"'
check "L the collector started (read-only) and, with no archive in this fake game, had nothing to do" 'grep -q "collector v.* starting (read-only" "$W/ashen/logs/harvest.txt" && grep -q "no .bhd/.bdt archive pair" "$W/ashen/logs/harvest.txt"'
check "L the text-table scan ran and finished" 'grep -q "text-table scan 1" "$W/ashen/logs/probe-ds3.txt" && grep -q "found .* text tables" "$W/ashen/logs/probe-ds3.txt"'
check "L the sandbox still worked (VERIFIED, real save untouched)" 'grep -q "VERIFIED" "$W/ashen/logs/launcher.log" && [ "$(treehash "$W/appdata")" = "$REAL0" ]'
check "L the hook config said probe=true" 'grep -q "\"probe\": true" "$W/ashen/config/ashenmarine.json"'
make_world L2; LAUNCHER_ARGS="" launch "$W" ASHEN_FAKE_HOLD_SECS=0
check "L2 without --probe no probe file is created" '[ ! -e "$W/ashen/logs/probe-ds3.txt" ] && [ ! -e "$W/ashen/logs/harvest.txt" ]'

echo; echo "== M: --sounds --experiments in a game it does not know (the feature loads its sounds, says why it stays off, changes nothing) =="
make_world M; REAL0="$(treehash "$W/appdata")"
mkdir -p "$W/ashen/assets/sounds"
python3 -I - "$W/ashen/assets/sounds" <<'PY'
import json, os, struct, sys
d = sys.argv[1]
data = b''.join(struct.pack('<h', 1000) for _ in range(4410))
wav = b'RIFF' + struct.pack('<I', 36 + len(data)) + b'WAVEfmt ' + struct.pack('<IHHIIHH', 16, 1, 1, 44100, 88200, 2, 16) + b'data' + struct.pack('<I', len(data)) + data
open(os.path.join(d, 'a_1.wav'), 'wb').write(wav)
open(os.path.join(d, 'a_2.wav'), 'wb').write(wav)
json.dump({'format': 1, 'sounds': [{'slot': 'chainsword_swing_1', 'event': 'x', 'loop': False, 'files': ['a_1.wav', 'a_2.wav']}]}, open(os.path.join(d, 'index.json'), 'w'))
PY
LAUNCHER_ARGS="--sounds --experiments" launch "$W" ASHEN_FAKE_HOLD_SECS=6
sed 's/^/    /' "$W/ashen/logs/sfx.txt" 2>/dev/null | cut -c1-220 | head -10
check "M launcher exit code 0" '[ "$(cat "$W/launcher.rc")" = "0" ]'
check "M the hook config switched sounds and experiments on and names the assets folder" 'grep -q "\"sounds\": true" "$W/ashen/config/ashenmarine.json" && grep -q "\"experiments\": true" "$W/ashen/config/ashenmarine.json" && grep -q "assets" "$W/ashen/config/ashenmarine.json"'
check "M the sound feature started and loaded the prepared slot" 'grep -q "sound feature v" "$W/ashen/logs/sfx.txt" && grep -q "loaded 1 sound slots" "$W/ashen/logs/sfx.txt"'
check "M it noticed it cannot read this game version and stayed off" 'grep -q "cannot tell which game build" "$W/ashen/logs/sfx.txt"'
check "M the sandbox still worked (VERIFIED, real save untouched)" 'grep -q "VERIFIED" "$W/ashen/logs/launcher.log" && [ "$(treehash "$W/appdata")" = "$REAL0" ]'
make_world M2; LAUNCHER_ARGS="--sounds" launch "$W" ASHEN_FAKE_HOLD_SECS=4
check "M2 without prepared sounds it says what to do" 'grep -q "Run Prepare-AshenMarine.bat" "$W/ashen/logs/sfx.txt"'
check "M2 the game was not disturbed (exit code 0)" '[ "$(cat "$W/launcher.rc")" = "0" ]'
echo; echo "== M4: --rename: the new names are written over the old ones in the game's memory, in place, and nothing else =="
make_world M4; REAL0="$(treehash "$W/appdata")"
LAUNCHER_ARGS="--rename" launch "$W" ASHEN_FAKE_HOLD_SECS=12 ASHEN_FAKE_PLANT_NAMES=1
sed 's/^/    /' "$W/ashen/logs/rename.txt" 2>/dev/null | cut -c1-220 | head -12
check "M4 launcher exit code 0" '[ "$(cat "$W/launcher.rc")" = "0" ]'
check "M4 the hook config switched rename on" 'grep -q "\"rename\": true" "$W/ashen/config/ashenmarine.json"'
check "M4 the rename feature started and said what it will write" 'grep -q "rename v.* starting" "$W/ashen/logs/rename.txt" && grep -q "will write: .*\"Shortsword\" -> \"Chainsword\"" "$W/ashen/logs/rename.txt"'
check "M4 the game's strings were found and written" 'grep -q "WROTE \"Chainsword\" over \"Shortsword\"" "$W/ashen/logs/rename.txt" && grep -q "WROTE \"Bolter\" over \"Avelyn\"" "$W/ashen/logs/rename.txt" && grep -q "WROTE \"Bolt Rounds\" over \"Standard Bolt\"" "$W/ashen/logs/rename.txt"'
check "M4 the game reads the new names, and only those three changed" 'grep -q "^names_after=Chainsword|Bolter|Bolt Rounds  |Longsword|Shortsword +1$" "$W/result.txt"'
check "M4 the sandbox still worked (VERIFIED, real save untouched)" 'grep -q "VERIFIED" "$W/ashen/logs/launcher.log" && [ "$(treehash "$W/appdata")" = "$REAL0" ]'
make_world M5; LAUNCHER_ARGS="" launch "$W" ASHEN_FAKE_HOLD_SECS=6 ASHEN_FAKE_PLANT_NAMES=1
check "M5 without --rename no log is made and the names are as they were" '[ ! -e "$W/ashen/logs/rename.txt" ] && grep -q "^names_after=Shortsword|Avelyn|Standard Bolt|Longsword|Shortsword +1$" "$W/result.txt"'
make_world M3; LAUNCHER_ARGS="" launch "$W" ASHEN_FAKE_HOLD_SECS=0
check "M3 without --sounds no sound file is created" '[ ! -e "$W/ashen/logs/sfx.txt" ]'

echo; echo "== K: the packaged kit layout with the REAL (trimmed) ModEngine2 =="
KITSRC="$(ls -d "$ROOT"/dist/AshenMarine-dev-* 2>/dev/null | grep -v '\.zip$' | head -1)"
if [ -z "$KITSRC" ]; then
  echo "  SKIP K: no dist/AshenMarine-dev-* (run tools/package-dev-kit.sh first)"
else
  make_world K; REAL0="$(treehash "$W/appdata")"
  rm -rf "$W/kit" "$W/ashen" "$W/me2"; cp -r "$KITSRC" "$W/kit"
  # The kit is tested exactly as shipped, and must contain the very binaries the other scenarios just used.
  check "K the kit ships the binaries that were tested" '[ "$(sha256sum < "$W/kit/ashenmarine/ashenmarine_hook.dll")" = "$(sha256sum < "$BIN/ashenmarine_hook.dll")" ] && [ "$(sha256sum < "$W/kit/ashenmarine/ashenmarine-launcher.exe")" = "$(sha256sum < "$BIN/ashenmarine-launcher.exe")" ]'
  # The whole path a player takes, with the shipped files: prepare (reads a synthetic Space Marine 2) ...
  cargo build -p ashen-setup --example make_fake_sm2 2>&1 | grep -E "^(warning|error)" -A5
  "$ROOT/target/debug/examples/make_fake_sm2" "$W/Space Marine 2" > /dev/null || echo "  (cannot build the fake Space Marine 2)"
  SM2_BEFORE="$(treehash "$W/Space Marine 2")"
  ( cd "$W" && WINEDEBUG=-all timeout 120 xvfb-run -a "$WINE" "$W/kit/ashenmarine/ashenmarine-setup.exe" prepare --sm2 "$(winpath "$W/Space Marine 2")" > "$W/prepare.out" 2>&1; echo $? > "$W/prepare.rc" )
  sed 's/^/    /' "$W/prepare.out" | tail -12
  check "K prepare (shipped exe) succeeded" '[ "$(cat "$W/prepare.rc")" = "0" ] && [ -f "$W/kit/ashenmarine/assets/ready.json" ] && [ -f "$W/kit/ashenmarine/assets/sounds/index.json" ]'
  check "K prepare left the synthetic Space Marine 2 untouched" '[ "$(treehash "$W/Space Marine 2")" = "$SM2_BEFORE" ]'
  # ... then play with the sounds switched on
  ( cd "$W" && ASHEN_FAKE_BEHAVIOR=sleep ASHEN_FAKE_REAL="$(winpath "$W/appdata/DarkSoulsIII")" WINEDEBUG=-all timeout 150 xvfb-run -a "$WINE" "$W/kit/ashenmarine/ashenmarine-launcher.exe" \
      --game "$(winpath "$W/game")" --appdata "$(winpath "$W/appdata")" --silent --probe --sounds > "$W/launcher.out" 2>&1; echo $? > "$W/launcher.rc" )
  sed 's/^/    /' "$W/kit/ashenmarine/logs/launcher.log" | tail -14; echo "    -- hook.log:"; sed 's/^/    /' "$W/kit/ashenmarine/logs/hook.log" | cut -c1-220 | head -12
  check "K launcher exit code 0" '[ "$(cat "$W/launcher.rc")" = "0" ]'
  check "K the kit's ModEngine2 was found by default (no --me2 given)" 'grep -q "ModEngine2:" "$W/kit/ashenmarine/logs/launcher.log"'
  check "K the REAL ModEngine2 loaded our hook DLL (hook reports READY)" 'grep -q "READY:" "$W/kit/ashenmarine/logs/hook.log"'
  check "K both in-process self-tests passed" 'grep -q "redirect self-test passed" "$W/kit/ashenmarine/logs/hook.log" && grep -q "network self-test passed" "$W/kit/ashenmarine/logs/hook.log"'
  check "K ModEngine2's own log shows it loaded the external DLL" 'grep -rq "Loaded external DLL" "$W/kit/modengine2/modengine2/logs/"'
  check "K real save byte-identical" '[ "$(treehash "$W/appdata")" = "$REAL0" ]'
  check "K launcher log says VERIFIED" 'grep -q "VERIFIED" "$W/kit/ashenmarine/logs/launcher.log"'
  check "K the probe thread started inside the real-ModEngine2-loaded hook" 'grep -q "DS3 probe v" "$W/kit/ashenmarine/logs/probe-ds3.txt"'
  check "K the collector thread started inside the real-ModEngine2-loaded hook" 'grep -q "collector v" "$W/kit/ashenmarine/logs/harvest.txt"'
  check "K the sound feature read what prepare made (index.json contract)" 'grep -q "loaded [1-9][0-9]* sound slots" "$W/kit/ashenmarine/logs/sfx.txt"'
  check "K the logs the player sends contain no game audio" '! find "$W/kit/ashenmarine/logs" -name "*.wav" | grep -q .'
fi

echo; if [ "$FAILS" -eq 0 ]; then echo "ALL LAUNCHER TESTS PASSED"; else echo "$FAILS CHECK(S) FAILED"; fi
exit "$FAILS"
