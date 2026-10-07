//! The Dark Souls III probe: a strictly READ-ONLY look at the running game, for private test kits. It answers the
//! questions the weapons feature depends on, with real data from the player's own game:
//!   * which game build is this, and is it one the `darksouls3` crate supports?
//!   * what does the weapon parameter table look like (ids, categories, models, icons, damage)?
//!   * what is in the inventory, and how do stamina / health / item counts change while the player plays?
//!   * where does the game keep its text tables (item names), and can they be read and parsed?
//!   * can the mashup play sound next to the game's own audio?
//!
//! Output goes next to the hook log: probe-ds3.txt, probe-ds3-weapons.csv, probe-ds3-inventory.csv,
//! probe-ds3-samples.csv and probe-ds3-text-*.csv. Nothing here writes to game memory or calls a game function.
//!
//! Lesson from the first real run: a step must never switch the whole probe off. Every step is tried on its own, a
//! failed step is retried later and only given up after several failures, and pointers are checked with
//! `ReadProcessMemory` before they are followed.
use super::{audio, gametask, memscan, version};
use ashen_common::{config::HookConfig, fmg, logging::Logger, mixer::Clip, VERSION};
use darksouls3::param::{EQUIP_PARAM_WEAPON_ST, ParamDef};
use darksouls3::sprj::{CSRegulationManager, GameDataMan, ParamResCap, PlayerIns};
use fromsoftware_shared::FromStatic;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

static FRAMES: AtomicU64 = AtomicU64::new(0);
/// Set by the in-game task once a real world is loaded (not the title screen).
static IN_WORLD: AtomicBool = AtomicBool::new(false);

/// The anchors that identify the item-name tables (exact whole strings, English game).
const ANCHORS: [&str; 10] = ["Straight Sword", "Dagger", "Broadsword", "Longsword", "Short Bow", "Light Crossbow", "Avelyn", "Bolt", "Estus Flask", "Uchigatana"];

fn dir_of(cfg: &HookConfig) -> PathBuf {
    Path::new(&cfg.log_file).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Runs on the features thread for the life of the process.
pub fn thread_body(cfg: HookConfig) {
    let dir = dir_of(&cfg);
    let log = Arc::new(Logger::open(&dir.join("probe-ds3.txt"), "probe"));
    log.log(&format!("DS3 probe v{VERSION} starting (read-only: no game memory is written, no game function is called)"));

    let ver = version::detect();
    let supported = match &ver {
        Ok(v) => {
            log.log(&format!("game: product {:?}, version {}, language id 0x{:02X}", v.product, v.version, v.lang));
            let s = version::supported(v);
            log.log(&format!("this build is {} by the darksouls3 crate (it knows 1.15.2.0 English and 1.15.2.1 Japanese)", if s { "SUPPORTED" } else { "NOT SUPPORTED" }));
            s
        }
        Err(e) => {
            log.log(&format!("cannot read the game version: {e}"));
            false
        }
    };

    // The task handle must stay alive for the whole process: dropping it only sets a flag the game ignores.
    let mut _task_handle = None;
    if supported {
        match catch_unwind(AssertUnwindSafe(|| register(&cfg, &log))) {
            Ok(Ok(h)) => _task_handle = Some(h),
            Ok(Err(e)) => log.log(&format!("could not start the in-game probe: {e}")),
            Err(p) => log.log(&format!("the in-game probe crashed while starting: {}", panic_text(&*p))),
        }
    } else {
        log.log("in-game probe skipped (unsupported build); only the text-table scan and the sound check run");
    }

    // what happens when: some relative to the start, some relative to "the player is in a world"
    let start = Instant::now();
    let mut world_since: Option<Instant> = None;
    let mut scans = [false; 6];
    let mut beep_title = false;
    let mut beep_world = false;
    let mut last_frames = 0u64;
    let mut last_beat = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let t = start.elapsed();
        if IN_WORLD.load(Ordering::Relaxed) && world_since.is_none() {
            world_since = Some(Instant::now());
            log.log(&format!("a world is loaded (the player is in the game) {:.0}s after the probe started", t.as_secs_f32()));
        }
        let w = world_since.map(|w| w.elapsed());

        // sound check at the title screen (does not depend on the world being detected) and again in the world
        if !beep_title && t >= Duration::from_secs(20) {
            beep_title = true;
            audio_check(&log, "title screen");
        }
        if !beep_world && w.is_some_and(|w| w >= Duration::from_secs(6)) {
            beep_world = true;
            audio_check(&log, "in the world");
        }

        // text-table scans: two by the clock, four after the world loaded (item names are loaded by then, if ever)
        let due = [
            t >= Duration::from_secs(25),
            t >= Duration::from_secs(60),
            w.is_some_and(|w| w >= Duration::from_secs(10)),
            w.is_some_and(|w| w >= Duration::from_secs(45)),
            w.is_some_and(|w| w >= Duration::from_secs(120)),
            w.is_some_and(|w| w >= Duration::from_secs(240)),
        ];
        for (i, d) in due.iter().enumerate() {
            if *d && !scans[i] {
                scans[i] = true;
                let round = i + 1;
                if let Err(p) = catch_unwind(AssertUnwindSafe(|| scan_text_tables(&log, &dir, round, round >= 5))) {
                    log.log(&format!("text-table scan {round} crashed: {}", panic_text(&*p)));
                }
                break; // one scan per half second keeps the game smooth
            }
        }

        if last_beat.elapsed() >= Duration::from_secs(30) {
            last_beat = Instant::now();
            let f = FRAMES.load(Ordering::Relaxed);
            if f != last_frames {
                log.log(&format!("in-game task alive: {f} frames so far; in a world: {}", IN_WORLD.load(Ordering::Relaxed)));
                last_frames = f;
            } else if supported && t.as_secs() > 60 && f == 0 {
                log.log("in-game task has NOT run once (registered but never called)");
            }
        }
    }
}

/// Two short beeps through the mashup's own output path, so the player can confirm sound works inside the game.
fn audio_check(log: &Arc<Logger>, when: &str) {
    match catch_unwind(AssertUnwindSafe(|| audio::start(log.clone()))) {
        Ok(Some(a)) => {
            a.play(Arc::new(Clip::tone(523.0, 250, 0.25)), 1.0);
            std::thread::sleep(Duration::from_millis(450));
            a.play(Arc::new(Clip::tone(784.0, 250, 0.25)), 1.0);
            std::thread::sleep(Duration::from_millis(400));
            log.log(&format!("audio check ({when}): two beeps were sent to the sound device (a low and a high beep should be audible)"));
            // the device thread ends when the handle is dropped
        }
        Ok(None) => log.log(&format!("audio check ({when}): no sound device could be opened")),
        Err(p) => log.log(&format!("audio check ({when}) crashed: {}", panic_text(&*p))),
    }
}

// ------------------------------------------------------------------------------------------ in-game task

fn register(cfg: &HookConfig, log: &Arc<Logger>) -> Result<fromsoftware_shared::RecurringTaskHandle<usize>, String> {
    let mut state = ProbeState::new(cfg, log.clone());
    gametask::register(log, "the probe", move || {
        FRAMES.fetch_add(1, Ordering::Relaxed);
        state.frame();
    })
}

/// One part of the probe. A part that is not ready is tried again later; one that crashes is tried again a few
/// times and then given up; neither ever stops the other parts.
#[derive(Default)]
struct Step {
    done: bool,
    fails: u32,
    gave_up: bool,
    tries: u32,
}

impl Step {
    const MAX_FAILS: u32 = 5;

    fn active(&self) -> bool {
        !self.done && !self.gave_up
    }

    /// `f` returns Ok(true) when finished, Ok(false) when the game is not ready yet, Err(why) for a soft failure.
    fn run(&mut self, log: &Logger, name: &str, f: impl FnOnce() -> Result<bool, String>) {
        self.tries += 1;
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(Ok(true)) => self.done = true,
            Ok(Ok(false)) => {}
            Ok(Err(why)) => {
                self.fails += 1;
                if self.fails <= 3 {
                    log.log(&format!("{name}: {why}"));
                }
                if self.fails >= Self::MAX_FAILS {
                    self.gave_up = true;
                    log.log(&format!("{name}: given up after {} failures", self.fails));
                }
            }
            Err(p) => {
                self.fails += 1;
                log.log(&format!("{name}: step crashed ({}), failure {} of {}", panic_text(&*p), self.fails, Self::MAX_FAILS));
                if self.fails >= Self::MAX_FAILS {
                    self.gave_up = true;
                    log.log(&format!("{name}: given up"));
                }
            }
        }
    }
}

struct ProbeState {
    dir: PathBuf,
    log: Arc<Logger>,
    t0: Instant,
    last_tick: Instant,
    last_param_try: Instant,
    params: Step,
    table_list: Step,
    player_dump: Step,
    singletons_logged: bool,
    title_logged: bool,
    last_player_note: Instant,
    last_values: Option<(i32, i32, i32, u32, u32, u32)>,
    last_slots: Option<[i32; 22]>,
    last_inventory: HashMap<u32, u32>,
    last_inventory_check: Instant,
    samples: String,
    samples_rows: usize,
    last_flush: Instant,
    finished: bool,
}

/// Is `len` bytes at `addr` readable right now?
fn readable(addr: usize, len: usize) -> bool {
    memscan::readable(addr, len)
}

impl ProbeState {
    fn new(cfg: &HookConfig, log: Arc<Logger>) -> Self {
        ProbeState {
            dir: dir_of(cfg),
            log,
            t0: Instant::now(),
            last_tick: Instant::now(),
            last_param_try: Instant::now(),
            params: Step::default(),
            table_list: Step::default(),
            player_dump: Step::default(),
            singletons_logged: false,
            title_logged: false,
            last_player_note: Instant::now(),
            last_values: None,
            last_slots: None,
            last_inventory: HashMap::new(),
            last_inventory_check: Instant::now(),
            samples: String::from("t_ms,chr_hp,chr_fp,chr_stamina,save_hp,save_mp,save_stamina\n"),
            samples_rows: 0,
            last_flush: Instant::now(),
            finished: false,
        }
    }

    /// Called by the game once per frame, on the game's own thread.
    fn frame(&mut self) {
        if self.finished || self.last_tick.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last_tick = Instant::now();
        if self.t0.elapsed() > Duration::from_secs(25 * 60) {
            self.flush_samples();
            self.finished = true;
            self.log.log("probe finished sampling (25 minutes); it stays quiet from here");
            return;
        }
        unsafe { self.tick() }
    }

    unsafe fn tick(&mut self) {
        // --- parameter tables: not before the game has registered and loaded them; retried every 2 s
        if (self.params.active() || self.table_list.active()) && self.last_param_try.elapsed() >= Duration::from_secs(2) {
            self.last_param_try = Instant::now();
            let log = self.log.clone();
            let dir = self.dir.clone();
            if self.table_list.active() {
                let tries = self.table_list.tries;
                self.table_list.run(&log, "parameter table list", || list_tables(&log, tries));
            }
            if self.params.active() {
                let tries = self.params.tries;
                self.params.run(&log, "weapon parameter table", || match dump_weapon_params(&log, &dir) {
                    Ok(true) => Ok(true),
                    Ok(false) => {
                        if tries.is_multiple_of(15) {
                            log.log(&format!("parameter tables not ready yet (check {})", tries + 1));
                        }
                        Ok(false)
                    }
                    Err(e) => Err(e),
                });
            }
        }

        // --- the player
        let log = self.log.clone();
        let player = match PlayerIns::local_player() {
            Ok(p) => p,
            Err(e) => {
                // say so now and then, so a silent failure to find the player is visible in the log
                if self.last_player_note.elapsed() >= Duration::from_secs(20) {
                    self.last_player_note = Instant::now();
                    log.log(&format!("no local player yet ({e})"));
                }
                return;
            }
        };
        if !self.singletons_logged {
            self.singletons_logged = true;
            log.log(&format!(
                "a local player exists: GameDataMan {}, MapItemMan {}",
                if GameDataMan::instance().is_ok() { "present" } else { "absent" },
                if darksouls3::sprj::MapItemMan::instance().is_ok() { "present" } else { "absent" }
            ));
        }
        let pgd = player.player_game_data.as_ref();
        if pgd.equipment.is_main_menu() {
            // the synthetic character the game keeps on the title screen
            if !self.title_logged {
                self.title_logged = true;
                log.log("the player data is the title-screen placeholder (12 items): waiting for a real character");
            }
            return;
        }
        if self.player_dump.active() {
            let dir = self.dir.clone();
            self.player_dump.run(&log, "player dump", || {
                dump_player(&log, &dir, player);
                Ok(true)
            });
            if self.player_dump.done {
                IN_WORLD.store(true, Ordering::Relaxed);
            }
        }
        // the player is in a world even if the dump step has trouble: that is what the beep test waits for
        if !IN_WORLD.load(Ordering::Relaxed) && self.player_dump.gave_up {
            IN_WORLD.store(true, Ordering::Relaxed);
        }

        // --- live values: the character's own data module, and the save-side copy, side by side
        let m = &player.super_chr_ins.modules.data;
        let cur = (m.hp, m.fp, m.stamina, pgd.player_info.hp, pgd.player_info.mp, pgd.player_info.stamina);
        if self.last_values != Some(cur) {
            self.last_values = Some(cur);
            let _ = writeln!(self.samples, "{},{},{},{},{},{},{}", self.t0.elapsed().as_millis(), cur.0, cur.1, cur.2, cur.3, cur.4, cur.5);
            self.samples_rows += 1;
        }
        // written every few seconds (the first real run lost its samples because the game was closed before the 100th row)
        if self.samples_rows > 0 && self.last_flush.elapsed() >= Duration::from_secs(3) {
            self.flush_samples();
        }
        if self.last_slots != Some(pgd.equipment.equipment_indexes) {
            let first = self.last_slots.is_none();
            self.last_slots = Some(pgd.equipment.equipment_indexes);
            log.log(&format!("{} equipment slot -> inventory index: {:?}", if first { "initial" } else { "CHANGED" }, pgd.equipment.equipment_indexes));
        }
        if self.last_inventory_check.elapsed() > Duration::from_millis(500) {
            self.last_inventory_check = Instant::now();
            let mut now: HashMap<u32, u32> = HashMap::new();
            for e in pgd.equipment.equip_inventory_data.items_data.items() {
                *now.entry(e.item_id.into_inner()).or_default() += e.quantity;
            }
            if !self.last_inventory.is_empty() && now != self.last_inventory {
                let mut keys: Vec<u32> = now.keys().chain(self.last_inventory.keys()).copied().collect();
                keys.sort_unstable();
                keys.dedup();
                for k in keys {
                    let (a, b) = (self.last_inventory.get(&k).copied().unwrap_or(0), now.get(&k).copied().unwrap_or(0));
                    if a != b {
                        log.log(&format!("inventory change: item 0x{k:08X} {a} -> {b}"));
                    }
                }
            }
            self.last_inventory = now;
        }
    }

    fn flush_samples(&mut self) {
        self.last_flush = Instant::now();
        let _ = std::fs::write(self.dir.join("probe-ds3-samples.csv"), &self.samples);
    }
}

/// Follow the pointers to parameter table `idx` with every hop checked first. Ok(table name, rows) when it is there.
unsafe fn table_info(reg: &CSRegulationManager, idx: usize) -> Result<(String, usize), String> {
    let n = reg.params.len();
    if n <= idx {
        return Err(format!("only {n} parameter tables are registered so far"));
    }
    let cap = reg.params[idx].as_ptr() as usize;
    if !readable(cap, core::mem::size_of::<ParamResCap>()) {
        return Err(format!("table slot {idx} is not readable yet"));
    }
    let cap_ref: &ParamResCap = &*(cap as *const ParamResCap);
    let fd4 = cap_ref.param.as_ptr() as usize;
    if !readable(fd4, 0x70) {
        return Err(format!("table {idx}: resource not readable yet"));
    }
    let fd4_ref = &*(fd4 as *const darksouls3::sprj::FD4ParamResCap);
    let table = fd4_ref.table.as_ptr() as usize;
    if !readable(table, 0x40) {
        return Err(format!("table {idx}: data not loaded yet"));
    }
    let table_ref = &*(table as *const darksouls3::sprj::ParamTable);
    // the table's own name sits behind all of its rows, so for big tables the offset is large (the first real run
    // wrongly refused anything over 4 KB, which hid the weapon table); it must still lie inside the table
    let name_at = table + table_ref.name_offset;
    if table_ref.name_offset < 0x40 || table_ref.name_offset > 256 << 20 || !readable(name_at, 8) {
        return Err(format!("table {idx}: name not readable yet"));
    }
    let mut name = Vec::new();
    for i in 0..64 {
        let mut b = [0u8; 1];
        if memscan::read_into(name_at + i, &mut b) != 1 || b[0] == 0 {
            break;
        }
        name.push(b[0]);
    }
    Ok((String::from_utf8_lossy(&name).to_string(), table_ref.length as usize))
}

/// Log every parameter table the game has registered (name and row count), once. `tries` counts earlier attempts:
/// after 20 the list is logged as it is, even if some tables are still not readable.
unsafe fn list_tables(log: &Logger, tries: u32) -> Result<bool, String> {
    let reg = CSRegulationManager::instance().map_err(|e| format!("{e}"))?;
    let n = reg.params.len();
    if n < 100 && tries < 20 {
        return Ok(false); // the real list has well over 100 tables
    }
    let mut line = String::new();
    let mut ok = 0;
    for idx in 0..n {
        match table_info(reg, idx) {
            Ok((name, rows)) => {
                ok += 1;
                let _ = write!(line, "{idx}:{name}({rows}) ");
            }
            Err(_) => {
                let _ = write!(line, "{idx}:? ");
            }
        }
    }
    if ok < n && tries < 20 {
        return Ok(false);
    }
    log.log(&format!("{n} parameter tables registered, {ok} readable: {line}"));
    Ok(true)
}

/// The name the game keeps for a parameter row (its third header word is the offset of the name from the start of the
/// table). UTF-16 or single-byte text; empty when there is none.
unsafe fn row_name(table_addr: usize, info: &darksouls3::sprj::ParamRowInfo) -> String {
    let raw = (info as *const darksouls3::sprj::ParamRowInfo as *const u64).add(2).read_unaligned();
    if raw == 0 || raw > 256 << 20 {
        return String::new();
    }
    let at = table_addr + raw as usize;
    let mut b = [0u8; 96];
    let n = memscan::read_into(at, &mut b);
    if n < 2 {
        return String::new();
    }
    let b = &b[..n];
    if b[1] == 0 && b[0] != 0 {
        // UTF-16
        let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
        String::from_utf16_lossy(&units)
    } else {
        let end = b.iter().position(|&x| x == 0).unwrap_or(b.len());
        String::from_utf8_lossy(&b[..end]).to_string()
    }
}

fn csv_text(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Dump the weapon table (and the goods table's names) once it is loaded. Ok(false) = not loaded yet.
unsafe fn dump_weapon_params(log: &Logger, dir: &Path) -> Result<bool, String> {
    let reg = match CSRegulationManager::instance() {
        Ok(r) => r,
        Err(_) => return Ok(false),
    };
    let idx = EQUIP_PARAM_WEAPON_ST::INDEX;
    match table_info(reg, idx) {
        Err(_) => return Ok(false),
        Ok((name, rows)) => {
            if name != EQUIP_PARAM_WEAPON_ST::NAME {
                return Err(format!("table {idx} is named {name:?}, not {:?}: the table order differs from the crate's", EQUIP_PARAM_WEAPON_ST::NAME));
            }
            if rows == 0 {
                return Ok(false);
            }
        }
    }
    let weapons = reg.get_param::<EQUIP_PARAM_WEAPON_ST>();
    let table_addr = &weapons.table as *const darksouls3::sprj::ParamTable as usize;
    let mut csv = String::from("id,name,weapon_category,wepmotion_category,equip_model_id,icon_id,atk_base_physics,weight,durability,behavior_variation_id,arrow_bolt_equip_id,max_arrow_quantity,wep_se_id_offset\n");
    let mut n = 0usize;
    let mut named = 0usize;
    for info in weapons.table.row_info() {
        let Some(row) = weapons.get(info.id) else { continue };
        n += 1;
        let name = row_name(table_addr, info);
        if !name.is_empty() {
            named += 1;
        }
        let _ = writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{},{},{},{}",
            info.id,
            csv_text(&name),
            row.weapon_category(),
            row.wepmotion_category(),
            row.equip_model_id(),
            row.icon_id(),
            row.atk_base_physics(),
            row.weight(),
            row.durability(),
            row.behavior_variation_id(),
            row.arrow_bolt_equip_id(),
            row.max_arrow_quantity(),
            row.wep_se_id_offset()
        );
    }
    let _ = std::fs::write(dir.join("probe-ds3-weapons.csv"), csv);
    log.log(&format!("weapon parameter table: {n} rows ({named} with a row name) written to probe-ds3-weapons.csv"));

    // the goods table (Estus Flask and friends): only the names, to compare them with the item-name text later
    let gidx = darksouls3::param::EQUIP_PARAM_GOODS_ST::INDEX;
    if let Ok((gname, grows)) = table_info(reg, gidx) {
        if gname == darksouls3::param::EQUIP_PARAM_GOODS_ST::NAME && grows > 0 {
            let goods = reg.get_param::<darksouls3::param::EQUIP_PARAM_GOODS_ST>();
            let gaddr = &goods.table as *const darksouls3::sprj::ParamTable as usize;
            let mut g = String::from("id,name\n");
            for info in goods.table.row_info() {
                let _ = writeln!(g, "{},{}", info.id, csv_text(&row_name(gaddr, info)));
            }
            let _ = std::fs::write(dir.join("probe-ds3-goods.csv"), g);
            log.log(&format!("goods parameter table: {grows} rows written to probe-ds3-goods.csv"));
        }
    }
    Ok(true)
}

unsafe fn dump_player(log: &Logger, dir: &Path, player: &mut PlayerIns) {
    let pgd = player.player_game_data.as_ref();
    let info = &pgd.player_info;
    let len = info.character_name.iter().position(|c| *c == 0).unwrap_or(info.character_name.len());
    // the character's name is not logged (nothing here needs it, and logs get sent around)
    log.log(&format!(
        "player: name has {len} characters, vigor {} attunement {} endurance {} strength {} dexterity {} intelligence {} faith {} luck {} vitality {}",
        info.vigor, info.attunement, info.endurance, info.strength, info.dexterity, info.intelligence, info.faith, info.luck, info.vitality
    ));
    let items = &pgd.equipment.equip_inventory_data.items_data;
    log.log(&format!("inventory: {} normal + {} key items, capacity {}", items.normal_items_count, items.key_items_count, items.total_capacity));
    let mut csv = String::from("item_id_hex,category,param_id,quantity\n");
    for e in items.items() {
        let _ = writeln!(csv, "0x{:08X},{:?},{},{}", e.item_id.into_inner(), e.item_id.category(), e.item_id.param_id(), e.quantity);
    }
    let _ = std::fs::write(dir.join("probe-ds3-inventory.csv"), csv);
    log.log("inventory written to probe-ds3-inventory.csv");
}

// ------------------------------------------------------------------------------------------ text tables

fn content_hash(f: &fmg::Fmg) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f.entries.hash(&mut h);
    h.finish()
}

struct FoundFmg {
    addr: usize,
    fmg: fmg::Fmg,
}

/// A header that looks right but whose body did not parse: shown in the log so the layout can be corrected.
struct NearMiss {
    addr: usize,
    size: usize,
    head: Vec<u8>,
    first_offsets: Vec<u64>,
}

#[cfg(test)]
fn find_fmgs(log: &Logger, budget: Duration) -> Vec<FoundFmg> {
    find_fmgs_with_misses(log, budget).0
}

fn find_fmgs_with_misses(log: &Logger, budget: Duration) -> (Vec<FoundFmg>, Vec<NearMiss>) {
    const CHUNK: usize = 8 << 20;
    const OVERLAP: usize = 64;
    let started = Instant::now();
    let finder = memchr::memmem::Finder::new(&[0u8, 0, 2, 0]);
    let mut found: Vec<FoundFmg> = Vec::new();
    let mut misses: Vec<NearMiss> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let regions = memscan::regions(1 << 30);
    let total: usize = regions.iter().map(|r| r.size).sum();
    log.log(&format!("scanning {} memory regions, {} MB, for text tables", regions.len(), total >> 20));
    let mut buf = vec![0u8; CHUNK];
    'regions: for r in regions {
        let mut off = 0usize;
        while off < r.size {
            if started.elapsed() > budget {
                log.log("text-table scan stopped: time budget used up");
                break 'regions;
            }
            let want = CHUNK.min(r.size - off);
            let got = memscan::read_into(r.base + off, &mut buf[..want]);
            if got >= fmg::HEADER_LEN {
                for p in finder.find_iter(&buf[..got]) {
                    if p + fmg::HEADER_LEN > got {
                        continue;
                    }
                    let Some(size) = fmg::header_file_size(&buf[p..p + fmg::HEADER_LEN]) else { continue };
                    let addr = r.base + off + p;
                    if !seen.insert(addr) {
                        continue;
                    }
                    if let Some(bytes) = memscan::read(addr, size) {
                        match fmg::parse_at(&bytes, addr as u64) {
                            Some(f) => found.push(FoundFmg { addr, fmg: f }),
                            None if misses.len() < 6 => {
                                let table = u64::from_le_bytes(bytes[0x18..0x20].try_into().unwrap()) as usize;
                                let first_offsets = (0..6).filter_map(|i| bytes.get(table + i * 8..table + i * 8 + 8)).map(|b| u64::from_le_bytes(b.try_into().unwrap())).collect();
                                misses.push(NearMiss { addr, size, head: bytes[..fmg::HEADER_LEN].to_vec(), first_offsets });
                            }
                            None => {}
                        }
                    }
                }
            }
            if off + want >= r.size {
                break;
            }
            off += want.saturating_sub(OVERLAP).max(1);
        }
    }
    (found, misses)
}

fn scan_text_tables(log: &Logger, dir: &Path, round: usize, final_round: bool) {
    log.log(&format!("--- text-table scan {round} ---"));
    let (found, misses) = find_fmgs_with_misses(log, Duration::from_secs(45));
    log.log(&format!("found {} text tables that parse as FMG, {} more with a table-like header that did not parse", found.len(), misses.len()));
    for m in &misses {
        log.log(&format!("  near miss at 0x{:X} ({} bytes): header {:02x?}, first string offsets {:#x?}", m.addr, m.size, m.head, m.first_offsets));
    }
    let mut dumped = 0usize;
    let mut first_of: HashMap<u64, usize> = HashMap::new();
    let no_english = found.iter().all(|g| !ANCHORS.iter().any(|a| g.fmg.entries.iter().any(|(_, t)| t == a)));
    for (i, f) in found.iter().enumerate() {
        let ids = (f.fmg.entries.iter().map(|e| e.0).min(), f.fmg.entries.iter().map(|e| e.0).max());
        let hits: Vec<&str> = ANCHORS.iter().copied().filter(|a| f.fmg.entries.iter().any(|(_, t)| t == a)).collect();
        let sample: Vec<String> = f.fmg.entries.iter().take(3).map(|(id, t)| format!("{id}={:?}", t.chars().take(24).collect::<String>())).collect();
        let hash = content_hash(&f.fmg);
        if let Some(first) = first_of.get(&hash) {
            log.log(&format!("  text table #{i} at 0x{:X}: same content as #{first} (a second copy in memory)", f.addr));
            continue;
        }
        first_of.insert(hash, i);
        log.log(&format!(
            "  text table #{i} at 0x{:X}: {} bytes, {} strings in {} groups, ids {:?}..{:?}, anchors {:?}, e.g. {}",
            f.addr,
            f.fmg.file_size,
            f.fmg.entries.len(),
            f.fmg.group_count,
            ids.0,
            ids.1,
            hits,
            sample.join(" ")
        ));
        // item-name tables (anchors found) are dumped in full, once per distinct content; with no English anchors
        // anywhere, the final scan dumps the biggest tables instead so another language still shows up
        let want = !hits.is_empty() || (final_round && no_english && f.fmg.entries.len() > 200);
        if want && dumped < 24 {
            let name = format!("probe-ds3-text-{}-{:08x}.csv", hits.first().map(|h| h.replace(' ', "_")).unwrap_or_else(|| "table".into()), hash as u32);
            if !dir.join(&name).exists() {
                let mut csv = String::from("id,text\n");
                for (id, t) in f.fmg.entries.iter().take(30_000) {
                    let _ = writeln!(csv, "{id},\"{}\"", t.replace('"', "\"\"").replace('\n', "\\n"));
                }
                let _ = std::fs::write(dir.join(&name), csv);
                dumped += 1;
                log.log(&format!("    dumped to {name}"));
            }
        }
    }
    // the item names are not in an .fmg-shaped table (first real runs): look at how they are stored instead
    if found.is_empty() && matches!(round, 3 | 6) {
        super::textdiag::run(log, round);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ashen-probe-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_text_tables_in_this_process_and_dumps_item_tables() {
        let dir = temp_dir("fmg");
        let weapons = fmg::build(&[(1_000_000, &[Some("Dagger"), None, Some("Parrying Dagger")]), (2_000_000, &[Some("Straight Sword"), Some("Broadsword")])]);
        let other = fmg::build(&[(10, &[Some("Hello"), Some("World")])]);
        // keep them at odd offsets inside bigger heap blocks, like a real loaded file
        let mut block_a = vec![0u8; 5000];
        block_a[1001..1001 + weapons.len()].copy_from_slice(&weapons);
        let mut block_b = vec![0u8; 3000];
        block_b[77..77 + other.len()].copy_from_slice(&other);
        let wa = block_a.as_ptr() as usize + 1001;
        let ob = block_b.as_ptr() as usize + 77;

        let log = Logger::open(&dir.join("log.txt"), "test");
        let found = find_fmgs(&log, Duration::from_secs(60));
        assert!(found.iter().any(|f| f.addr == wa && f.fmg.entries.len() == 4), "item table not found");
        assert!(found.iter().any(|f| f.addr == ob && f.fmg.entries.len() == 2), "other table not found");

        scan_text_tables(&log, &dir, 1, false);
        let dumped: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("probe-ds3-text-")).collect();
        // stale copies in freed heap memory may add partial dumps; the full item table must be among them, once
        let full: Vec<String> = dumped.iter().map(|d| std::fs::read_to_string(dir.join(d)).unwrap()).filter(|csv| csv.contains("2000000,\"Straight Sword\"") && csv.contains("1000002,\"Parrying Dagger\"")).collect();
        assert_eq!(full.len(), 1, "the full item table is dumped exactly once: {dumped:?}");
        assert!(dumped.len() <= 4, "{dumped:?}");
        assert!(!dumped.iter().any(|d| d.contains("Other") || d.contains("Hello")), "tables without item names are not dumped");
        drop((block_a, block_b));
    }

    #[test]
    fn a_step_retries_gives_up_and_never_lets_a_panic_out() {
        let dir = temp_dir("step");
        let log = Logger::open(&dir.join("log.txt"), "t");
        let mut s = Step::default();
        s.run(&log, "x", || Ok(false));
        assert!(s.active() && s.fails == 0, "not ready is not a failure");
        s.run(&log, "x", || -> Result<bool, String> { panic!("boom") });
        assert!(s.active() && s.fails == 1, "a panic is caught and counted");
        for _ in 0..4 {
            s.run(&log, "x", || Err("soft failure".to_string()));
        }
        assert!(s.gave_up && !s.active(), "five failures give the step up");
        let mut done = Step::default();
        done.run(&log, "y", || Ok(true));
        assert!(done.done && !done.active());
        let text = std::fs::read_to_string(dir.join("log.txt")).unwrap();
        assert!(text.contains("step crashed (boom)") && text.contains("given up"), "{text}");
    }

    #[test]
    fn pointers_are_checked_before_they_are_followed() {
        assert!(!readable(0, 16));
        assert!(!readable(0x1000, 16));
        let v = vec![1u8; 64];
        assert!(readable(v.as_ptr() as usize, 64));
    }

    #[test]
    fn reading_unreadable_memory_fails_cleanly() {
        assert!(memscan::read(8, 16).is_none());
        let mut buf = [0u8; 16];
        assert_eq!(memscan::read_into(0x10, &mut buf), 0);
        assert!(!memscan::regions(usize::MAX).is_empty());
    }
}
