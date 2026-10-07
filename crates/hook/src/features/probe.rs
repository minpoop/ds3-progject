//! The Dark Souls III probe: a strictly READ-ONLY look at the running game, for private test kits. It answers the
//! questions the weapons feature depends on, with real data from the player's own game:
//!   * which game build is this, and is it one the `darksouls3` crate supports?
//!   * what does the weapon parameter table look like (ids, categories, models, icons, damage)?
//!   * what is in the inventory, and how do stamina / health / item counts change while the player plays?
//!   * where does the game keep its text tables (item names), and can they be read and parsed?
//!
//! Output goes next to the hook log: probe-ds3.txt, probe-ds3-weapons.csv, probe-ds3-inventory.csv,
//! probe-ds3-samples.csv and probe-ds3-text-*.csv. Nothing here writes to game memory or calls a game function.
use super::{memscan, version};
use ashen_common::{config::HookConfig, fmg, logging::Logger, VERSION};
use darksouls3::param::EQUIP_PARAM_WEAPON_ST;
use darksouls3::sprj::{CSRegulationManager, GameDataMan, PlayerIns, SprjTaskGroupIndex, SprjTaskImp};
use darksouls3::util::system::wait_for_system_init;
use fromsoftware_shared::{FromStatic, Program, SharedTaskImpExt};
use pelite::pe64::Pe;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

static FRAMES: AtomicU64 = AtomicU64::new(0);

/// The anchors that identify the item-name tables (exact whole strings, English game).
const ANCHORS: [&str; 10] = ["Straight Sword", "Dagger", "Broadsword", "Longsword", "Short Bow", "Light Crossbow", "Avelyn", "Bolt", "Estus Flask", "Uchigatana"];

fn dir_of(cfg: &HookConfig) -> PathBuf {
    Path::new(&cfg.log_file).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
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
        log.log("in-game probe skipped (unsupported build); only the text-table scan runs");
    }

    let start = Instant::now();
    let mut scans_done = 0usize;
    let scan_at = [Duration::from_secs(25), Duration::from_secs(100), Duration::from_secs(260)];
    let mut last_frames = 0u64;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let t = start.elapsed();
        if scans_done < scan_at.len() && t >= scan_at[scans_done] {
            scans_done += 1;
            let final_scan = scans_done == scan_at.len();
            match catch_unwind(AssertUnwindSafe(|| scan_text_tables(&log, &dir, scans_done, final_scan))) {
                Ok(()) => {}
                Err(p) => log.log(&format!("text-table scan {scans_done} crashed: {}", panic_text(&*p))),
            }
        }
        if t.as_secs() % 30 == 5 {
            let f = FRAMES.load(Ordering::Relaxed);
            if f != last_frames {
                log.log(&format!("in-game task alive: {f} frames so far"));
                last_frames = f;
            } else if supported && t.as_secs() > 60 && f == 0 {
                log.log("in-game task has NOT run once (registered but never called)");
            }
        }
        if t > Duration::from_secs(3600) && scans_done >= scan_at.len() {
            // nothing left to do: stay alive only because the task handle lives here
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
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

// ------------------------------------------------------------------------------------------ in-game task

fn register(cfg: &HookConfig, log: &Arc<Logger>) -> Result<fromsoftware_shared::RecurringTaskHandle<usize>, String> {
    // The crate's own wait spins a CPU core; poll the same flag ourselves with sleeps, then let the crate confirm it.
    let flag_va = Program::current().rva_to_va(darksouls3::rva::get().global_hinstance).map_err(|e| format!("bad address table: {e}"))? as usize;
    let waited = Instant::now();
    loop {
        if memscan::read(flag_va, 8).is_some_and(|b| b.iter().any(|&x| x != 0)) {
            break;
        }
        if waited.elapsed() > Duration::from_secs(180) {
            return Err("the game did not finish starting within 3 minutes".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    wait_for_system_init(&Program::current(), Duration::from_secs(30)).map_err(|e| format!("the game did not finish starting: {e}"))?;
    log.log(&format!("game systems are initialised ({:.1}s after the probe started)", waited.elapsed().as_secs_f32()));
    let task = SprjTaskImp::wait_for_instance(Duration::from_secs(60)).map_err(|e| format!("no task manager: {e}"))?;
    log.log("task manager found; registering the per-frame probe");
    let mut state = ProbeState::new(cfg, log.clone());
    let handle = task.run_recurring(
        move |_: &usize| {
            FRAMES.fetch_add(1, Ordering::Relaxed);
            state.frame();
        },
        SprjTaskGroupIndex::FrameBegin,
    );
    Ok(handle)
}

struct ProbeState {
    dir: PathBuf,
    log: Arc<Logger>,
    t0: Instant,
    last_tick: Instant,
    params_done: bool,
    param_tries: u32,
    world_dumped: bool,
    singletons_logged: bool,
    last_stamina: Option<(i32, i32, i32, u32, u32, u32)>,
    last_inventory: HashMap<u32, u32>,
    last_inventory_check: Instant,
    samples: String,
    samples_rows: usize,
    disabled: bool,
}

impl ProbeState {
    fn new(cfg: &HookConfig, log: Arc<Logger>) -> Self {
        ProbeState {
            dir: dir_of(cfg),
            log,
            t0: Instant::now(),
            last_tick: Instant::now(),
            params_done: false,
            param_tries: 0,
            world_dumped: false,
            singletons_logged: false,
            last_stamina: None,
            last_inventory: HashMap::new(),
            last_inventory_check: Instant::now(),
            samples: String::from("t_ms,chr_hp,chr_fp,chr_stamina,save_hp,save_mp,save_stamina\n"),
            samples_rows: 0,
            disabled: false,
        }
    }

    /// Called by the game once per frame, on the game's own thread.
    fn frame(&mut self) {
        if self.disabled || self.last_tick.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last_tick = Instant::now();
        if self.t0.elapsed() > Duration::from_secs(20 * 60) {
            self.flush_samples();
            self.disabled = true;
            self.log.log("probe finished sampling (20 minutes); it stays quiet from here");
            return;
        }
        let r = catch_unwind(AssertUnwindSafe(|| unsafe { self.tick() }));
        if let Err(p) = r {
            self.log.log(&format!("probe step crashed and is switched off: {}", panic_text(&*p)));
            self.flush_samples();
            self.disabled = true;
        }
    }

    unsafe fn tick(&mut self) {
        if !self.params_done {
            self.param_tries += 1;
            self.params_done = self.dump_weapon_params();
            if !self.params_done && (self.param_tries == 1 || self.param_tries.is_multiple_of(100)) {
                self.log.log(&format!("parameter tables not ready yet (try {})", self.param_tries));
            }
        }
        let Ok(player) = PlayerIns::local_player() else { return };
        if !self.singletons_logged {
            self.singletons_logged = true;
            self.log.log(&format!(
                "in a world: GameDataMan {}, MapItemMan {}",
                if GameDataMan::instance().is_ok() { "present" } else { "absent" },
                if darksouls3::sprj::MapItemMan::instance().is_ok() { "present" } else { "absent" }
            ));
        }
        let pgd = player.player_game_data.as_ref();
        if pgd.equipment.is_main_menu() {
            return; // the synthetic character the game keeps on the title screen
        }
        if !self.world_dumped {
            self.world_dumped = true;
            self.dump_player(player);
        }
        // live values: the character's own data module, and the save-side copy, side by side
        let m = &player.super_chr_ins.modules.data;
        let cur = (m.hp, m.fp, m.stamina, pgd.player_info.hp, pgd.player_info.mp, pgd.player_info.stamina);
        if self.last_stamina != Some(cur) {
            self.last_stamina = Some(cur);
            let _ = writeln!(self.samples, "{},{},{},{},{},{},{}", self.t0.elapsed().as_millis(), cur.0, cur.1, cur.2, cur.3, cur.4, cur.5);
            self.samples_rows += 1;
            if self.samples_rows.is_multiple_of(200) {
                self.flush_samples();
            }
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
                        self.log.log(&format!("inventory change: item 0x{k:08X} {a} -> {b}"));
                    }
                }
            }
            self.last_inventory = now;
        }
    }

    fn flush_samples(&mut self) {
        let _ = std::fs::write(self.dir.join("probe-ds3-samples.csv"), &self.samples);
    }

    unsafe fn dump_weapon_params(&mut self) -> bool {
        let reg = match CSRegulationManager::instance() {
            Ok(r) => r,
            Err(_) => return false,
        };
        let weapons = reg.get_param::<EQUIP_PARAM_WEAPON_ST>();
        let mut csv = String::from("id,weapon_category,wepmotion_category,equip_model_id,icon_id,atk_base_physics,weight,durability,behavior_variation_id,arrow_bolt_equip_id,max_arrow_quantity,wep_se_id_offset\n");
        let mut n = 0usize;
        for (id, row) in weapons.iter() {
            n += 1;
            let _ = writeln!(
                csv,
                "{id},{},{},{},{},{},{},{},{},{},{},{}",
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
        let _ = std::fs::write(self.dir.join("probe-ds3-weapons.csv"), csv);
        self.log.log(&format!("weapon parameter table: {n} rows written to probe-ds3-weapons.csv"));
        true
    }

    unsafe fn dump_player(&mut self, player: &mut PlayerIns) {
        let pgd = player.player_game_data.as_ref();
        let info = &pgd.player_info;
        let len = info.character_name.iter().position(|c| *c == 0).unwrap_or(info.character_name.len());
        // the character's name is not logged (nothing here needs it, and logs get sent around)
        self.log.log(&format!(
            "player: name has {len} characters, vigor {} attunement {} endurance {} strength {} dexterity {} intelligence {} faith {} luck {} vitality {}",
            info.vigor,
            info.attunement,
            info.endurance,
            info.strength,
            info.dexterity,
            info.intelligence,
            info.faith,
            info.luck,
            info.vitality
        ));
        self.log.log(&format!("equipment slot -> inventory index: {:?}", pgd.equipment.equipment_indexes));
        let items = &pgd.equipment.equip_inventory_data.items_data;
        self.log.log(&format!("inventory: {} normal + {} key items, capacity {}", items.normal_items_count, items.key_items_count, items.total_capacity));
        let mut csv = String::from("item_id_hex,category,param_id,quantity\n");
        for e in items.items() {
            let _ = writeln!(csv, "0x{:08X},{:?},{},{}", e.item_id.into_inner(), e.item_id.category(), e.item_id.param_id(), e.quantity);
        }
        let _ = std::fs::write(self.dir.join("probe-ds3-inventory.csv"), csv);
        self.log.log("inventory written to probe-ds3-inventory.csv");
    }
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

fn find_fmgs(log: &Logger, budget: Duration) -> Vec<FoundFmg> {
    const CHUNK: usize = 8 << 20;
    const OVERLAP: usize = 64;
    let started = Instant::now();
    let finder = memchr::memmem::Finder::new(&[0u8, 0, 2, 0]);
    let mut found: Vec<FoundFmg> = Vec::new();
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
                        if let Some(f) = fmg::parse(&bytes) {
                            found.push(FoundFmg { addr, fmg: f });
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
    found
}

fn scan_text_tables(log: &Logger, dir: &Path, round: usize, final_round: bool) {
    log.log(&format!("--- text-table scan {round} ---"));
    let found = find_fmgs(log, Duration::from_secs(45));
    log.log(&format!("found {} text tables that parse as FMG", found.len()));
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
    if found.is_empty() {
        diagnose_anchor(log);
    }
}

/// No table parsed although one may exist: the layout this code assumes could be off. Show the raw neighbourhood
/// of an item name, plus the nearest places before it that look like a table start, so the layout can be worked out
/// offline from the log.
fn diagnose_anchor(log: &Logger) {
    let needle: Vec<u8> = "Straight Sword".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let finder = memchr::memmem::Finder::new(&needle);
    let starts = memchr::memmem::Finder::new(&[0u8, 0, 2, 0]);
    let mut shown = 0;
    let mut buf = vec![0u8; 8 << 20];
    for r in memscan::regions(1 << 30) {
        let mut off = 0usize;
        while off < r.size && shown < 3 {
            let want = (8usize << 20).min(r.size - off);
            let got = memscan::read_into(r.base + off, &mut buf[..want]);
            for p in finder.find_iter(&buf[..got]) {
                if shown >= 3 {
                    break;
                }
                shown += 1;
                let addr = r.base + off + p;
                let lo = p.saturating_sub(48);
                let hi = (p + needle.len() + 48).min(got);
                log.log(&format!("  no text table parsed; \"Straight Sword\" at 0x{addr:X} (region type {}, protect 0x{:X}): {:02x?}", r.kind, r.protect, &buf[lo..hi]));
                // the nearest candidate table starts before it (a table is a few KB to a few hundred KB long)
                let back = (1usize << 20).min(addr.saturating_sub(r.base));
                if let Some(chunk) = memscan::read(addr - back, back) {
                    let mut cands: Vec<usize> = starts.find_iter(&chunk).collect();
                    cands.reverse();
                    for c in cands.into_iter().take(4) {
                        let end = (c + 0x30).min(chunk.len());
                        log.log(&format!("    candidate start {} bytes before it: {:02x?}", back - c, &chunk[c..end]));
                    }
                }
            }
            if off + want >= r.size {
                break;
            }
            off += want - 64;
        }
    }
    if shown == 0 {
        log.log("  \"Straight Sword\" was not found in memory either (another language, or not loaded yet)");
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
    fn explains_what_it_sees_when_no_table_parses() {
        let dir = temp_dir("diag");
        let mut block: Vec<u8> = vec![0, 0, 2, 0, 9, 9, 9, 9]; // looks like a table start but is not one
        block.extend(vec![0u8; 100]);
        block.extend("Straight Sword".encode_utf16().flat_map(|u| u.to_le_bytes()));
        block.extend([0u8, 0]);
        let log = Logger::open(&dir.join("log.txt"), "t");
        diagnose_anchor(&log);
        let text = std::fs::read_to_string(dir.join("log.txt")).unwrap();
        assert!(text.contains("no text table parsed") && text.contains("Straight Sword"), "{text}");
        drop(block);
    }

    #[test]
    fn reading_unreadable_memory_fails_cleanly() {
        assert!(memscan::read(8, 16).is_none());
        let mut buf = [0u8; 16];
        assert_eq!(memscan::read_into(0x10, &mut buf), 0);
        assert!(!memscan::regions(usize::MAX).is_empty());
    }
}
