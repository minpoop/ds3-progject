//! Ashen Marine launcher. Melty runs this when you press Play. It never trusts the protections blindly:
//!
//!   1. refuses to start while Dark Souls III is already running;
//!   2. fingerprints and backs up your REAL save (sheet: save_backup_verify);
//!   3. makes sure the mashup has its own private copy of the save (save_sandbox_copy);
//!   4. writes the hook DLL's config and ModEngine2's config, then starts the game through ModEngine2 (me2_launch);
//!   5. waits for the game to close, then proves your real save is byte-identical, or puts it back.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod platform;

use ashen_common::config::{me2_toml, Features, HookConfig};
use ashen_common::generated::files as f;
use ashen_common::logging::{file_tag, Logger};
use ashen_common::paths::Roots;
use ashen_common::save;
use ashen_common::{steam, DS3_APP_ID, VERSION, WINDOW_SUFFIX};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

const DS3_EXE_NAME: &str = "DarkSoulsIII.exe";
const USAGE: &str = "ashenmarine-launcher [--game <Dark Souls III folder>] [--me2 <ModEngine2 folder>] [--data <folder>] [--appdata <folder>] [--silent] [--probe] [--sounds] [--experiments] [--rename]\n\
  --game     Dark Souls III install folder (default: found through Steam)\n\
  --me2      ModEngine2 folder (default: ..\\modengine2 next to this program)\n\
  --data     where the mashup keeps its save copy, backups and logs (default: next to this program)\n\
  --appdata  override %APPDATA% (testing)\n\
  --silent   no message boxes (testing)\n\
  --probe    private test kits: also write a read-only report about the running game (probe-ds3.txt)\n\
  --sounds   play the Space Marine 2 sounds the setup step prepared (assets\\sounds) for swings and shots\n\
  --experiments  private test kits: hotkey experiments that change the running game (F8 gives the test weapons)\n\
  --rename   private test kits: write the new names of the test weapons over the old ones in the running game's memory (logs\\rename.txt)";

#[derive(Default)]
struct Opts {
    game: Option<PathBuf>,
    me2: Option<PathBuf>,
    data: Option<PathBuf>,
    appdata: Option<PathBuf>,
    silent: bool,
    probe: bool,
    sounds: bool,
    experiments: bool,
    rename: bool,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value\n{USAGE}"));
        match a.as_str() {
            "--game" => o.game = Some(value("--game")?.into()),
            "--me2" => o.me2 = Some(value("--me2")?.into()),
            "--data" => o.data = Some(value("--data")?.into()),
            "--appdata" => o.appdata = Some(value("--appdata")?.into()),
            "--silent" => o.silent = true,
            "--probe" => o.probe = true,
            "--sounds" => o.sounds = true,
            "--experiments" => o.experiments = true,
            "--rename" => o.rename = true,
            "--help" | "-h" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    Ok(o)
}

struct Ctx<'a> {
    log: &'a Logger,
    silent: bool,
}

impl Ctx<'_> {
    fn say(&self, msg: &str) {
        self.log.log(msg);
    }
    fn tell(&self, text: &str, error: bool) {
        self.log.log(&format!("{}: {text}", if error { "ERROR" } else { "NOTE" }));
        if !self.silent {
            platform::message_box("Ashen Marine", text, error);
        }
    }
}

fn wait_for(deadline: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + deadline;
    while Instant::now() < end {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    done()
}

/// Where is Dark Souls III? In order: --game, a `game-folder.txt` next to this program (one line: the folder),
/// Steam's own library list, then the usual default Steam folders.
fn find_ds3(o: &Opts, exe_dir: &Path) -> Option<PathBuf> {
    if let Some(g) = &o.game {
        return Some(g.clone());
    }
    if let Ok(text) = std::fs::read_to_string(exe_dir.join("game-folder.txt")) {
        let line = text.lines().next().unwrap_or("").trim().trim_matches('"');
        if !line.is_empty() {
            return Some(PathBuf::from(line));
        }
    }
    for root in steam::candidate_roots(steam::registry_root()) {
        if let Some((p, _)) = steam::find_game(&steam::libraries(&root), DS3_APP_ID) {
            return Some(p);
        }
        let guess = root.join("steamapps").join("common").join("DARK SOULS III");
        if guess.join("Game").join(DS3_EXE_NAME).is_file() {
            return Some(guess);
        }
    }
    None
}

fn run(o: &Opts, exe_dir: &Path) -> Result<ExitCode, (String, Option<Logger>)> {
    let data = o.data.clone().unwrap_or_else(|| exe_dir.to_path_buf());
    let mut roots = Roots { data: Some(data.clone()), ..Roots::default() };
    let log_path = roots.file(f::LAUNCHER_LOG).map_err(|e| (e, None))?;
    let log = Logger::open(&log_path, "launcher");
    let ctx = Ctx { log: &log, silent: o.silent };
    ctx.say(&format!("Ashen Marine launcher v{VERSION} starting (pid {})", std::process::id()));

    let fail = |msg: String| -> (String, Option<Logger>) { (msg, None) };
    if !platform::single_instance() {
        return Err(fail("Ashen Marine is already starting or running. Wait for it, or close Dark Souls III first.".into()));
    }

    // ---- find everything
    roots.appdata = o.appdata.clone().or_else(platform::known_appdata);
    roots.game = find_ds3(o, exe_dir);
    roots.me2 = Some(o.me2.clone().unwrap_or_else(|| exe_dir.join("..").join("modengine2")));
    let ds3_exe = roots.file(f::DS3_EXE).map_err(|_| fail("Dark Souls III was not found. Install it through Steam, or put its folder (the one that contains Game\\DarkSoulsIII.exe) on the first line of a file called game-folder.txt next to ashenmarine-launcher.exe.".into()))?;
    if !ds3_exe.is_file() {
        return Err(fail(format!("Dark Souls III was not found at {} (expected Game\\DarkSoulsIII.exe there).", roots.game.as_ref().unwrap().display())));
    }
    let me2_launcher = roots.file(f::ME2_LAUNCHER).map_err(fail)?;
    let me2_dll = roots.file(f::ME2_DLL).map_err(fail)?;
    if !me2_launcher.is_file() || !me2_dll.is_file() {
        return Err(fail(format!("ModEngine2 was not found at {} (needs modengine2_launcher.exe and modengine2\\bin\\modengine2.dll).", roots.me2.as_ref().unwrap().display())));
    }
    let hook_dll = roots.file(f::HOOK_DLL_FILE).map_err(fail)?;
    if !hook_dll.is_file() {
        return Err(fail(format!("ashenmarine_hook.dll is missing next to the launcher ({}). Reinstall the mashup.", hook_dll.display())));
    }
    let real_save = roots.file(f::REAL_SAVE_DIR).map_err(|_| fail("Cannot work out where Dark Souls III keeps your save (%APPDATA%).".into()))?;
    let sandbox = roots.file(f::SANDBOX_SAVE_DIR).map_err(fail)?;
    let backups = roots.file(f::REAL_SAVE_BACKUP).map_err(fail)?;
    let mod_dir = roots.file(f::MOD_DIR).map_err(fail)?;
    let hook_cfg_path = roots.file(f::HOOK_CONFIG).map_err(fail)?;
    let me2_cfg_path = roots.file(f::ME2_CONFIG).map_err(fail)?;
    let hook_log = roots.file(f::HOOK_LOG).map_err(fail)?;
    let assets_dir = roots.file(f::CONVERTED_ASSETS).map_err(fail)?;
    ctx.say(&format!("Dark Souls III: {}", ds3_exe.display()));
    ctx.say(&format!("ModEngine2:      {}", me2_launcher.display()));
    ctx.say(&format!("real save:       {}", real_save.display()));
    ctx.say(&format!("private save:    {}", sandbox.display()));

    if platform::process_running(DS3_EXE_NAME) {
        return Err(fail("Dark Souls III is already running. Close it first, then press Play again.".into()));
    }

    // ---- 2. protect the real save
    let before = save::manifest(&real_save).map_err(|e| fail(format!("Cannot read your real save folder {}: {e}", real_save.display())))?;
    ctx.say(&format!("real save fingerprint: {} files", before.files.len()));
    let session_backup = if real_save.is_dir() {
        let tag = format!("session-{}", file_tag());
        if !backups.join("original").exists() {
            save::make_backup(&real_save, &backups, "original").map_err(|e| fail(format!("Cannot back up your save: {e}")))?;
            ctx.say("kept a permanent first-ever backup of your real save in backups/original");
        }
        let b = save::make_backup(&real_save, &backups, &tag).map_err(|e| fail(format!("Cannot back up your save: {e}")))?;
        let _ = save::prune_backups(&backups, "session-", 3);
        ctx.say(&format!("backup for this session: {}", b.as_ref().map(|p| p.display().to_string()).unwrap_or_default()));
        b
    } else {
        ctx.say("no real save folder yet (Dark Souls III never run on this account): nothing to back up");
        None
    };

    // ---- 3. private copy
    if sandbox.is_dir() {
        ctx.say("using the existing private save copy");
    } else if real_save.is_dir() {
        let n = save::copy_tree(&real_save, &sandbox).map_err(|e| fail(format!("Cannot create the private copy of your save: {e}")))?;
        ctx.say(&format!("created the private save copy from your real save ({n} files)"));
    } else {
        std::fs::create_dir_all(&sandbox).map_err(|e| fail(format!("Cannot create the private save folder: {e}")))?;
        ctx.say("created an empty private save folder (a fresh game)");
    }

    // ---- 4. configs and launch
    std::fs::create_dir_all(&mod_dir).map_err(|e| fail(format!("Cannot create {}: {e}", mod_dir.display())))?;
    let hook_cfg = HookConfig {
        version: VERSION.to_string(),
        real_save_dir: real_save.to_string_lossy().into_owned(),
        sandbox_save_dir: sandbox.to_string_lossy().into_owned(),
        log_file: hook_log.to_string_lossy().into_owned(),
        window_suffix: WINDOW_SUFFIX.to_string(),
        silent: o.silent,
        block_network: true,
        features: Features { probe: o.probe, sounds: o.sounds, experiments: o.experiments, rename: o.rename },
        assets_dir: assets_dir.to_string_lossy().into_owned(),
    };
    hook_cfg.save(&hook_cfg_path).map_err(|e| fail(format!("Cannot write {}: {e}", hook_cfg_path.display())))?;
    std::fs::write(&me2_cfg_path, me2_toml(&hook_dll, &mod_dir)).map_err(|e| fail(format!("Cannot write {}: {e}", me2_cfg_path.display())))?;
    // The hook explains why it closed the game in FATAL.txt (next to its log; next to the DLL if it had no config).
    let fatal_markers: Vec<PathBuf> = vec![hook_log.parent().map(|d| d.join("FATAL.txt")), hook_dll.parent().map(|d| d.join("FATAL.txt"))].into_iter().flatten().collect();
    for m in &fatal_markers {
        let _ = std::fs::remove_file(m);
    }
    let _ = std::fs::remove_file(&hook_log);

    ctx.say("starting Dark Souls III through ModEngine2");
    let mut child = Command::new(&me2_launcher)
        .arg("-t")
        .arg("ds3")
        .arg("-p")
        .arg(&ds3_exe)
        .arg("-c")
        .arg(&me2_cfg_path)
        .arg("--modengine-dll")
        .arg(&me2_dll)
        .current_dir(me2_launcher.parent().unwrap_or(Path::new(".")))
        .spawn()
        .map_err(|e| fail(format!("Cannot start ModEngine2: {e}")))?;
    // ---- 5. watch the game. ModEngine2's launcher may exit straight away or stay alive while the game runs;
    //         either is fine. The game should show up within 40 s, or the hook has already closed it (FATAL.txt),
    //         or ModEngine2 failed.
    let mut me2_status: Option<std::process::ExitStatus> = None;
    let fatal_exists = || fatal_markers.iter().any(|m| m.is_file());
    let appeared = wait_for(Duration::from_secs(40), || {
        if me2_status.is_none() {
            if let Ok(Some(s)) = child.try_wait() {
                me2_status = Some(s);
            }
        }
        platform::process_running(DS3_EXE_NAME) || fatal_exists() || matches!(me2_status, Some(s) if !s.success())
    });
    match me2_status {
        Some(s) if !s.success() => ctx.say(&format!("ModEngine2's launcher exited with {s}")),
        Some(_) => ctx.say("ModEngine2's launcher finished"),
        None => ctx.say("ModEngine2's launcher is running alongside the game"),
    }
    let running = platform::process_running(DS3_EXE_NAME);
    let mut lasted = Duration::ZERO;
    if running {
        ctx.say("Dark Souls III is running");
        let since = Instant::now();
        wait_for(Duration::from_secs(60 * 60 * 24), || !platform::process_running(DS3_EXE_NAME));
        lasted = since.elapsed();
        ctx.say(&format!("Dark Souls III closed after {:.0} s", lasted.as_secs_f32()));
        wait_for(Duration::from_secs(5), || matches!(child.try_wait(), Ok(Some(_))));
    } else if appeared && fatal_exists() {
        ctx.say("Dark Souls III was closed by the hook straight after starting");
    } else {
        ctx.say("Dark Souls III never appeared (ModEngine2 may have failed to start it)");
    }

    // ---- what did the hook say?
    let mut exit = ExitCode::SUCCESS;
    let hook_text = std::fs::read_to_string(&hook_log).unwrap_or_default();
    for line in hook_text.lines().filter(|l| l.contains("READY:") || l.contains("FATAL") || l.contains("STATS")).take(6) {
        ctx.say(&format!("hook: {line}"));
    }
    if running {
        // which files of the mod folder did the game really load? (the hook logs the first open of each one)
        let loaded = mod_files_opened(&hook_text);
        if loaded.is_empty() {
            ctx.say("the game did not open a single file from the mod folder (so none of the new names or weapon models was loaded)");
        } else {
            ctx.say(&format!("the game opened {} file(s) from the mod folder: {}", loaded.len(), loaded.join(", ")));
        }
    }
    if let Some(text) = fatal_markers.iter().find_map(|m| std::fs::read_to_string(m).ok()) {
        ctx.tell(&text, true);
        exit = ExitCode::from(3);
    }
    let hook_ready = hook_text.contains("READY:");
    if running && !hook_ready && exit == ExitCode::SUCCESS {
        // Typical cause: Steam was not running, so Dark Souls III restarted itself through Steam - and that copy has
        // none of Ashen Marine's protection (it is the plain game on the real save).
        ctx.say("WARNING: the hook never reported READY. The real-save check below is the safety net.");
        ctx.tell("Dark Souls III ran, but Ashen Marine's protection never started inside it, so that session was the NORMAL game on your real save (nothing of Ashen Marine was active). This usually happens when Steam is not running or not signed in: start Steam, wait until it is ready, then press Play again.", true);
        exit = ExitCode::from(5);
    } else if !running && !appeared && exit == ExitCode::SUCCESS {
        ctx.tell("Dark Souls III did not start. Make sure Steam is running and signed in, then press Play again. (Details are in logs\\launcher.log.)", true);
    } else if running && hook_ready && lasted < Duration::from_secs(20) && exit == ExitCode::SUCCESS {
        ctx.tell(&format!("Dark Souls III closed after only {} seconds. If you did not close it yourself, check that Steam is running and signed in, then press Play again.", lasted.as_secs()), false);
    }

    // ---- prove the real save is untouched
    let after = save::manifest(&real_save).map_err(|e| fail(format!("Cannot re-read your real save folder: {e}")))?;
    let d = save::diff(&before, &after);
    if d.is_empty() {
        ctx.say("VERIFIED: your real save is byte-identical to before this session");
    } else {
        ctx.say(&format!("PROBLEM: the real save changed during the session: modified={:?} removed={:?} added={:?}", d.modified, d.removed, d.added));
        match &session_backup {
            Some(b) => match save::restore_changed(b, &real_save, &d) {
                Ok(restored) => {
                    ctx.tell(&format!("Dark Souls III changed your real save folder during this session, which it should never do. Ashen Marine put {} file(s) back from the backup it took at launch. Details: {}", restored.len(), log_path.display()), true);
                }
                Err(e) => ctx.tell(&format!("Your real save changed and restoring it failed ({e}). The untouched backup is in {}", b.display()), true),
            },
            None => ctx.tell("Your real save folder changed during the session and there was no backup to restore from.", true),
        }
        exit = ExitCode::from(4);
    }
    Ok(exit)
}

/// The part of a path after the last folder called `mod` (either kind of separator, any case): the hook logs paths like
/// `C:/kit/ashenmarine/mod\parts\wp_a_0200.partsbnd.dcx`, with both kinds, because ModEngine2 joins them that way.
fn after_mod_folder(path: &str) -> &str {
    let lower = path.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let is_sep = |b: u8| b == b'\\' || b == b'/';
    (0..bytes.len().saturating_sub(4)).rev().find(|&i| is_sep(bytes[i]) && &bytes[i + 1..i + 4] == b"mod" && is_sep(bytes[i + 4])).map_or(path, |i| &path[i + 5..])
}

/// The files below the mod folder that the game opened, as the hook logged them ("MOD FILE OPENED by the game [id]: <path> (+N
/// ms ...)"), shortened to the part after the mod folder (`msg\engus\item_dlc2.msgbnd.dcx`).
fn mod_files_opened(hook_log: &str) -> Vec<String> {
    hook_log
        .lines()
        .filter_map(|l| l.split_once("MOD FILE OPENED by the game [")?.1.split_once("]: ").map(|(_, rest)| rest))
        .map(|rest| after_mod_folder(rest.split(" (+").next().unwrap_or(rest)).to_string())
        .collect()
}

fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(msg) => {
            platform::message_box("Ashen Marine", &msg, false);
            return ExitCode::from(64);
        }
    };
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."));
    match run(&opts, &exe_dir) {
        Ok(code) => code,
        Err((msg, _)) => {
            // Best effort: the logger may not exist yet.
            let data = opts.data.clone().unwrap_or(exe_dir);
            let log = Logger::open(&data.join("logs").join("launcher.log"), "launcher");
            log.log(&format!("ERROR: {msg}"));
            if !opts.silent {
                platform::message_box("Ashen Marine", &msg, true);
            } else {
                eprintln!("{msg}");
            }
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_the_game_opened_are_read_from_the_hook_log() {
        let log = "[2026-10-10 00:28:55.291] [hook] MOD FILE asked for by the game [fs_getfileattributesexw]: C:/kit/ashenmarine/mod\\parts\\wp_a_0200.partsbnd.dcx (+11488 ms after the hook loaded; 287 different such files so far)\n\
[2026-10-10 00:29:01.240] [hook] MOD FILE OPENED by the game [fs_createfilew]: C:/kit/ashenmarine/mod\\msg\\engus\\item_dlc2.msgbnd.dcx (+10518 ms after the hook loaded; 1 such files so far)\n\
[2026-10-10 00:29:02.211] [hook] MOD FILE OPENED by the game [fs_createfilew]: C:/kit/ashenmarine/mod\\parts\\wp_a_0200.partsbnd.dcx (+11489 ms after the hook loaded; 2 such files so far)\n\
[2026-10-10 00:29:02.300] [hook] STATS redirected=1 denied_addr=1 denied_name=0\n";
        assert_eq!(mod_files_opened(log), vec!["msg\\engus\\item_dlc2.msgbnd.dcx".to_string(), "parts\\wp_a_0200.partsbnd.dcx".to_string()]);
        assert!(mod_files_opened("").is_empty());
        assert!(mod_files_opened("nothing about files here\nMOD FILE OPENED by the game [broken").is_empty());
    }
}
