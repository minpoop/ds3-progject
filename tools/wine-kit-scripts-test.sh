#!/usr/bin/env bash
# Runs the click-through scripts of the PACKAGED test kit (Prepare-AshenMarine.bat, Remove-Models.bat) under Wine's cmd.exe,
# against synthetic installs of both games, with the real programs of the kit. It is the closest thing to double-clicking them
# on a PC that can be done here: it catches batch-file syntax mistakes, a wrong order of steps and a wrong exit-code test.
#
#   P  Prepare: takes over the key files of an older kit folder, makes the sounds and the names, writes the reports, builds
#      the weapon models, asks whether to put them in the game (the question times out to "yes") and does so; the mod
#      folder then holds the models and their list, and neither game was touched
#   R  Remove-Models: takes the models out again (and only them)
#   N  Prepare against a Dark Souls III whose archives cannot be opened (no keys anywhere, no older kit folder beside it): the
#      models step says it made nothing, asks nothing, puts nothing in the game
#
# Needs the packaged kit: run tools/package-dev-kit.sh first (it is part of the usual run).
set -u
cd "$(dirname "$0")/.."
ROOT="$PWD"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
KIT="$ROOT/dist/AshenMarine-dev-$VERSION"
OUT="${ASHEN_TEST_ROOT:-$ROOT/target/wine-test}/kit-scripts"
WINE="${WINE:-/usr/lib/wine/wine64}"
FAILS=0
winpath() { printf 'Z:%s' "${1//\//\\}"; }
ok()   { echo "  PASS $1"; }
bad()  { echo "  FAIL $1"; FAILS=$((FAILS + 1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1  [$2]"; fi; }
treehash() { (cd "$1" 2>/dev/null && find . -type f -print0 | sort -z | xargs -0 -r sha256sum | sha256sum) || echo none; }
flat() { tr '\r\n' '  ' < "$1" | tr -s ' '; }

[ -d "$KIT" ] || { echo "no packaged kit at $KIT: run tools/package-dev-kit.sh first"; exit 1; }
cargo build -p ashen-setup --example make_fake_sm2 --example make_fake_ds3 2>&1 | grep -E "^(warning|error)" -A5
FAKE_SM2="$ROOT/target/debug/examples/make_fake_sm2"
FAKE_DS3="$ROOT/target/debug/examples/make_fake_ds3"

rm -rf "$OUT"; mkdir -p "$OUT/games"
"$FAKE_SM2" "$OUT/games/Space Marine 2" --weapons > /dev/null || { echo "cannot build the fake Space Marine 2"; exit 1; }
"$FAKE_DS3" "$OUT/games/DARK SOULS III" --no-exe-keys --write-keys "$OUT/keys.pem" > /dev/null || { echo "cannot build the fake Dark Souls III"; exit 1; }
"$FAKE_DS3" "$OUT/games-nokeys/DARK SOULS III" --no-exe-keys > /dev/null || { echo "cannot build the fake Dark Souls III (no keys)"; exit 1; }
SM2_BEFORE="$(treehash "$OUT/games/Space Marine 2")"
DS3_BEFORE="$(treehash "$OUT/games/DARK SOULS III")"

# a new kit folder next to an older one that has the key files (this is how the owner's folders sit in Downloads)
new_kit() { # <name> <ds3 folder> [downloads folder name]
  local k="$OUT/${3:-downloads}/$1"
  rm -rf "$k"; mkdir -p "$k"
  cp -r "$KIT"/. "$k"/
  printf '%s\n' "$(winpath "$2")" > "$k/ashenmarine/game-folder.txt"
  printf '%s\n' "$(winpath "$OUT/games/Space Marine 2")" > "$k/ashenmarine/sm2-folder.txt"
}
run_bat() { # <kit folder> <name> <bat> -> output in $OUT/<name>.out, exit code in $OUT/<name>.rc
  local k="$1" name="$2" bat="$3"
  ( cd "$k" && yes '' 2>/dev/null | WINEDEBUG=-all timeout 400 xvfb-run -a "$WINE" cmd /c "$bat" > "$OUT/$name.out" 2>&1; echo "${PIPESTATUS[1]}" > "$OUT/$name.rc" )
}

echo; echo "== P: Prepare-AshenMarine.bat =="
mkdir -p "$OUT/downloads/AshenMarine-old/ashenmarine/cache"
cp "$OUT/keys.pem" "$OUT/downloads/AshenMarine-old/ashenmarine/cache/ds3-keys.pem"
cp "$OUT/keys.pem" "$OUT/downloads/AshenMarine-old/ashenmarine/cache/keys-seen.pem"
new_kit AshenMarine-new "$OUT/games/DARK SOULS III"
K="$OUT/downloads/AshenMarine-new"
run_bat "$K" p "Prepare-AshenMarine.bat"
sed 's/\r$//' "$OUT/p.out" | cut -c1-200 | grep -v "^$" | head -60
check "P the script ran to its end" 'grep -q "Next: double-click Play-AshenMarine.bat" "$OUT/p.out"'
check "P both key files of the older kit folder were taken over" 'grep -q "Took over ds3-keys.pem of an older kit folder" "$OUT/p.out" && grep -q "Took over keys-seen.pem of an older kit folder" "$OUT/p.out" && [ -f "$K/ashenmarine/cache/ds3-keys.pem" ] && [ -f "$K/ashenmarine/cache/keys-seen.pem" ]'
check "P the five steps all ran" 'for s in "1 of 5: Space Marine 2 sounds" "2 of 5: Dark Souls III item names" "3 of 5: where Dark Souls III keeps its files" "4 of 5: model reports" "5 of 5: the new weapon models"; do grep -q "$s" "$OUT/p.out" || exit 1; done'
check "P the sounds, the names and the reports were made" '[ -f "$K/ashenmarine/assets/sounds/index.json" ] && [ -f "$K/ashenmarine/mod/msg/engus/item_dlc2.msgbnd.dcx" ] && [ -f "$K/ashenmarine/ds3-probe/ds3-report.txt" ] && [ -f "$K/ashenmarine/probe-sm2-mesh/mesh-report.txt" ]'
# (Wine's choice.exe prints no prompt, so the question itself cannot be seen here; that the script goes on to install shows that
# the choice command was accepted and answered "yes", which is what it does by itself after 30 seconds on a PC)
check "P the trial passed, the script said so and went on to install" 'flat "$OUT/p.out" | grep -q "The new models passed every check" && flat "$OUT/p.out" | grep -q "The finished models are put in the mod folder"'
check "P the models were put in the game: the files and their list are in the mod folder" '[ -f "$K/ashenmarine/mod/parts/wp_a_0200.partsbnd.dcx" ] && [ -f "$K/ashenmarine/mod/parts/wp_a_1409.partsbnd.dcx" ] && [ -f "$K/ashenmarine/mod/ashenmarine-models.json" ]'
check "P neither game was changed" '[ "$(treehash "$OUT/games/Space Marine 2")" = "$SM2_BEFORE" ] && [ "$(treehash "$OUT/games/DARK SOULS III")" = "$DS3_BEFORE" ]'

echo; echo "== R: Remove-Models.bat =="
touch "$K/ashenmarine/mod/parts/mine.txt"
run_bat "$K" r "Remove-Models.bat"
sed 's/\r$//' "$OUT/r.out" | cut -c1-200 | grep -v "^$" | head -12
check "R the models and their list are gone, the player's own file and the name file are not" '[ ! -e "$K/ashenmarine/mod/parts/wp_a_0200.partsbnd.dcx" ] && [ ! -e "$K/ashenmarine/mod/parts/wp_a_1409.partsbnd.dcx" ] && [ ! -e "$K/ashenmarine/mod/ashenmarine-models.json" ] && [ -f "$K/ashenmarine/mod/parts/mine.txt" ] && [ -f "$K/ashenmarine/mod/msg/engus/item_dlc2.msgbnd.dcx" ]'
check "R it says what it did" 'flat "$OUT/r.out" | grep -q "Removed 3 file(s)"'
check "R neither game was changed" '[ "$(treehash "$OUT/games/Space Marine 2")" = "$SM2_BEFORE" ] && [ "$(treehash "$OUT/games/DARK SOULS III")" = "$DS3_BEFORE" ]'

echo; echo "== N: Prepare-AshenMarine.bat against a game whose archives cannot be opened =="
new_kit AshenMarine-nokeys "$OUT/games-nokeys/DARK SOULS III" downloads-nokeys
K2="$OUT/downloads-nokeys/AshenMarine-nokeys"
run_bat "$K2" n "Prepare-AshenMarine.bat"
sed 's/\r$//' "$OUT/n.out" | cut -c1-200 | grep -v "^$" | tail -12
check "N the script ran to its end" 'grep -q "Next: double-click Play-AshenMarine.bat" "$OUT/n.out"'
check "N it says no model was made, asks nothing and puts nothing in the game" 'flat "$OUT/n.out" | grep -q "No new model was made" && ! flat "$OUT/n.out" | grep -q "Put the new models into the game now" && [ ! -e "$K2/ashenmarine/mod/parts" ]'

echo; if [ "$FAILS" = "0" ]; then echo "ALL KIT SCRIPT CHECKS PASSED"; else echo "$FAILS KIT SCRIPT CHECK(S) FAILED"; fi
exit "$FAILS"
