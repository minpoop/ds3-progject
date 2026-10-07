#!/usr/bin/env bash
# End-to-end test of the launcher under Wine. The harness stands in for ModEngine2's launcher and for Dark
# Souls III; everything else (launcher, hook DLL, configs, backups, verification) is the real code.
#
#   E  happy path: the "game" plays; real save byte-identical, private copy updated, title marked, network refused
#   H  second Play reuses the private copy (the character persists) and the real save is still untouched
#   F  protection silently fails (game never loads the hook): the launcher detects the change and restores it
#   G  the hook closes the game (private save vanished): the launcher reports why; real save untouched
#   I  Dark Souls III already running: the launcher refuses to start
set -u
cd "$(dirname "$0")/.."
ROOT="$PWD"
OUT="${ASHEN_TEST_ROOT:-$ROOT/target/wine-test}/launcher"
BIN="$ROOT/target/x86_64-pc-windows-gnu/release"
WINE="${WINE:-/usr/lib/wine/wine64}"
FAILS=0
winpath() { printf 'Z:%s' "${1//\//\\}"; }
ok()   { echo "  PASS $1"; }
bad()  { echo "  FAIL $1"; FAILS=$((FAILS + 1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1  [$2]"; fi; }
treehash() { (cd "$1" 2>/dev/null && find . -type f -print0 | sort -z | xargs -0 -r sha256sum | sha256sum) || echo none; }

echo "== build =="
python3 tools/gen.py --check || exit 1
cargo build --release --target x86_64-pc-windows-gnu -p ashen-launcher -p ashen-hook -p ashen-harness 2>&1 | grep -E "^(warning|error)" -A5
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
        --game "$(winpath "$w/game")" --me2 "$(winpath "$w/me2")" --appdata "$(winpath "$w/appdata")" --silent > "$w/launcher.out" 2>&1; echo $? > "$w/launcher.rc" )
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
check "J the first launcher finished normally" '[ "$(cat "$W/first.rc")" = "0" ]'
check "J real save untouched" '[ "$(treehash "$W/appdata")" = "$REAL0" ]'

echo; echo "== K: the packaged kit layout with the REAL (trimmed) ModEngine2 =="
KITSRC="$(ls -d "$ROOT"/dist/AshenMarine-dev-* 2>/dev/null | grep -v '\.zip$' | head -1)"
if [ -z "$KITSRC" ]; then
  echo "  SKIP K: no dist/AshenMarine-dev-* (run tools/package-dev-kit.sh first)"
else
  make_world K; REAL0="$(treehash "$W/appdata")"
  rm -rf "$W/kit" "$W/ashen" "$W/me2"; cp -r "$KITSRC" "$W/kit"
  cp "$BIN/ashenmarine-launcher.exe" "$BIN/ashenmarine_hook.dll" "$W/kit/ashenmarine/"      # latest code, kit's ModEngine2 files
  ( cd "$W" && ASHEN_FAKE_BEHAVIOR=sleep ASHEN_FAKE_REAL="$(winpath "$W/appdata/DarkSoulsIII")" WINEDEBUG=-all timeout 150 xvfb-run -a "$WINE" "$W/kit/ashenmarine/ashenmarine-launcher.exe" \
      --game "$(winpath "$W/game")" --appdata "$(winpath "$W/appdata")" --silent > "$W/launcher.out" 2>&1; echo $? > "$W/launcher.rc" )
  sed 's/^/    /' "$W/kit/ashenmarine/logs/launcher.log" | tail -14; echo "    -- hook.log:"; sed 's/^/    /' "$W/kit/ashenmarine/logs/hook.log" | cut -c1-220 | head -12
  check "K launcher exit code 0" '[ "$(cat "$W/launcher.rc")" = "0" ]'
  check "K the kit's ModEngine2 was found by default (no --me2 given)" 'grep -q "ModEngine2:" "$W/kit/ashenmarine/logs/launcher.log"'
  check "K the REAL ModEngine2 loaded our hook DLL (hook reports READY)" 'grep -q "READY:" "$W/kit/ashenmarine/logs/hook.log"'
  check "K both in-process self-tests passed" 'grep -q "redirect self-test passed" "$W/kit/ashenmarine/logs/hook.log" && grep -q "network self-test passed" "$W/kit/ashenmarine/logs/hook.log"'
  check "K ModEngine2's own log shows it loaded the external DLL" 'grep -rq "Loaded external DLL" "$W/kit/modengine2/modengine2/logs/"'
  check "K real save byte-identical" '[ "$(treehash "$W/appdata")" = "$REAL0" ]'
  check "K launcher log says VERIFIED" 'grep -q "VERIFIED" "$W/kit/ashenmarine/logs/launcher.log"'
fi

echo; if [ "$FAILS" -eq 0 ]; then echo "ALL LAUNCHER TESTS PASSED"; else echo "$FAILS CHECK(S) FAILED"; fi
exit "$FAILS"
