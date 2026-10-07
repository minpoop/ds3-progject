//! Space Marine 2 sounds for swings and shots, and (private test kits only) the F8 experiment that puts the test weapons
//! in the inventory.
//!
//! Dark Souls III has no "a swing started" or "a shot was fired" event to listen to, so each frame (on the game's own
//! thread) this reads numbers it can see - stamina, ammunition counts in the inventory, whether an attack button is
//! down - and feeds the small state machines of `ashen_common::triggers`. When one fires, a prepared WAV clip is handed
//! to the private mixer (`audio`), which plays it next to the game's own sound. Nothing here sends input, hooks game
//! code or touches the game's audio engine. A trace of what it saw goes to `sfx-trace.csv` so the rules can be tuned
//! from a real play session.
use super::{audio, gametask, input, memscan, version};
use ashen_common::{
    config::HookConfig,
    logging::Logger,
    soundset::{self, Rng, SoundSet},
    triggers::{AmmoTracker, Edge, Swing, SwingDetector, SwingInput},
};
use darksouls3::sprj::{CSRegulationManager, GameDataMan, ItemCategory, ItemId, PlayerGameData, PlayerIns};
use fromsoftware_shared::FromStatic;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Mixer key of the chainsword idle loop.
const KEY_IDLE: u32 = 1;
/// Weapon-table rows 400000..=420000 are arrows and bolts (checked against the game's own table by the probe).
const AMMO_ROWS: std::ops::RangeInclusive<u32> = 400_000..=420_000;
/// How often the inventory is looked at, in frames (20 times a second at 60 fps).
const AMMO_EVERY_FRAMES: u64 = 3;
/// After this many crashes inside a frame the feature switches itself off.
const MAX_FRAME_PANICS: u32 = 5;
const TRACE_MAX_ROWS: usize = 400_000;

/// The Dark Souls III rows the test weapons are made from, and what they are called afterwards. The names are the
/// game's own English names (Paramdex / checked by the probe's weapon table), written over the item-name text in memory.
const EXPERIMENT_ITEMS: [(u32, u32, &str, &str); 3] = [
    // (weapon table row, quantity, old English name, new name)
    (2_000_000, 1, "Shortsword", "Chainsword"),
    (14_040_000, 1, "Light Crossbow", "Bolt Pistol"),
    (404_000, 60, "Standard Bolt", "Bolt Rounds"),
];

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into())
}

fn dir_of(cfg: &HookConfig) -> PathBuf {
    Path::new(&cfg.log_file).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

/// Runs on its own thread for the life of the process.
pub fn thread_body(cfg: HookConfig) {
    let dir = dir_of(&cfg);
    let log = Arc::new(Logger::open(&dir.join("sfx.txt"), "sfx"));
    log.log(&format!("sound feature v{} starting", ashen_common::VERSION));

    // --- the prepared sounds
    if cfg.assets_dir.is_empty() {
        log.log("no assets folder was configured: sounds are off");
        return;
    }
    let sounds_dir = Path::new(&cfg.assets_dir).join("sounds");
    let loaded = match SoundSet::load(&sounds_dir) {
        Ok(l) => l,
        Err(e) => {
            log.log(&format!("no sounds to play: {e}. Run Prepare-AshenMarine.bat (it reads your Space Marine 2 install and makes them), then start the game again."));
            return;
        }
    };
    for w in loaded.warnings.iter().take(40) {
        log.log(&format!("sound file problem: {w}"));
    }
    let sounds = loaded.set;
    log.log(&format!("loaded {} sound slots ({:.1} s of audio): {}", sounds.len(), sounds.total_seconds(), sounds.names().join(", ")));
    if sounds.is_empty() {
        log.log("nothing playable: sounds are off");
        return;
    }

    match version::detect() {
        Ok(v) if version::supported(&v) => log.log(&format!("game build {} is supported", v.version)),
        Ok(v) => {
            log.log(&format!("game build {} (language 0x{:02X}) is not one the sound feature can read stamina from: sounds are off", v.version, v.lang));
            return;
        }
        Err(e) => {
            log.log(&format!("cannot tell which game build this is ({e}): sounds are off"));
            return;
        }
    }

    // --- the sound device
    let Some(audio) = catch_unwind(AssertUnwindSafe(|| audio::start(log.clone()))).ok().flatten() else {
        log.log("no sound device: sounds are off");
        return;
    };

    // --- the per-frame task
    let volumes: HashMap<String, f32> = soundset::sheet_slots().into_iter().map(|s| (s.id, s.volume)).collect();
    let input = input::Input::new();
    log.log(&format!(
        "input: gamepad support {}, mouse {}; trace goes to sfx-trace.csv; hotkeys: F5/F6 quieter/louder, F7 chainsword idle loop{}",
        if input.has_gamepad_support() { "available" } else { "missing" },
        "read while the game window is in front",
        if cfg.features.experiments { ", F8 test weapons + names" } else { "" }
    ));
    let mut state = Sfx::new(log.clone(), audio, sounds, volumes, input, dir.join("sfx-trace.csv"), cfg.features.experiments);
    let task = gametask::register(&log, "the sound feature", move || state.frame());
    let _handle = match task {
        Ok(h) => h,
        Err(e) => {
            log.log(&format!("could not start the sound feature: {e}"));
            return;
        }
    };
    // The handle has to stay alive for the whole process; this thread has nothing else to do.
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

/// What one frame of the game looked like.
struct FrameView {
    stamina: i32,
    hp: i32,
    buttons: input::Buttons,
    /// `Some` on the frames when the inventory was looked at (`Some(None)` = it could not be read)
    ammo: Option<Option<Vec<(u32, u32)>>>,
}

struct Sfx {
    log: Arc<Logger>,
    audio: audio::Audio,
    sounds: SoundSet,
    volumes: HashMap<String, f32>,
    rng: Rng,
    input: input::Input,
    t0: Instant,
    frame_no: u64,
    panics: u32,
    off: bool,
    swing: SwingDetector,
    ammo: AmmoTracker,
    f5: Edge,
    f6: Edge,
    f7: Edge,
    f8: Edge,
    /// master volume, changed with F5 (quieter) and F6 (louder) in steps of 3 dB
    master_db: f32,
    idle_on: bool,
    experiments: bool,
    experiment_done: bool,
    in_world_logged: bool,
    plays: u32,
    trace: Trace,
}

impl Sfx {
    fn new(log: Arc<Logger>, audio: audio::Audio, sounds: SoundSet, volumes: HashMap<String, f32>, input: input::Input, trace_path: PathBuf, experiments: bool) -> Sfx {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        Sfx {
            log,
            audio,
            sounds,
            volumes,
            rng: Rng::new(seed),
            input,
            t0: Instant::now(),
            frame_no: 0,
            panics: 0,
            off: false,
            swing: SwingDetector::new(),
            ammo: AmmoTracker::new(),
            f5: Edge::default(),
            f6: Edge::default(),
            f7: Edge::default(),
            f8: Edge::default(),
            master_db: 0.0,
            idle_on: false,
            experiments,
            experiment_done: false,
            in_world_logged: false,
            plays: 0,
            trace: Trace::new(trace_path),
        }
    }

    /// Called by the game once per frame, on the game's own thread. A crash in here is caught and counted; it never
    /// reaches the game.
    fn frame(&mut self) {
        if self.off {
            return;
        }
        self.frame_no += 1;
        let r = catch_unwind(AssertUnwindSafe(|| unsafe { self.tick() }));
        if let Err(p) = r {
            self.panics += 1;
            self.log.log(&format!("a frame crashed ({}), {} of {}", panic_text(&*p), self.panics, MAX_FRAME_PANICS));
            if self.panics >= MAX_FRAME_PANICS {
                self.off = true;
                self.audio.stop(KEY_IDLE);
                self.log.log("too many crashes: the sound feature is switched off for this session");
            }
        }
        self.trace.flush_if_due();
    }

    unsafe fn tick(&mut self) {
        let now_ms = self.t0.elapsed().as_millis() as u64;

        // hotkeys count only while the game window is in front
        let focus = input::game_has_focus();
        let f5 = self.f5.rising(focus && input::key_down(input::VK_F5));
        let f6 = self.f6.rising(focus && input::key_down(input::VK_F6));
        let f7 = self.f7.rising(focus && input::key_down(input::VK_F7));
        if f5 || f6 {
            self.change_volume(if f6 { 3.0 } else { -3.0 });
        }
        let f8 = self.f8.rising(focus && self.experiments && input::key_down(input::VK_F8));
        if f7 {
            self.toggle_idle();
        }

        let Ok(player) = PlayerIns::local_player() else { return };
        // This runs 60 times a second, also across loading screens: look before every hop so a stale pointer is a skipped
        // frame, not a crash.
        let pgd_addr = player.player_game_data.as_ptr() as usize;
        let modules_addr = player.super_chr_ins.modules.as_ptr() as usize;
        if !memscan::readable(player as *const PlayerIns as usize, core::mem::size_of::<PlayerIns>())
            || !memscan::readable(pgd_addr, core::mem::size_of::<PlayerGameData>())
            || !memscan::readable(modules_addr, 0x40)
            || !memscan::readable(*(modules_addr as *const usize).add(3), 0x1c8)
        {
            return;
        }
        let pgd: &PlayerGameData = player.player_game_data.as_ref();
        if pgd.equipment.is_main_menu() {
            return; // the placeholder character of the title screen
        }
        if !self.in_world_logged {
            self.in_world_logged = true;
            self.log.log(&format!("a character is loaded; watching stamina, ammunition and attack buttons ({} s after the feature started)", now_ms / 1000));
        }
        if f8 && !self.experiment_done {
            self.experiment_done = true;
            self.run_experiment();
        }

        let view = FrameView {
            stamina: player.super_chr_ins.modules.data.stamina,
            hp: player.super_chr_ins.modules.data.hp,
            buttons: self.input.buttons(),
            // the inventory is only looked at every few frames
            ammo: if self.frame_no % AMMO_EVERY_FRAMES == 0 { Some(read_ammo(pgd)) } else { None },
        };
        self.step(now_ms, view);
    }

    /// Everything that decides and plays, separated from reading the game so it can be tested with made-up frames.
    fn step(&mut self, now_ms: u64, v: FrameView) {
        let swing = self.swing.update(SwingInput { now_ms, stamina: v.stamina, light_down: v.buttons.light, strong_down: v.buttons.strong });
        let mut event = "";
        if let Some(sw) = swing {
            let slot = match sw {
                Swing::Light { step } => format!("chainsword_swing_{step}"),
                Swing::Strong => "chainsword_strong".to_string(),
            };
            event = "swing";
            self.play(&slot);
        }
        if let Some(counts) = v.ammo {
            let shots = self.ammo.update(counts.as_deref());
            if shots > 0 {
                event = "shot";
                for _ in 0..shots.min(AmmoTracker::MAX_SHOTS_AT_ONCE) {
                    self.play("boltpistol_fire");
                }
            }
        }
        self.trace.row(now_ms, v.stamina, v.hp, &v.buttons, event);
    }

    /// F5 / F6: the mashup's sounds quieter / louder (3 dB per press, between -30 dB and +9 dB).
    fn change_volume(&mut self, step_db: f32) {
        self.master_db = (self.master_db + step_db).clamp(-30.0, 9.0);
        self.log.log(&format!("master volume {:+.0} dB", self.master_db));
        if self.idle_on {
            // the loop is already playing at the old level: restart it at the new one
            self.audio.stop(KEY_IDLE);
            let random = self.rng.next();
            if let Some(clip) = self.sounds.pick("chainsword_idle", random) {
                let gain = self.gain_of("chainsword_idle", 0.5);
                self.audio.play_loop(KEY_IDLE, clip, gain);
            }
        }
    }

    fn gain_of(&self, slot: &str, default: f32) -> f32 {
        self.volumes.get(slot).copied().unwrap_or(default) * 10f32.powf(self.master_db / 20.0)
    }

    fn play(&mut self, slot: &str) {
        let random = self.rng.next();
        match self.sounds.pick(slot, random) {
            Some(clip) => {
                let gain = self.gain_of(slot, 0.8);
                self.audio.play(clip, gain);
                self.plays += 1;
                if self.plays <= 25 {
                    self.log.log(&format!("playing {slot} (gain {gain:.2})"));
                }
            }
            None => {
                if self.plays <= 25 {
                    self.log.log(&format!("no sound for {slot} (not prepared)"));
                }
            }
        }
    }

    fn toggle_idle(&mut self) {
        self.idle_on = !self.idle_on;
        if self.idle_on {
            self.play("chainsword_idle_start");
            let random = self.rng.next();
            match self.sounds.pick("chainsword_idle", random) {
                Some(clip) => {
                    let gain = self.gain_of("chainsword_idle", 0.5);
                    self.audio.play_loop(KEY_IDLE, clip, gain);
                    self.log.log("F7: chainsword idle loop ON");
                }
                None => self.log.log("F7: there is no chainsword_idle sound"),
            }
        } else {
            self.audio.stop(KEY_IDLE);
            self.log.log("F7: chainsword idle loop OFF");
        }
    }

    /// F8 (private test kits): put the test weapons in the inventory and rename them in memory. Everything is logged.
    unsafe fn run_experiment(&mut self) {
        self.log.log("F8 pressed: giving the test weapons and renaming them (experiment)");
        let reg = match CSRegulationManager::instance() {
            Ok(r) => r,
            Err(e) => {
                self.log.log(&format!("F8: no parameter manager yet ({e}); nothing done"));
                self.experiment_done = false;
                return;
            }
        };
        let gdm = match GameDataMan::instance_mut() {
            Ok(g) => g,
            Err(e) => {
                self.log.log(&format!("F8: no game data manager ({e}); nothing done"));
                self.experiment_done = false;
                return;
            }
        };
        for (row, qty, old, new) in EXPERIMENT_ITEMS {
            let Ok(item) = ItemId::new(ItemCategory::Weapon, row) else { continue };
            match reg.get_equip_param(item) {
                Some(_) => {
                    gdm.give_item_directly(item, qty);
                    self.log.log(&format!("F8: gave {qty} x weapon row {row} ({old}, to be called {new})"));
                }
                None => self.log.log(&format!("F8: the game has no weapon row {row} ({old}); skipped")),
            }
        }
        // the renaming scans the whole address space: not on the game's thread
        let log = self.log.clone();
        let pairs: Vec<(&'static str, &'static str)> = EXPERIMENT_ITEMS.iter().map(|&(_, _, old, new)| (old, new)).collect();
        let spawned = std::thread::Builder::new().name("ashen-rename".into()).spawn(move || {
            if let Err(p) = catch_unwind(AssertUnwindSafe(|| rename_items(&log, &pairs))) {
                log.log(&format!("renaming crashed: {}", panic_text(&*p)));
            }
        });
        if spawned.is_err() {
            self.log.log("F8: could not start the renaming thread");
        }
    }
}

/// (item id, quantity) of every arrow and bolt stack. `None` when the inventory does not look sane this frame.
unsafe fn read_ammo(pgd: &PlayerGameData) -> Option<Vec<(u32, u32)>> {
    let items = &pgd.equipment.equip_inventory_data.items_data;
    if items.normal_items_capacity > 8192 || items.key_items_capacity > 1024 || items.normal_items_count > items.normal_items_capacity {
        return None;
    }
    let head = items.normal_items_head.as_ptr() as usize;
    if head < 0x10000 {
        return None;
    }
    let mut v = Vec::new();
    for e in items.items() {
        if e.item_id.category() == ItemCategory::Weapon && AMMO_ROWS.contains(&e.item_id.param_id()) {
            v.push((e.item_id.into_inner(), e.quantity));
        }
    }
    Some(v)
}

// ------------------------------------------------------------------------------------------------ renaming

/// Overwrite every whole-string occurrence of each old name with the new one, padded with spaces to the old length so a
/// string that carries its length somewhere stays consistent. Names that are longer than the old one are refused.
fn rename_items(log: &Logger, pairs: &[(&str, &str)]) {
    for (old, new) in pairs {
        if new.chars().count() > old.chars().count() {
            log.log(&format!("rename {old:?} -> {new:?}: the new name is longer than the old one; skipped"));
            continue;
        }
        let hits = memscan::find_exact_utf16(old, Duration::from_secs(40));
        log.log(&format!("rename {old:?} -> {new:?}: {} whole-string occurrence(s) in memory", hits.len()));
        let mut padded: Vec<u16> = new.encode_utf16().collect();
        padded.resize(old.encode_utf16().count(), 0x20);
        let bytes: Vec<u8> = padded.iter().flat_map(|u| u.to_le_bytes()).collect();
        let mut done = 0;
        for addr in &hits {
            if memscan::write_into(*addr, &bytes) {
                done += 1;
            } else {
                log.log(&format!("  could not write at 0x{addr:X} (page not writable)"));
            }
        }
        let after = memscan::find_exact_utf16(new, Duration::from_secs(40));
        log.log(&format!("  rewrote {done} of {}; the new name is now found {} time(s) (open the inventory to see whether the game shows it)", hits.len(), after.len()));
    }
}

// ------------------------------------------------------------------------------------------------ trace

/// What the feature saw, one line whenever something changed, for tuning the trigger rules from a real session.
struct Trace {
    path: PathBuf,
    text: String,
    rows: usize,
    last: Option<(i32, u16, (u8, u8), (bool, bool))>,
    last_flush: Instant,
}

impl Trace {
    fn new(path: PathBuf) -> Trace {
        Trace { path, text: String::from("t_ms,stamina,hp,pad_buttons,pad_lt,pad_rt,lmb,rmb,event\n"), rows: 0, last: None, last_flush: Instant::now() }
    }

    fn row(&mut self, t_ms: u64, stamina: i32, hp: i32, b: &input::Buttons, event: &str) {
        if self.rows >= TRACE_MAX_ROWS {
            return;
        }
        let key = (stamina, b.pad_buttons, b.pad_triggers, b.mouse);
        // stamina creeps up one point at a time while resting: only a change of 2 or more, a button change or an event is news
        let news = match self.last {
            None => true,
            Some((s, pb, pt, m)) => (stamina - s).abs() >= 2 || pb != b.pad_buttons || pt != b.pad_triggers || m != b.mouse,
        } || !event.is_empty();
        if !news {
            return;
        }
        self.last = Some(key);
        self.rows += 1;
        let _ = writeln!(self.text, "{t_ms},{stamina},{hp},0x{:04X},{},{},{},{},{event}", b.pad_buttons, b.pad_triggers.0, b.pad_triggers.1, b.mouse.0 as u8, b.mouse.1 as u8);
    }

    fn flush_if_due(&mut self) {
        if self.last_flush.elapsed() >= Duration::from_secs(3) {
            self.last_flush = Instant::now();
            let _ = std::fs::write(&self.path, &self.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_whole_strings_only_and_renames_them_in_place() {
        // a heap string with a header before it, like the game's, plus a longer word that merely ends the same way
        let mut block: Vec<u8> = vec![0x3e, 0x4a, 0x7e, 0x6b, 0x00, 0x4c, 0x00, 0x90];
        let name: Vec<u8> = "Zzqxy Test Sword".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let at = block.len();
        block.extend(&name);
        block.extend([0u8, 0]);
        block.extend([0u8; 6]);
        let longer: Vec<u8> = "Big Zzqxy Test Sword".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let at2 = block.len();
        block.extend(&longer);
        block.extend([0u8, 0, 0, 0]);
        let base = block.as_ptr() as usize;
        let hits = memscan::find_exact_utf16("Zzqxy Test Sword", Duration::from_secs(60));
        assert!(hits.contains(&(base + at)), "the whole string is found");
        assert!(!hits.contains(&(base + at2 + 8 * 2)), "the tail of a longer string is not");

        let dir = std::env::temp_dir().join(format!("ashen-sfx-test-{}", std::process::id()));
        let log = Logger::open(&dir.join("log.txt"), "t");
        rename_items(&log, &[("Zzqxy Test Sword", "Zzqxy Axe")]);
        let after: Vec<u16> = block[at..at + name.len()].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16_lossy(&after), "Zzqxy Axe       ", "padded with spaces to the old length");
        assert_eq!(&block[at + name.len()..at + name.len() + 2], &[0, 0], "the terminator is untouched");
        let text = std::fs::read_to_string(dir.join("log.txt")).unwrap();
        assert!(text.contains("rewrote"), "{text}");
        // a longer replacement is refused
        rename_items(&log, &[("Zzqxy Axe       ", "A much much longer replacement name")]);
        assert!(std::fs::read_to_string(dir.join("log.txt")).unwrap().contains("longer than the old one"));
        drop(block);
    }

    #[test]
    fn the_trace_only_records_news() {
        let mut t = Trace::new(std::env::temp_dir().join("ashen-trace-test.csv"));
        let b = input::Buttons::default();
        t.row(0, 100, 500, &b, "");
        t.row(16, 101, 500, &b, ""); // regeneration by one point: not news
        t.row(32, 99, 500, &b, ""); // within 2 of the last recorded value: not news
        t.row(48, 70, 500, &b, ""); // a real drop
        let pressed = input::Buttons { pad_buttons: 0x0200, ..b };
        t.row(64, 70, 500, &pressed, ""); // a button
        t.row(80, 70, 500, &pressed, "swing"); // an event
        assert_eq!(t.rows, 4, "{}", t.text);
        assert!(t.text.contains("0x0200") && t.text.contains(",swing"));
    }

    fn test_sfx(tag: &str) -> (Sfx, std::sync::mpsc::Receiver<audio::Msg>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ashen-sfx-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // one WAV per slot of the design sheet, each a different length so the test can tell which slot played
        let sheet = soundset::sheet_slots();
        let mut slots = Vec::new();
        for (i, s) in sheet.iter().enumerate() {
            let frames = 441 * (i + 1); // 10 ms, 20 ms, ...
            let name = format!("{}_1.wav", s.id);
            std::fs::write(dir.join(&name), ashen_common::wav::build(1, 44_100, &vec![1000i16; frames])).unwrap();
            slots.push(format!(r#"{{"slot":"{}","event":"{}","loop":{},"files":["{name}"]}}"#, s.id, s.sm2_event, s.looped));
        }
        std::fs::write(dir.join("index.json"), format!(r#"{{"format":1,"sounds":[{}]}}"#, slots.join(","))).unwrap();
        let sounds = SoundSet::load(&dir).unwrap().set;
        let volumes: HashMap<String, f32> = sheet.iter().map(|s| (s.id.clone(), s.volume)).collect();
        let (audio, rx) = audio::Audio::test_pair();
        let log = Arc::new(Logger::open(&dir.join("sfx.txt"), "t"));
        (Sfx::new(log, audio, sounds, volumes, input::Input::new(), dir.join("trace.csv"), false), rx, dir)
    }

    fn played(rx: &std::sync::mpsc::Receiver<audio::Msg>) -> Vec<String> {
        let sheet = soundset::sheet_slots();
        let mut out = Vec::new();
        while let Ok(m) = rx.try_recv() {
            let (kind, frames) = match &m {
                audio::Msg::Play(c, _) => ("play", c.len_frames()),
                audio::Msg::Loop(_, c, _) => ("loop", c.len_frames()),
                audio::Msg::Stop(_) => ("stop", 0),
            };
            let slot = if frames == 0 { String::new() } else { sheet[frames / 441 - 1].id.clone() };
            out.push(format!("{kind}:{slot}"));
        }
        out
    }

    #[test]
    fn swings_shots_and_the_idle_loop_reach_the_mixer_as_the_sheet_says() {
        let (mut s, rx, _dir) = test_sfx("step");
        let none = input::Buttons::default();
        let light = input::Buttons { light: true, ..none };
        let strong = input::Buttons { strong: true, ..none };
        let frame = |stamina: i32, buttons: input::Buttons, ammo: Option<Option<Vec<(u32, u32)>>>| FrameView { stamina, hp: 500, buttons, ammo };

        // first attack, a second one 0.6 s later (combo step 2), a roll in between (no button) that must stay silent
        s.step(0, frame(100, none, None));
        s.step(16, frame(100, light, None));
        s.step(32, frame(85, light, None));
        s.step(300, frame(85, none, None));
        s.step(310, frame(60, none, None)); // a roll
        s.step(620, frame(70, light, None));
        s.step(640, frame(55, light, None));
        assert_eq!(played(&rx), vec!["play:chainsword_swing_1", "play:chainsword_swing_2"]);
        // a strong attack
        s.step(2500, frame(100, none, None));
        s.step(2516, frame(100, strong, None));
        s.step(2532, frame(70, strong, None));
        assert_eq!(played(&rx), vec!["play:chainsword_strong"]);
        // shots: bolts 60 -> 59 -> 57
        s.step(3000, frame(100, none, Some(Some(vec![(404_000, 60)]))));
        s.step(3100, frame(100, none, Some(Some(vec![(404_000, 59)]))));
        s.step(3200, frame(100, none, Some(Some(vec![(404_000, 57)]))));
        s.step(3300, frame(100, none, Some(None))); // unreadable inventory: nothing
        assert_eq!(played(&rx), vec!["play:boltpistol_fire", "play:boltpistol_fire", "play:boltpistol_fire"]);
        // F7 twice: start sound + loop, then stop
        s.toggle_idle();
        s.toggle_idle();
        assert_eq!(played(&rx), vec!["play:chainsword_idle_start", "loop:chainsword_idle", "stop:"]);
        assert!(s.trace.text.contains(",swing") && s.trace.text.contains(",shot"));
    }

    #[test]
    fn the_master_volume_changes_in_steps_and_is_limited() {
        let (mut s, rx, _dir) = test_sfx("volume");
        let base = s.gain_of("chainsword_swing_1", 0.8);
        s.change_volume(-3.0);
        let quieter = s.gain_of("chainsword_swing_1", 0.8);
        assert!((quieter / base - 10f32.powf(-3.0 / 20.0)).abs() < 1e-4, "{base} {quieter}");
        for _ in 0..30 {
            s.change_volume(3.0);
        }
        assert_eq!(s.master_db, 9.0);
        for _ in 0..30 {
            s.change_volume(-3.0);
        }
        assert_eq!(s.master_db, -30.0);
        // a running idle loop is restarted at the new level
        s.master_db = 0.0;
        s.idle_on = true;
        s.change_volume(-3.0);
        assert_eq!(played(&rx), vec!["stop:", "loop:chainsword_idle"]);
    }

    #[test]
    fn a_missing_slot_is_skipped_quietly() {
        let (mut s, rx, dir) = test_sfx("missing");
        std::fs::write(dir.join("index.json"), r#"{"format":1,"sounds":[]}"#).unwrap();
        s.sounds = SoundSet::load(&dir).unwrap().set;
        s.play("chainsword_swing_1");
        assert!(played(&rx).is_empty());
        assert!(std::fs::read_to_string(dir.join("sfx.txt")).unwrap().contains("no sound for chainsword_swing_1"));
    }

    #[test]
    fn the_experiment_table_only_renames_to_names_that_fit() {
        for (_, _, old, new) in EXPERIMENT_ITEMS {
            assert!(new.chars().count() <= old.chars().count(), "{new:?} must fit where {old:?} is");
        }
    }
}
