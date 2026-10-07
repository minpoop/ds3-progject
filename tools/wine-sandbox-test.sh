#!/usr/bin/env bash
# Runs the hook DLL under Wine (headless) and tries to break out of the sandbox.
#
#   tools/wine-sandbox-test.sh            # needs: wine64, xvfb-run, mingw-w64, rust target x86_64-pc-windows-gnu
#
# Scenarios:
#   A  full probe: every file operation under the REAL save path lands in the private copy; every non-loopback
#      network call is refused; loopback still works; the real save is byte-identical afterwards.
#   B  control run with the network guard switched off: proves the guard is what refuses connections.
#   C  fail closed: private save folder missing  -> the process is terminated and says why.
#   D  fail closed: config missing               -> the process is terminated and says why.
set -u
cd "$(dirname "$0")/.."
ROOT="$PWD"
OUT="${ASHEN_TEST_ROOT:-$ROOT/target/wine-test}"
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
cargo build --release --target x86_64-pc-windows-gnu --workspace 2>&1 | grep -E "^(warning|error)" -A5
[ -f "$BIN/ashenmarine_hook.dll" ] && [ -f "$BIN/ashen-harness.exe" ] || { echo "build failed"; exit 1; }

# make_world <name> <block_network true|false>  -> sets W (world dir)
make_world() {
  W="$OUT/$1"; rm -rf "$W"; mkdir -p "$W/real/DarkSoulsIII/76561198000000000" "$W/data/config" "$W/data/logs" "$W/other"
  printf 'MY REAL CHARACTER' > "$W/real/DarkSoulsIII/76561198000000000/DS30000.sl2"
  printf '<graphics/>' > "$W/real/DarkSoulsIII/GraphicsConfig.xml"
  mkdir -p "$W/data/save" && cp -r "$W/real/DarkSoulsIII" "$W/data/save/DarkSoulsIII"      # the private copy
  cp "$BIN/ashenmarine_hook.dll" "$W/data/"
  python3 -I - "$W" "$2" <<'EOF'
import json, sys
w, block = sys.argv[1], sys.argv[2] == "true"
def win(p): return "Z:" + p.replace("/", "\\")
cfg = {
  "version": "test", "real_save_dir": win(w + "/real/DarkSoulsIII"), "sandbox_save_dir": win(w + "/data/save/DarkSoulsIII"),
  "log_file": win(w + "/data/logs/hook.log"), "window_suffix": " - Ashen Marine (offline copy)", "silent": True, "block_network": block,
}
json.dump(cfg, open(w + "/data/config/ashenmarine.json", "w"), indent=2)
EOF
}

run_probe() { # <world> <block>
  (cd "$1" && WINEDEBUG=-all timeout 120 xvfb-run -a "$WINE" "$BIN/ashen-harness.exe" probe --dll "$(winpath "$1/data/ashenmarine_hook.dll")" --other "$(winpath "$1/other/plain.txt")" --block "$2" > "$1/probe.out" 2>&1; echo $? > "$1/probe.rc")
}

echo; echo "== A: full sandbox probe =="
make_world A true; REAL_BEFORE="$(treehash "$W/real")"
run_probe "$W" true; cat "$W/probe.out" | sed 's/^/    /'
check "A harness exit code 0 (no FAIL lines)" '[ "$(cat "$W/probe.rc")" = "0" ]'
check "A real save folder is byte-identical afterwards" '[ "$(treehash "$W/real")" = "$REAL_BEFORE" ]'
check "A the probe's file landed in the private copy" '[ "$(cat "$W/data/save/DarkSoulsIII/probe-id/DS30000.sl2" 2>/dev/null)" = "hello save" ]'
check "A nothing from the probe appeared in the real folder" '[ ! -e "$W/real/DarkSoulsIII/probe-id" ]'
check "A the original character is still in the private copy" '[ "$(cat "$W/data/save/DarkSoulsIII/76561198000000000/DS30000.sl2")" = "MY REAL CHARACTER" ]'
check "A a file outside the save folder was not redirected" '[ "$(cat "$W/other/plain.txt" 2>/dev/null)" = "plain" ]'
check "A log has READY, REDIRECT and DENY lines" 'grep -q "READY:" "$W/data/logs/hook.log" && grep -q "REDIRECT" "$W/data/logs/hook.log" && grep -q "DENY" "$W/data/logs/hook.log"'
check "A log shows both self-tests passed" 'grep -q "redirect self-test passed" "$W/data/logs/hook.log" && grep -q "network self-test passed" "$W/data/logs/hook.log"'
check "A self-test file was cleaned up" '[ ! -e "$W/data/save/DarkSoulsIII/.ashen-selftest" ]'

echo; echo "== B: control run, network guard switched off =="
make_world B false
run_probe "$W" false; cat "$W/probe.out" | sed 's/^/    /'
check "B harness exit code 0" '[ "$(cat "$W/probe.rc")" = "0" ]'
check "B guard hooks were skipped, so nothing was denied" '! grep -q "DENY" "$W/data/logs/hook.log" && grep -q "block_network=false" "$W/data/logs/hook.log"'

echo; echo "== C: fail closed - private save folder missing =="
make_world C true; rm -rf "$W/data/save"; REAL_BEFORE="$(treehash "$W/real")"
run_probe "$W" true
check "C the process was terminated (non-zero exit)" '[ "$(cat "$W/probe.rc")" != "0" ]'
check "C FATAL.txt explains why" 'grep -q "does not exist" "$W/data/logs/FATAL.txt" 2>/dev/null || grep -q "does not exist" "$W/data/FATAL.txt" 2>/dev/null'
check "C real save untouched" '[ "$(treehash "$W/real")" = "$REAL_BEFORE" ]'

echo; echo "== D: fail closed - config missing =="
make_world D true; rm -f "$W/data/config/ashenmarine.json"; REAL_BEFORE="$(treehash "$W/real")"
run_probe "$W" true
check "D the process was terminated (non-zero exit)" '[ "$(cat "$W/probe.rc")" != "0" ]'
check "D FATAL.txt explains why" 'grep -q "cannot read the config" "$W/data/FATAL.txt" 2>/dev/null'
check "D real save untouched" '[ "$(treehash "$W/real")" = "$REAL_BEFORE" ]'

echo; if [ "$FAILS" -eq 0 ]; then echo "ALL SANDBOX TESTS PASSED"; else echo "$FAILS CHECK(S) FAILED"; fi
exit "$FAILS"
