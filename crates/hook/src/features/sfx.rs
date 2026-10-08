//! Space Marine 2 sounds for swings and shots, and (private test kits only) the F8 experiment that puts the test weapons
//! in the inventory.
//!
//! Dark Souls III has no "a swing started" or "a shot was fired" event to listen to, so each frame (on the game's own
//! thread) this reads numbers it can see - stamina, ammunition counts in the inventory, whether an attack button is
//! down - and feeds the small state machines of `ashen_common::triggers`. When one fires, a prepared WAV clip is handed
//! to the private mixer (`audio`), which plays it next to the game's own sound. Nothing here sends input, hooks game
//! code or touches the game's audio engine. A trace of what it saw goes to `sfx-trace.csv` so the rules can be tuned
//! from a real play session.
use super::{audio, equip, gametask, input, memscan, version};
use ashen_common::{
    config::HookConfig,
    logging::Logger,
    soundset::{self, Rng, SoundSet, ALT_SET},
    triggers::{AmmoTracker, Combo, Edge, Hand, ShotQueue, Swing, SwingDetector, SwingInput},
    weapons::{self, WeaponClass},
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
/// Weapon-table rows 400000..=420000 are arrows and bolts (checked against the game's own table by the probe); the bolts
/// among them are picked out by their category.
const AMMO_ROWS: std::ops::RangeInclusive<u32> = 400_000..=420_000;
/// After this many crashes inside a frame the feature switches itself off.
const MAX_FRAME_PANICS: u32 = 5;
const TRACE_MAX_ROWS: usize = 400_000;
/// At most this many equipment changes are written to the log (a session of weapon switching stays readable).
const MAX_EQUIPMENT_LOG_LINES: u32 = 200;
/// How many of the ammunition the F8 experiment gives.
const EXPERIMENT_AMMO: u32 = 60;

/// What F8 puts in the inventory, from the weapons sheet: every weapon once, and the ammunition of the ranged ones.
/// (weapon table row, quantity, what to call it in the log)
fn experiment_items() -> Vec<(u32, u32, String)> {
    let mut v = Vec::new();
    for w in weapons::sheet_weapons() {
        v.push((w.base_row, 1, format!("{} (a {})", w.display_name, w.base_name)));
        if let Some(ammo) = w.ammo_row {
            v.push((ammo, EXPERIMENT_AMMO, format!("bolts for the {}", w.display_name)));
        }
    }
    v
}

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
    // a second set made from the same events with the volumes and delays of the game's own sound bank (key F9 switches)
    let alt_dir = Path::new(&cfg.assets_dir).join(ALT_SET);
    let alt = match SoundSet::load(&alt_dir) {
        Ok(l) if !l.set.is_empty() => {
            for w in l.warnings.iter().take(20) {
                log.log(&format!("second sound set, file problem: {w}"));
            }
            log.log(&format!("second sound set (the bank read exactly): {} slots, {:.1} s of audio; F9 switches between the two sets", l.set.len(), l.set.total_seconds()));
            Some(l.set)
        }
        _ => {
            log.log("no second sound set (the bank could not be read exactly, or it was not prepared): F9 does nothing");
            None
        }
    };

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
        "input: gamepad support {}, mouse {}; trace goes to sfx-trace.csv; hotkeys: F5/F6 quieter/louder, F7 chainsword idle loop, F9 other sound set{}",
        if input.has_gamepad_support() { "available" } else { "missing" },
        "read while the game window is in front",
        if cfg.features.experiments { ", F8 test weapons + names" } else { "" }
    ));
    let mut state = Sfx::new(log.clone(), audio, sounds, alt, volumes, input, dir.join("sfx-trace.csv"), cfg.features.experiments);
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
    /// (item id, quantity) of every bolt stack. `Some(None)` = the inventory could not be read; `None` = not looked at this frame
    ammo: Option<Option<Vec<(u32, u32)>>>,
    /// what is in the hands; `None` = could not be read at all (then every attack and every shot counts, as in kit 4)
    hands: Option<equip::Hands>,
}

struct Sfx {
    log: Arc<Logger>,
    audio: audio::Audio,
    sounds: SoundSet,
    /// the second set (F9), when the setup tool made one
    alt: Option<SoundSet>,
    use_alt: bool,
    volumes: HashMap<String, f32>,
    rng: Rng,
    input: input::Input,
    t0: Instant,
    frame_no: u64,
    panics: u32,
    off: bool,
    swing: SwingDetector,
    combo: Combo,
    ammo: AmmoTracker,
    shots: ShotQueue,
    f5: Edge,
    f6: Edge,
    f7: Edge,
    f8: Edge,
    f9: Edge,
    /// master volume, changed with F5 (quieter) and F6 (louder) in steps of 3 dB
    master_db: f32,
    idle_on: bool,
    experiments: bool,
    experiment_done: bool,
    in_world_logged: bool,
    plays: u32,
    trace: Trace,
    /// weapon-table row -> is it a bolt (looked up once per row)
    bolt_rows: HashMap<u32, bool>,
    last_hands: Option<equip::Hands>,
    equipment_log_lines: u32,
}

impl Sfx {
    fn new(log: Arc<Logger>, audio: audio::Audio, sounds: SoundSet, alt: Option<SoundSet>, volumes: HashMap<String, f32>, input: input::Input, trace_path: PathBuf, experiments: bool) -> Sfx {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        Sfx {
            log,
            audio,
            sounds,
            alt,
            use_alt: false,
            volumes,
            rng: Rng::new(seed),
            input,
            t0: Instant::now(),
            frame_no: 0,
            panics: 0,
            off: false,
            swing: SwingDetector::new(),
            combo: Combo::new(),
            ammo: AmmoTracker::new(),
            shots: ShotQueue::new(),
            f5: Edge::default(),
            f6: Edge::default(),
            f7: Edge::default(),
            f8: Edge::default(),
            f9: Edge::default(),
            master_db: 0.0,
            idle_on: false,
            experiments,
            experiment_done: false,
            in_world_logged: false,
            plays: 0,
            trace: Trace::new(trace_path),
            bolt_rows: HashMap::new(),
            last_hands: None,
            equipment_log_lines: 0,
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
        if self.f9.rising(focus && input::key_down(input::VK_F9)) {
            self.toggle_set();
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
            self.log.log(&format!("a character is loaded; watching stamina, ammunition, attack buttons and the hands ({} s after the feature started)", now_ms / 1000));
        }
        if f8 && !self.experiment_done {
            self.experiment_done = true;
            self.run_experiment(pgd);
        }

        let reg = CSRegulationManager::instance().ok();
        let hands = equip::read(pgd, reg);
        self.note_hands(&hands);
        let ammo = Some(self.read_bolts(pgd, reg));
        let view = FrameView {
            stamina: player.super_chr_ins.modules.data.stamina,
            hp: player.super_chr_ins.modules.data.hp,
            buttons: self.input.buttons(),
            ammo,
            hands: hands.asm.is_some().then_some(hands),
        };
        self.step(now_ms, view);
    }

    /// Write a line to the log whenever what is in the hands changes (the first thing to check when a sound is missing or wrong).
    fn note_hands(&mut self, hands: &equip::Hands) {
        if self.last_hands.as_ref() == Some(hands) {
            return;
        }
        if self.equipment_log_lines < MAX_EQUIPMENT_LOG_LINES {
            self.equipment_log_lines += 1;
            let first = self.last_hands.is_none();
            self.log.log(&format!("{}: {}", if first { "equipment" } else { "equipment changed" }, hands.describe()));
        }
        self.last_hands = Some(hands.clone());
    }

    /// Everything that decides and plays, separated from reading the game so it can be tested with made-up frames.
    fn step(&mut self, now_ms: u64, v: FrameView) {
        let b = v.buttons;
        let mut events: Vec<String> = Vec::new();
        let attack = self.swing.update(SwingInput { now_ms, stamina: v.stamina, right_light: b.right_light, right_strong: b.right_strong, left_light: b.left_light });
        if let Some(a) = attack {
            // an attack sounds like a swing only when the weapon in that hand is one that is swung; when the hands could not
            // be read the answer is "unknown", which counts as a swing
            let class = v.hands.as_ref().and_then(|h| h.class_for(a.hand)).unwrap_or(WeaponClass::Unknown);
            let who = format!("{}{}", if a.hand == Hand::Right { "R" } else { "L" }, if a.strong { "s" } else { "" });
            if class.makes_swing_sound() {
                let slot = match self.combo.swing(a.strong, now_ms) {
                    Swing::Light { step } => format!("chainsword_swing_{step}"),
                    Swing::Strong => "chainsword_strong".to_string(),
                };
                self.play(&slot);
                events.push(format!("swing:{who}:{}", class.name()));
            } else {
                events.push(format!("silent:{who}:{}", class.name()));
            }
        }
        if let Some(counts) = v.ammo {
            let shots = self.ammo.update(counts.as_deref());
            if shots > 0 {
                // bolts only leave a crossbow; when the hands could not be read every shot counts
                if v.hands.as_ref().is_none_or(|h| h.has_crossbow()) {
                    self.shots.add(shots, now_ms);
                    events.push(format!("shot:{shots}"));
                } else {
                    events.push(format!("bolts-used-no-crossbow:{shots}"));
                }
            }
        }
        for _ in 0..self.shots.take_due(now_ms) {
            self.play("boltpistol_fire");
        }
        let hands_text = v.hands.as_ref().map_or("?".to_string(), |h| {
            let c = |hand| h.class_for(hand).map_or("?", |c| c.name());
            format!("R:{}/L:{}", c(Hand::Right), c(Hand::Left))
        });
        self.trace.row(now_ms, v.stamina, v.hp, &b, &hands_text, &events.join("+"));
    }

    /// F5 / F6: the mashup's sounds quieter / louder (3 dB per press, between -30 dB and +9 dB).
    fn change_volume(&mut self, step_db: f32) {
        self.master_db = (self.master_db + step_db).clamp(-30.0, 9.0);
        self.log.log(&format!("master volume {:+.0} dB", self.master_db));
        if self.idle_on {
            // the loop is already playing at the old level: restart it at the new one
            self.audio.stop(KEY_IDLE);
            let random = self.rng.next();
            if let Some(clip) = active_set(&mut self.sounds, &mut self.alt, self.use_alt).pick("chainsword_idle", random) {
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
        match active_set(&mut self.sounds, &mut self.alt, self.use_alt).pick(slot, random) {
            Some(clip) => {
                let gain = self.gain_of(slot, 0.8);
                self.audio.play(clip, gain);
                self.plays += 1;
                if self.plays <= 25 {
                    self.log.log(&format!("playing {slot} (gain {gain:.2}{})", if self.use_alt { ", exact set" } else { "" }));
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
            match active_set(&mut self.sounds, &mut self.alt, self.use_alt).pick("chainsword_idle", random) {
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

    /// F9: switch between the two sound sets and play a swing from the one now in use, so the difference is heard at once.
    fn toggle_set(&mut self) {
        if self.alt.is_none() {
            self.log.log("F9: there is no second sound set (Prepare-AshenMarine.bat makes one when the sound bank can be read exactly)");
            return;
        }
        self.use_alt = !self.use_alt;
        self.log.log(if self.use_alt {
            "F9: now playing the EXACT set (volumes and delays from the game's own sound bank)"
        } else {
            "F9: now playing the CLASSIC set (every sound at full volume, as in kit 4)"
        });
        if self.idle_on {
            self.audio.stop(KEY_IDLE);
            let random = self.rng.next();
            if let Some(clip) = active_set(&mut self.sounds, &mut self.alt, self.use_alt).pick("chainsword_idle", random) {
                let gain = self.gain_of("chainsword_idle", 0.5);
                self.audio.play_loop(KEY_IDLE, clip, gain);
            }
        }
        self.play("chainsword_swing_1");
    }

    /// F8 (private test kits): put the test weapons in the inventory. A weapon that is already there is not given again.
    /// Everything is logged. (The names come from the item-text override that the setup tool writes, not from here.)
    unsafe fn run_experiment(&mut self, pgd: &PlayerGameData) {
        self.log.log("F8 pressed: giving the test weapons (experiment)");
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
        let owned: Vec<u32> = pgd.equipment.equip_inventory_data.items_data.items().filter(|e| e.item_id.category() == ItemCategory::Weapon).map(|e| weapons::base_row(e.item_id.param_id())).collect();
        for (row, qty, what) in experiment_items() {
            let Ok(item) = ItemId::new(ItemCategory::Weapon, row) else { continue };
            if qty == 1 && owned.contains(&row) {
                self.log.log(&format!("F8: {what} (row {row}) is already in the inventory; skipped"));
                continue;
            }
            match reg.get_equip_param(item) {
                Some(_) => {
                    gdm.give_item_directly(item, qty);
                    self.log.log(&format!("F8: gave {qty} x row {row}: {what}"));
                }
                None => self.log.log(&format!("F8: the game has no weapon row {row} ({what}); skipped")),
            }
        }
    }

    /// (item id, quantity) of every bolt stack. `None` when the inventory does not look sane this frame.
    unsafe fn read_bolts(&mut self, pgd: &PlayerGameData, reg: Option<&CSRegulationManager>) -> Option<Vec<(u32, u32)>> {
        let stacks = read_ammo(pgd)?;
        let mut v = Vec::with_capacity(stacks.len());
        for (id, qty) in stacks {
            let row = weapons::base_row(id & 0x0FFF_FFFF);
            let is_bolt = *self.bolt_rows.entry(row).or_insert_with(|| reg.and_then(|r| equip::weapon_category(r, id)) == Some(weapons::CAT_BOLT));
            if is_bolt {
                v.push((id, qty));
            }
        }
        Some(v)
    }
}

/// The set in use: the second one after F9 (when there is one), else the first.
fn active_set<'a>(first: &'a mut SoundSet, second: &'a mut Option<SoundSet>, use_second: bool) -> &'a mut SoundSet {
    match second {
        Some(s) if use_second => s,
        _ => first,
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

// ------------------------------------------------------------------------------------------------ trace

/// What the feature saw, one line whenever something changed, for tuning the trigger rules from a real session.
struct Trace {
    path: PathBuf,
    text: String,
    rows: usize,
    last: Option<(i32, u16, (u8, u8), (bool, bool), bool)>,
    last_flush: Instant,
}

impl Trace {
    fn new(path: PathBuf) -> Trace {
        Trace { path, text: String::from("t_ms,stamina,hp,pad_buttons,pad_lt,pad_rt,lmb,rmb,shift,hands,event\n"), rows: 0, last: None, last_flush: Instant::now() }
    }

    fn row(&mut self, t_ms: u64, stamina: i32, hp: i32, b: &input::Buttons, hands: &str, event: &str) {
        if self.rows >= TRACE_MAX_ROWS {
            return;
        }
        let key = (stamina, b.pad_buttons, b.pad_triggers, b.mouse, b.shift);
        // stamina creeps up one point at a time while resting: only a change of 2 or more, a button change or an event is news
        let news = match self.last {
            None => true,
            Some((s, pb, pt, m, sh)) => (stamina - s).abs() >= 2 || pb != b.pad_buttons || pt != b.pad_triggers || m != b.mouse || sh != b.shift,
        } || !event.is_empty();
        if !news {
            return;
        }
        self.last = Some(key);
        self.rows += 1;
        let _ = writeln!(
            self.text,
            "{t_ms},{stamina},{hp},0x{:04X},{},{},{},{},{},{hands},{event}",
            b.pad_buttons, b.pad_triggers.0, b.pad_triggers.1, b.mouse.0 as u8, b.mouse.1 as u8, b.shift as u8
        );
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
    use equip::{HandItem, Hands};

    #[test]
    fn the_trace_only_records_news() {
        let mut t = Trace::new(std::env::temp_dir().join("ashen-trace-test.csv"));
        let b = input::Buttons::default();
        t.row(0, 100, 500, &b, "R:melee/L:unarmed", "");
        t.row(16, 101, 500, &b, "R:melee/L:unarmed", ""); // regeneration by one point: not news
        t.row(32, 99, 500, &b, "R:melee/L:unarmed", ""); // within 2 of the last recorded value: not news
        t.row(48, 70, 500, &b, "R:melee/L:unarmed", ""); // a real drop
        let pressed = input::Buttons { pad_buttons: 0x0200, ..b };
        t.row(64, 70, 500, &pressed, "R:melee/L:unarmed", ""); // a button
        t.row(80, 70, 500, &pressed, "R:melee/L:unarmed", "swing:R:melee"); // an event
        let shift = input::Buttons { shift: true, ..pressed };
        t.row(96, 70, 500, &shift, "R:melee/L:unarmed", ""); // the shift key
        assert_eq!(t.rows, 5, "{}", t.text);
        assert!(t.text.contains("0x0200") && t.text.contains(",swing:R:melee"));
        assert!(t.text.starts_with("t_ms,stamina,hp,pad_buttons,pad_lt,pad_rt,lmb,rmb,shift,hands,event\n"));
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
        (Sfx::new(log, audio, sounds, None, volumes, input::Input::new(), dir.join("trace.csv"), false), rx, dir)
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

    fn item(class: WeaponClass) -> Option<HandItem> {
        Some(HandItem { raw_id: 2_000_000, class })
    }

    /// Hands as the game reports them with the given weapons in the slots in use.
    fn hands(left: Option<HandItem>, right: Option<HandItem>) -> Option<Hands> {
        let raw = [1, 0, 0, 0, 0, 0, 0];
        Some(Hands { asm: weapons::parse_chr_asm(raw), raw, left, right })
    }

    fn frame(stamina: i32, buttons: input::Buttons, ammo: Option<Option<Vec<(u32, u32)>>>, hands: Option<Hands>) -> FrameView {
        FrameView { stamina, hp: 500, buttons, ammo, hands }
    }

    const NONE: input::Buttons = input::Buttons { right_light: false, right_strong: false, left_light: false, shift: false, pad_buttons: 0, pad_triggers: (0, 0), mouse: (false, false) };

    #[test]
    fn swings_shots_and_the_idle_loop_reach_the_mixer_as_the_sheet_says() {
        let (mut s, rx, _dir) = test_sfx("step");
        let light = input::Buttons { right_light: true, ..NONE };
        let strong = input::Buttons { right_strong: true, ..NONE };
        let h = || hands(item(WeaponClass::Crossbow), item(WeaponClass::Melee));

        // first attack, a second one 0.6 s later (combo step 2), a roll in between (no button) that must stay silent
        s.step(0, frame(100, NONE, None, h()));
        s.step(16, frame(100, light, None, h()));
        s.step(32, frame(85, light, None, h()));
        s.step(300, frame(85, NONE, None, h()));
        s.step(310, frame(60, NONE, None, h())); // a roll
        s.step(620, frame(70, light, None, h()));
        s.step(640, frame(55, light, None, h()));
        assert_eq!(played(&rx), vec!["play:chainsword_swing_1", "play:chainsword_swing_2"]);
        // a strong attack
        s.step(2500, frame(100, NONE, None, h()));
        s.step(2516, frame(100, strong, None, h()));
        s.step(2532, frame(70, strong, None, h()));
        assert_eq!(played(&rx), vec!["play:chainsword_strong"]);
        // shots with the crossbow in the left hand: bolts 60 -> 59 -> 57
        s.step(3000, frame(100, NONE, Some(Some(vec![(404_000, 60)])), h()));
        s.step(3100, frame(100, NONE, Some(Some(vec![(404_000, 59)])), h()));
        s.step(3200, frame(100, NONE, Some(Some(vec![(404_000, 57)])), h()));
        s.step(3210, frame(100, NONE, None, h())); // the second of the two shots is 110 ms after the first
        s.step(3320, frame(100, NONE, Some(None), h())); // unreadable inventory: nothing
        assert_eq!(played(&rx), vec!["play:boltpistol_fire", "play:boltpistol_fire", "play:boltpistol_fire"]);
        // F7 twice: start sound + loop, then stop
        s.toggle_idle();
        s.toggle_idle();
        assert_eq!(played(&rx), vec!["play:chainsword_idle_start", "loop:chainsword_idle", "stop:"]);
        assert!(s.trace.text.contains(",swing:R:melee") && s.trace.text.contains(",shot:"));
    }

    #[test]
    fn a_crossbow_shot_with_the_right_mouse_button_is_not_a_sword_swing() {
        // the user's real session: crossbow in the left hand, sword in the right; the right mouse button fires the crossbow (-26 stamina)
        let (mut s, rx, _dir) = test_sfx("crossbow");
        let right_mouse = input::Buttons { left_light: true, mouse: (false, true), ..NONE };
        let h = || hands(item(WeaponClass::Crossbow), item(WeaponClass::Melee));
        s.step(0, frame(95, NONE, None, h()));
        s.step(16, frame(95, right_mouse, None, h()));
        s.step(32, frame(69, right_mouse, None, h()));
        assert!(played(&rx).is_empty(), "no melee sound for a crossbow");
        assert!(s.trace.text.contains(",silent:L:crossbow"), "{}", s.trace.text);
        // the sword in the right hand with the left mouse button still sounds
        let left_mouse = input::Buttons { right_light: true, mouse: (true, false), ..NONE };
        s.step(2000, frame(95, NONE, None, h()));
        s.step(2016, frame(95, left_mouse, None, h()));
        s.step(2032, frame(78, left_mouse, None, h()));
        assert_eq!(played(&rx), vec!["play:chainsword_swing_1"]);
    }

    #[test]
    fn a_crossbow_in_the_right_hand_fires_with_the_attack_button_without_a_swing_sound() {
        let (mut s, rx, _dir) = test_sfx("crossbow-right");
        let light = input::Buttons { right_light: true, ..NONE };
        let h = || hands(item(WeaponClass::Unarmed), item(WeaponClass::Crossbow));
        s.step(0, frame(95, NONE, Some(Some(vec![(404_000, 60)])), h()));
        s.step(16, frame(95, light, None, h()));
        s.step(32, frame(69, light, None, h()));
        s.step(400, frame(95, NONE, Some(Some(vec![(404_000, 59)])), h()));
        assert_eq!(played(&rx), vec!["play:boltpistol_fire"]);
    }

    #[test]
    fn a_shield_or_a_catalyst_makes_no_swing_sound_and_bolts_without_a_crossbow_make_no_shot_sound() {
        let (mut s, rx, _dir) = test_sfx("others");
        let left = input::Buttons { left_light: true, ..NONE };
        let h = || hands(item(WeaponClass::Shield), item(WeaponClass::Catalyst));
        s.step(0, frame(95, NONE, Some(Some(vec![(404_000, 60)])), h()));
        s.step(16, frame(95, left, None, h()));
        s.step(32, frame(80, left, None, h()));
        s.step(300, frame(95, NONE, Some(Some(vec![(404_000, 59)])), h())); // a bolt dropped on the floor, say
        assert!(played(&rx).is_empty(), "{:?}", played(&rx));
        assert!(s.trace.text.contains("silent:L:shield") && s.trace.text.contains("bolts-used-no-crossbow:1"), "{}", s.trace.text);
    }

    #[test]
    fn when_the_hands_cannot_be_read_everything_sounds_as_in_kit_4() {
        let (mut s, rx, _dir) = test_sfx("unknown");
        let right_mouse = input::Buttons { left_light: true, ..NONE };
        s.step(0, frame(95, NONE, Some(Some(vec![(404_000, 60)])), None));
        s.step(16, frame(95, right_mouse, None, None));
        s.step(32, frame(70, right_mouse, None, None));
        s.step(400, frame(95, NONE, Some(Some(vec![(404_000, 59)])), None));
        assert_eq!(played(&rx), vec!["play:chainsword_swing_1", "play:boltpistol_fire"]);
    }

    #[test]
    fn a_burst_taken_at_once_is_played_as_three_shots_spaced_apart() {
        let (mut s, rx, _dir) = test_sfx("burst");
        let h = || hands(item(WeaponClass::Crossbow), item(WeaponClass::Melee));
        s.step(0, frame(100, NONE, Some(Some(vec![(404_000, 60)])), h()));
        s.step(1000, frame(100, NONE, Some(Some(vec![(404_000, 57)])), h()));
        assert_eq!(played(&rx), vec!["play:boltpistol_fire"], "the first at once");
        s.step(1050, frame(100, NONE, None, h()));
        assert!(played(&rx).is_empty());
        s.step(1110, frame(100, NONE, None, h()));
        assert_eq!(played(&rx), vec!["play:boltpistol_fire"]);
        s.step(1220, frame(100, NONE, None, h()));
        assert_eq!(played(&rx), vec!["play:boltpistol_fire"]);
        s.step(1500, frame(100, NONE, None, h()));
        assert!(played(&rx).is_empty());
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

    /// A second set whose clips are twice as long as the first set's (so the test can tell which set played).
    fn with_alt(s: &mut Sfx, dir: &Path) {
        let alt_dir = dir.join("alt");
        std::fs::create_dir_all(&alt_dir).unwrap();
        let sheet = soundset::sheet_slots();
        let mut slots = Vec::new();
        for (i, sl) in sheet.iter().enumerate() {
            let name = format!("{}_1.wav", sl.id);
            std::fs::write(alt_dir.join(&name), ashen_common::wav::build(1, 44_100, &vec![1000i16; 441 * (i + 1) * 2])).unwrap();
            slots.push(format!(r#"{{"slot":"{}","event":"{}","loop":{},"files":["{name}"]}}"#, sl.id, sl.sm2_event, sl.looped));
        }
        std::fs::write(alt_dir.join("index.json"), format!(r#"{{"format":1,"sounds":[{}]}}"#, slots.join(","))).unwrap();
        s.alt = Some(SoundSet::load(&alt_dir).unwrap().set);
    }

    fn frames_of(msg: &audio::Msg) -> usize {
        match msg {
            audio::Msg::Play(c, _) | audio::Msg::Loop(_, c, _) => c.len_frames(),
            audio::Msg::Stop(_) => 0,
        }
    }

    #[test]
    fn f9_switches_between_the_two_sets_and_previews_a_swing() {
        let (mut s, rx, dir) = test_sfx("sets");
        // without a second set F9 does nothing but say so
        s.toggle_set();
        assert!(rx.try_recv().is_err());
        assert!(std::fs::read_to_string(dir.join("sfx.txt")).unwrap().contains("no second sound set"));
        with_alt(&mut s, &dir);
        let swing_1 = soundset::sheet_slots().iter().position(|x| x.id == "chainsword_swing_1").unwrap() + 1;
        s.toggle_set();
        let m = rx.try_recv().expect("a preview swing from the second set");
        assert_eq!(frames_of(&m), 441 * swing_1 * 2, "the second set's clip");
        s.play("chainsword_swing_1");
        assert_eq!(frames_of(&rx.try_recv().unwrap()), 441 * swing_1 * 2);
        s.toggle_set();
        assert_eq!(frames_of(&rx.try_recv().unwrap()), 441 * swing_1, "back to the first set");
        // a running idle loop is restarted from the set now in use
        s.idle_on = true;
        s.toggle_set();
        let msgs: Vec<audio::Msg> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(matches!(msgs[0], audio::Msg::Stop(_)) && matches!(msgs[1], audio::Msg::Loop(..)), "{}", msgs.len());
        let text = std::fs::read_to_string(dir.join("sfx.txt")).unwrap();
        assert!(text.contains("now playing the EXACT set") && text.contains("now playing the CLASSIC set"), "{text}");
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
    fn f8_gives_what_the_weapons_sheet_says() {
        let items = experiment_items();
        let rows: Vec<(u32, u32)> = items.iter().map(|(r, q, _)| (*r, *q)).collect();
        assert_eq!(rows, vec![(2_000_000, 1), (14_090_000, 1), (404_000, EXPERIMENT_AMMO)]);
    }

    #[test]
    fn equipment_changes_are_logged_once_each_and_the_log_is_capped() {
        let (mut s, _rx, dir) = test_sfx("equipment-log");
        let a = hands(item(WeaponClass::Crossbow), item(WeaponClass::Melee)).unwrap();
        s.note_hands(&a);
        s.note_hands(&a); // unchanged: nothing
        let b = hands(item(WeaponClass::Unarmed), item(WeaponClass::Melee)).unwrap();
        s.note_hands(&b);
        let text = std::fs::read_to_string(dir.join("sfx.txt")).unwrap();
        assert_eq!(text.matches("equipment").count(), 2, "{text}");
        assert!(text.contains("equipment: left hand slot 1: crossbow") && text.contains("equipment changed: left hand slot 1: unarmed"), "{text}");
        for k in 0..400 {
            let c = hands(Some(HandItem { raw_id: 2_000_000 + k, class: WeaponClass::Melee }), item(WeaponClass::Melee)).unwrap();
            s.note_hands(&c);
        }
        assert_eq!(s.equipment_log_lines, MAX_EQUIPMENT_LOG_LINES);
    }
}
