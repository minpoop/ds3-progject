//! The collector. Dark Souls III keeps the table of contents of each archive (`Data0.bhd` ...) encrypted on disk, so the
//! setup tool cannot read the player's item names, weapon models and so on by itself. The running game must read them
//! though, so for a while after it starts this looks through the game's own memory for what the setup tool needs and saves it
//! next to the program:
//!
//!   * `cache/ds3-keys.pem`   RSA public keys (PEM text, DER, key blobs, or a big-number structure that a real archive proves)
//!   * `cache/bhd5/<name>.bin` the plain table of contents of an archive, when it lies in memory whole and checks out
//!
//! Strictly read-only on the game (it only reads memory the game has already committed, with `ReadProcessMemory`, which
//! cannot fault); the only things written are the files above and `logs/harvest.txt`. It stops by itself as soon as every
//! archive of the install is covered, and does not start at all when the cache already covers them.
//!
//! Everything it keeps is checked against the player's own files first: a key must open the first block of an archive
//! (the plain text starts with `BHD5`) - or be a key by its structure -, a table must parse completely, fit the size of
//! its `.bhd` and keep every file inside its `.bdt`. Nothing private is looked for or kept.
use super::memscan::{self, Region};
use ashen_common::{config::HookConfig, logging::Logger};
use ashen_ds3data::hash::path_hash;
use ashen_ds3data::install::PlainHeader;
use ashen_ds3data::keys::{load_pem_file, RsaPublicKey};
use ashen_ds3data::rsa::first_block_starts_with;
use ashen_ds3data::scan::{check_image, compact_strided, modulus_key, pairs_with, HeaderHit, Scanner, BLOCK};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};

/// Size of the pieces of memory that are searched, and how much of each piece is searched again with the next one (a key
/// is under 16 KiB; the small structures are under a page).
const CHUNK: usize = 8 << 20;
const OVERLAP: usize = 16 << 10;
/// The most big-number candidates that are tried per pass (each one costs a few archive-key tests).
const MAX_BN_TESTS: usize = 600;
/// How many times a header-like place is tried before it is given up (the game may still be filling it).
const MAX_TRIES: u32 = 3;
/// The most lines of detail about places that looked like a table of contents but were not one.
const MAX_NEAR_MISS_LINES: usize = 12;
/// The most places where a known path hash was seen next to a size and an offset that are described.
const MAX_ENTRY_LINES: usize = 8;

/// One archive of the install.
struct Slot {
    name: String,
    bhd_len: u64,
    bdt_len: u64,
    /// The first 256 bytes of the `.bhd`: the first encrypted block.
    block: Vec<u8>,
    /// Fingerprint of a key that opens it.
    key: Option<String>,
    /// The saved plain table: (files, declared size).
    header: Option<(usize, usize)>,
    /// How many times its encrypted start was seen in memory (a hint at how the game handles the file).
    cipher_seen: usize,
}

impl Slot {
    fn covered(&self) -> bool {
        self.key.is_some() || self.header.is_some()
    }
}

/// What one pass over the memory found.
#[derive(Default)]
struct PassStats {
    regions: usize,
    bytes: u64,
    cut: bool,
    forms: BTreeMap<&'static str, usize>,
    new_keys: usize,
    pem_blocks: usize,
    pem_rejected: usize,
    bn_hits: usize,
    bn_tested: usize,
    bn_proven: usize,
    exp_hits: usize,
    header_hits: usize,
    images_saved: usize,
    near_misses: usize,
    cipher_hits: usize,
}

struct Harvester<'a> {
    log: &'a Logger,
    cache: PathBuf,
    scanner: Scanner,
    slots: Vec<Slot>,
    /// Every distinct key seen, by any shape.
    keys: Vec<RsaPublicKey>,
    keys_dirty: bool,
    /// Fingerprints of the moduli that were already tried (found through big-number structures).
    tried: HashSet<String>,
    /// How often each header-like place (absolute address) was tried; `u32::MAX` once it has been used.
    places: HashMap<usize, u32>,
    near_miss_lines: usize,
    entry_lines: usize,
    probes: Vec<(u32, &'static str)>,
    max_bdt: u64,
}

fn region_text(r: &Region) -> String {
    let kind = match r.kind {
        0x20000 => "private",
        0x40000 => "mapped",
        0x1000000 => "module",
        _ => "other",
    };
    let rights = match r.protect & 0xFF {
        0x02 => "r",
        0x04 | 0x08 => "rw",
        0x10 => "x",
        0x20 => "rx",
        0x40 | 0x80 => "rwx",
        _ => "?",
    };
    format!("{kind} {rights}")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// The files of `dir` that are `.bhd` archives with a `.bdt`, as slots (name order).
fn open_slots(dir: &Path) -> Vec<Slot> {
    let Ok(listing) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(String, PathBuf)> = listing.filter_map(|e| e.ok()).map(|e| (e.file_name().to_string_lossy().to_string(), e.path())).collect();
    files.sort_by_key(|(n, _)| n.to_lowercase());
    let mut slots = Vec::new();
    for (name, path) in &files {
        let Some(stem) = name.rsplit_once('.').filter(|(_, ext)| ext.eq_ignore_ascii_case("bhd")).map(|(s, _)| s.to_string()) else { continue };
        let Some(bdt) = files.iter().find(|(n, _)| n.rsplit_once('.').is_some_and(|(s, e)| e.eq_ignore_ascii_case("bdt") && s.eq_ignore_ascii_case(&stem))) else { continue };
        let (Ok(bhd_meta), Ok(bdt_meta)) = (std::fs::metadata(path), std::fs::metadata(&bdt.1)) else { continue };
        let Ok(mut f) = std::fs::File::open(path) else { continue };
        let mut block = vec![0u8; BLOCK];
        if std::io::Read::read_exact(&mut f, &mut block).is_err() {
            continue;
        }
        slots.push(Slot { name: stem, bhd_len: bhd_meta.len(), bdt_len: bdt_meta.len(), block, key: None, header: None, cipher_seen: 0 });
    }
    slots
}

impl<'a> Harvester<'a> {
    fn new(log: &'a Logger, game_dir: &Path, cache: &Path) -> Harvester<'a> {
        let slots = open_slots(game_dir);
        let max_bdt = slots.iter().map(|s| s.bdt_len).max().unwrap_or(0);
        Harvester {
            log,
            cache: cache.to_path_buf(),
            scanner: Scanner::new(),
            slots,
            keys: Vec::new(),
            keys_dirty: false,
            tried: HashSet::new(),
            places: HashMap::new(),
            near_miss_lines: 0,
            entry_lines: 0,
            probes: vec![(path_hash("/msg/ENGLISH/item.msgbnd.dcx"), "msg/ENGLISH/item.msgbnd.dcx"), (path_hash("/msg/ENGLISH/menu.msgbnd.dcx"), "msg/ENGLISH/menu.msgbnd.dcx")],
            max_bdt,
        }
    }

    fn keys_file(&self) -> PathBuf {
        self.cache.join("ds3-keys.pem")
    }

    fn bhd5_dir(&self) -> PathBuf {
        self.cache.join("bhd5")
    }

    /// What an earlier run saved still counts, if it still fits this install.
    fn adopt_saved(&mut self) {
        if let Ok(old) = load_pem_file(&self.keys_file()) {
            for k in old {
                self.note_key(&k, "cache", false);
            }
        }
        let saved = PlainHeader::load_dir(&self.bhd5_dir());
        for slot in &mut self.slots {
            let named = |h: &&PlainHeader| h.label.rsplit_once('.').is_some_and(|(n, _)| n.eq_ignore_ascii_case(&slot.name));
            let found = saved.iter().filter(named).find(|h| h.fits(slot.bhd_len, slot.bdt_len));
            if let Some(h) = found {
                if let Ok(info) = check_image(&h.bytes, Some(slot.bdt_len)) {
                    slot.header = Some((info.entries, info.declared));
                }
            }
        }
    }

    fn all_covered(&self) -> bool {
        !self.slots.is_empty() && self.slots.iter().all(Slot::covered)
    }

    /// A key was seen: remember it, and see which archives it opens. Returns true if it was new.
    fn note_key(&mut self, key: &RsaPublicKey, form: &str, say: bool) -> bool {
        let new = !self.keys.contains(key);
        if new {
            self.keys.push(key.clone());
            self.keys_dirty = true;
        }
        let mut opens = Vec::new();
        for slot in &mut self.slots {
            if slot.key.is_none() && first_block_starts_with(key, &slot.block, b"BHD5") {
                slot.key = Some(key.fingerprint());
                opens.push(slot.name.clone());
            }
        }
        if (new && say) || !opens.is_empty() {
            self.log.log(&format!("key {} ({} bits, found as {form}){}", key.fingerprint(), key.bits(), if opens.is_empty() { String::new() } else { format!(": it OPENS {}", opens.join(", ")) }));
        }
        new
    }

    /// Saves the keys (merged with the file's) when there is something new.
    fn save_keys(&mut self) {
        if !self.keys_dirty {
            return;
        }
        self.keys_dirty = false;
        let mut text = String::new();
        for k in &self.keys {
            text.push_str(&k.to_pem());
        }
        let _ = std::fs::create_dir_all(&self.cache);
        match write_atomic(&self.keys_file(), text.as_bytes()) {
            Ok(()) => self.log.log(&format!("{} key(s) saved to cache\\ds3-keys.pem", self.keys.len())),
            Err(e) => self.log.log(&format!("could not save cache\\ds3-keys.pem: {e}")),
        }
    }

    fn scan_chunk(&mut self, region: &Region, base: usize, data: &[u8], st: &mut PassStats) {
        // keys in the shapes programs keep them
        let found = self.scanner.keys(data);
        st.pem_blocks += found.pem_blocks;
        st.pem_rejected += found.pem_rejected;
        for f in found.keys {
            *st.forms.entry(f.form.name()).or_default() += 1;
            if self.note_key(&f.key, f.form.name(), true) {
                st.new_keys += 1;
            }
        }
        // big-number structures that point at a 2048-bit number
        if self.slots.iter().any(|s| s.key.is_none()) {
            for hit in self.scanner.bignums(data, base) {
                st.bn_hits += 1;
                if st.bn_tested >= MAX_BN_TESTS {
                    continue;
                }
                let mut limbs = [0u8; BLOCK];
                if memscan::read_into(hit.ptr, &mut limbs) != BLOCK {
                    continue;
                }
                let Some(key) = modulus_key(&limbs) else { continue };
                if !self.tried.insert(key.fingerprint()) {
                    continue;
                }
                st.bn_tested += 1;
                // a number is only kept when a real archive proves it is a key
                let before = self.slots.iter().filter(|s| s.key.is_some()).count();
                if self.slots.iter().any(|s| s.key.is_none() && first_block_starts_with(&key, &s.block, b"BHD5")) {
                    self.note_key(&key, &format!("a {} in {} memory", hit.kind.name(), region_text(region)), true);
                    st.bn_proven += self.slots.iter().filter(|s| s.key.is_some()).count() - before;
                }
            }
        }
        // a modulus kept next to its exponent, as a plain structure
        if self.slots.iter().any(|s| s.key.is_none()) {
            for key in self.scanner.moduli_near_exponent(data) {
                st.exp_hits += 1;
                if st.bn_tested >= MAX_BN_TESTS || !self.tried.insert(key.fingerprint()) {
                    continue;
                }
                st.bn_tested += 1;
                if self.slots.iter().any(|s| s.key.is_none() && first_block_starts_with(&key, &s.block, b"BHD5")) {
                    self.note_key(&key, &format!("a number next to the exponent in {} memory", region_text(region)), true);
                    st.bn_proven += 1;
                }
            }
        }
        // a plain table of contents
        if self.slots.iter().any(|s| s.header.is_none()) {
            for h in self.scanner.headers(data) {
                st.header_hits += 1;
                self.try_header(region, base + h.offset, &h, st);
            }
        }
        // hints at how the game holds the files, for the report
        for slot in &mut self.slots {
            if slot.cipher_seen == 0 {
                if let Some(head) = slot.block.get(..64) {
                    let n = memchr::memmem::find_iter(data, head).count();
                    if n > 0 {
                        slot.cipher_seen += n;
                        st.cipher_hits += n;
                    }
                }
            }
        }
        if self.entry_lines < MAX_ENTRY_LINES && self.slots.iter().any(|s| s.header.is_none()) {
            self.entry_intel(region, base, data);
        }
    }

    /// The header-like bytes at `addr`: read the whole table, and keep it if it is one.
    fn try_header(&mut self, region: &Region, addr: usize, h: &HeaderHit, st: &mut PassStats) {
        let tries = self.places.entry(addr).or_insert(0);
        if *tries >= MAX_TRIES {
            return;
        }
        *tries += 1;
        let direct = memscan::read(addr, h.declared);
        let mut why = String::new();
        let mut layout = "contiguous";
        let mut image = None;
        if let Some(bytes) = direct {
            match self.accept(&bytes) {
                Ok(()) => image = Some(bytes),
                Err(e) => why = e,
            }
        } else {
            why = "the memory after the start cannot be read".to_string();
        }
        if image.is_none() && h.zero_before {
            // maybe every 255-byte block sits in its own 256-byte slot
            let raw_len = h.declared.div_ceil(BLOCK - 1) * BLOCK;
            if let Some(raw) = memscan::read(addr - 1, raw_len) {
                let compact = compact_strided(&raw);
                if self.accept(&compact).is_ok() {
                    image = Some(compact);
                    layout = "in 256-byte slots";
                }
            }
        }
        match image {
            Some(bytes) => {
                self.places.insert(addr, u32::MAX);
                if self.save_image(&bytes, addr, region, layout) {
                    st.images_saved += 1;
                }
            }
            None => {
                st.near_misses += 1;
                if self.near_miss_lines < MAX_NEAR_MISS_LINES {
                    self.near_miss_lines += 1;
                    self.log.log(&format!(
                        "header-like place at 0x{addr:X} ({}), declares {} bytes, {} buckets, byte before is {}: not a usable table ({why}); first bytes: {}",
                        region_text(region),
                        h.declared,
                        h.buckets,
                        if h.zero_before { "zero" } else { "not zero" },
                        memscan::read(addr, 32).map(|b| hex(&b)).unwrap_or_default()
                    ));
                }
            }
        }
    }

    /// Does this plain table of contents belong to an archive of the install? (It must parse completely, fit one `.bhd`'s size
    /// and keep every file inside that archive's `.bdt`.)
    fn accept(&self, image: &[u8]) -> Result<(), String> {
        let info = check_image(image, None)?;
        if self.slots.iter().any(|s| pairs_with(info.declared, s.bhd_len) && info.max_end <= s.bdt_len) {
            Ok(())
        } else {
            Err(format!("parses ({} files, declares {} bytes, reaches {} into the archive) but fits none of the {} archives", info.entries, info.declared, info.max_end, self.slots.len()))
        }
    }

    fn save_image(&mut self, image: &[u8], addr: usize, region: &Region, layout: &str) -> bool {
        let Ok(info) = check_image(image, None) else { return false };
        // the archive: the one that fits (a name is not needed, the size and the files decide)
        let Some(i) = self.slots.iter().position(|s| s.header.is_none() && pairs_with(info.declared, s.bhd_len) && info.max_end <= s.bdt_len) else {
            return false;
        };
        let Some(trimmed) = image.get(..info.declared) else { return false };
        let slot = &mut self.slots[i];
        let dir = self.cache.join("bhd5");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}.bin", slot.name));
        match write_atomic(&path, trimmed) {
            Ok(()) => {
                slot.header = Some((info.entries, info.declared));
                self.log.log(&format!(
                    "TABLE OF CONTENTS of {} found at 0x{addr:X} ({}, {layout}): {} files in {} buckets, {} bytes, reaches {} of the {} byte .bdt; saved to cache\\bhd5\\{}.bin",
                    slot.name,
                    region_text(region),
                    info.entries,
                    info.buckets,
                    info.declared,
                    info.max_end,
                    slot.bdt_len,
                    slot.name
                ));
                true
            }
            Err(e) => {
                self.log.log(&format!("could not save cache\\bhd5\\{}.bin: {e}", slot.name));
                false
            }
        }
    }

    /// Where the hash of a known path is followed by a size and an offset that fit an archive: describes the neighbourhood
    /// (numbers only), so that a table the game keeps in its own layout can be recognised later.
    fn entry_intel(&mut self, region: &Region, base: usize, data: &[u8]) {
        let probes = self.probes.clone();
        for (hash, name) in probes {
            for hit in self.scanner.entry_hits(data, hash, 1 << 30, self.max_bdt, 3) {
                if self.entry_lines >= MAX_ENTRY_LINES {
                    return;
                }
                self.entry_lines += 1;
                let rec = |at: usize| -> String {
                    let get = |o: usize, n: usize| data.get(at + o..at + o + n);
                    match (get(0, 4), get(4, 4), get(8, 8)) {
                        (Some(h), Some(s), Some(o)) => format!(
                            "{:08x}/{}/{}",
                            u32::from_le_bytes(h.try_into().unwrap_or([0; 4])),
                            i32::from_le_bytes(s.try_into().unwrap_or([0; 4])),
                            i64::from_le_bytes(o.try_into().unwrap_or([0; 8]))
                        ),
                        _ => "-".to_string(),
                    }
                };
                let around: Vec<String> = [-80i64, -40, 0, 40, 80].iter().map(|d| usize::try_from(hit.offset as i64 + d).map(&rec).unwrap_or_else(|_| "-".into())).collect();
                self.log.log(&format!(
                    "path hash of {name} followed by size {} and offset {} at 0x{:X} ({}); records 2 before .. 2 after (hash/size/offset): {}; the 24 bytes before it: {}",
                    hit.padded_size,
                    hit.file_offset,
                    base + hit.offset,
                    region_text(region),
                    around.join("  "),
                    hex(data.get(hit.offset.saturating_sub(24)..hit.offset).unwrap_or_default())
                ));
            }
        }
    }

    /// One pass over the whole address space (within `budget`).
    fn pass(&mut self, budget: Duration) -> PassStats {
        let started = Instant::now();
        let mut st = PassStats::default();
        let mut buf = vec![0u8; CHUNK];
        'regions: for r in memscan::regions(1 << 30) {
            st.regions += 1;
            let mut off = 0usize;
            while off < r.size {
                if started.elapsed() > budget {
                    st.cut = true;
                    break 'regions;
                }
                let want = CHUNK.min(r.size - off);
                let got = memscan::read_into(r.base + off, &mut buf[..want]);
                if got > 0 {
                    st.bytes += got as u64;
                    self.scan_chunk(&r, r.base + off, &buf[..got], &mut st);
                }
                if off + want >= r.size {
                    break;
                }
                off += want.saturating_sub(OVERLAP).max(1);
            }
        }
        self.save_keys();
        st
    }

    fn summary(&self) -> String {
        self.slots
            .iter()
            .map(|s| {
                let how = match (&s.key, &s.header) {
                    (Some(k), Some((n, _))) => format!("key {k} + table of {n} files"),
                    (Some(k), None) => format!("key {k}"),
                    (None, Some((n, _))) => format!("table of {n} files"),
                    (None, None) => format!("NOT FOUND{}", if s.cipher_seen > 0 { format!(" (its encrypted start is in memory {}x)", s.cipher_seen) } else { String::new() }),
                };
                format!("{}: {how}", s.name)
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Writes through a temporary file and a rename, so a reader never sees half a file.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Which of the libraries that matter for archive keys are loaded in the game (names only).
fn loaded_crypto_modules() -> Vec<&'static str> {
    const NAMES: [&str; 11] = [
        "bcrypt.dll",
        "ncrypt.dll",
        "crypt32.dll",
        "libeay32.dll",
        "ssleay32.dll",
        "libcrypto-1_1-x64.dll",
        "libcrypto-3-x64.dll",
        "libssl-1_1-x64.dll",
        "libssl-3-x64.dll",
        "steam_api64.dll",
        "dxgi.dll",
    ];
    NAMES
        .iter()
        .copied()
        .filter(|n| {
            let wide: Vec<u16> = n.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe { !GetModuleHandleW(wide.as_ptr()).is_null() }
        })
        .collect()
}

/// The pause between passes: quick while the game loads, slower later.
fn pause_after(elapsed: Duration) -> Duration {
    match elapsed.as_secs() {
        0..=29 => Duration::from_millis(250),
        30..=179 => Duration::from_secs(5),
        _ => Duration::from_secs(30),
    }
}

/// How long the collector keeps trying.
const GIVE_UP_AFTER: Duration = Duration::from_secs(900);

/// The folder of `{data}`: the parent of the logs folder.
fn data_dir_of(cfg: &HookConfig) -> PathBuf {
    let logs = Path::new(&cfg.log_file).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    logs.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

/// Runs on its own thread for as long as there is something to collect.
pub fn thread_body(cfg: HookConfig) {
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
    let logs = Path::new(&cfg.log_file).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let log = Logger::open(&logs.join("harvest.txt"), "harvest");
    let started = Instant::now();
    log.log(&format!("collector v{} starting (read-only: it only reads the game's memory)", ashen_common::VERSION));
    let Some(game_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) else {
        log.log("the game's folder is unknown; nothing to do");
        return;
    };
    let cache = data_dir_of(&cfg).join("cache");
    run(&log, &game_dir, &cache, started);
}

/// The collecting loop (separate from [`thread_body`] so that tests can run it on a fake install).
fn run(log: &Logger, game_dir: &Path, cache: &Path, started: Instant) {
    let mut h = Harvester::new(log, game_dir, cache);
    if h.slots.is_empty() {
        log.log("no .bhd/.bdt archive pair in the game's folder; nothing to do");
        return;
    }
    log.log(&format!("{} archives: {}", h.slots.len(), h.slots.iter().map(|s| format!("{} ({} bytes)", s.name, s.bhd_len)).collect::<Vec<_>>().join(", ")));
    log.log(&format!("libraries loaded in the game: {}", loaded_crypto_modules().join(", ")));
    h.adopt_saved();
    if h.all_covered() {
        log.log(&format!("the cache already covers every archive ({}); nothing to collect", h.summary()));
        return;
    }
    log.log(&format!("already covered by the cache: {}", h.summary()));
    let mut pass_no = 0usize;
    let mut last_summary = String::new();
    loop {
        let t = started.elapsed();
        if t > GIVE_UP_AFTER {
            log.log(&format!("giving up after {} s: {}", t.as_secs(), h.summary()));
            return;
        }
        pass_no += 1;
        let pass_started = Instant::now();
        let before = h.summary();
        let result = catch_unwind(AssertUnwindSafe(|| h.pass(Duration::from_secs(60))));
        match result {
            Ok(st) => {
                let after = h.summary();
                let changed = after != before || st.new_keys > 0;
                if pass_no <= 3 || changed || pass_no.is_multiple_of(20) || st.cut {
                    log.log(&format!(
                        "pass {pass_no} at {:.1} s ({:.1} s, {} regions, {} MB{}): keys by shape {:?}, new {}; key text blocks {} ({} unusable); big-number structures {}, numbers next to the exponent {} (tried {}, proven {}); table-like places {} (saved {}, not usable {}); encrypted file starts seen {}",
                        t.as_secs_f32(),
                        pass_started.elapsed().as_secs_f32(),
                        st.regions,
                        st.bytes >> 20,
                        if st.cut { ", CUT SHORT" } else { "" },
                        st.forms,
                        st.new_keys,
                        st.pem_blocks,
                        st.pem_rejected,
                        st.bn_hits,
                        st.exp_hits,
                        st.bn_tested,
                        st.bn_proven,
                        st.header_hits,
                        st.images_saved,
                        st.near_misses,
                        st.cipher_hits
                    ));
                }
                if after != last_summary && changed {
                    log.log(&format!("covered so far: {after}"));
                    last_summary = after;
                }
            }
            Err(p) => log.log(&format!("pass {pass_no} crashed ({}); trying again", panic_text(&*p))),
        }
        if h.all_covered() {
            log.log(&format!("every archive is covered after {:.1} s: {}", started.elapsed().as_secs_f32(), h.summary()));
            return;
        }
        std::thread::sleep(pause_after(started.elapsed()));
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ashen_ds3data::testing::install::{build, FakeOptions};
    use ashen_ds3data::testing::keys::test_key;
    use std::sync::Mutex;

    /// The tests plant things in this process's own memory and scan it; one at a time, so that what one plants is not
    /// found by another while it is still alive.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ashen-harvest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The plain table of contents of the fake archive `name` (what the running game would hold).
    fn plain_table(game: &Path, name: &str, key: usize) -> Vec<u8> {
        let bhd = std::fs::read(game.join(format!("{name}.bhd"))).unwrap();
        let mut plain = ashen_ds3data::rsa::decrypt_header(&test_key(key).public, &bhd).unwrap();
        let declared = i32::from_le_bytes(plain[0x0C..0x10].try_into().unwrap()) as usize;
        plain.truncate(declared);
        plain
    }

    #[test]
    fn a_plain_table_in_memory_is_found_checked_saved_and_paired_with_its_archive() {
        let _one = one_at_a_time();
        let dir = temp_dir("table");
        let fake = build(&dir.join("DS3"), &FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
        let table = plain_table(&fake.game, "Data1", 1);
        // the table sits at an odd place inside a bigger block, like in a real heap
        let mut heap = vec![0u8; 20_000];
        heap[777..777 + table.len()].copy_from_slice(&table);
        // and a decoy: it looks like a table but its files lie far outside every archive
        let mut decoy = table.clone();
        let at = 0x1C + 11 + 8 * 7 + 8; // the offset field of the first file header (after the salt and the 7-bucket table)
        decoy[at..at + 8].copy_from_slice(&(1i64 << 40).to_le_bytes());
        let mut decoy_heap = vec![0u8; 4000];
        decoy_heap[100..100 + decoy.len()].copy_from_slice(&decoy);

        let log = Logger::open(&dir.join("harvest.txt"), "t");
        let cache = dir.join("cache");
        let mut h = Harvester::new(&log, &fake.game, &cache);
        let st = h.pass(Duration::from_secs(120));
        assert!(st.images_saved >= 1, "{}", std::fs::read_to_string(dir.join("harvest.txt")).unwrap());
        let saved = std::fs::read(cache.join("bhd5").join("Data1.bin")).unwrap();
        assert_eq!(saved, table, "the table is saved exactly, cut to the size it declares");
        assert!(h.slots.iter().find(|s| s.name == "Data1").unwrap().header.is_some());
        let text = std::fs::read_to_string(dir.join("harvest.txt")).unwrap();
        assert!(text.contains("TABLE OF CONTENTS of Data1 found at"), "{text}");
        assert!(st.near_misses >= 1 || text.contains("not a usable table"), "the decoy is reported, not kept: {text}");
        drop((heap, decoy_heap));
    }

    #[test]
    fn keys_are_found_in_their_shapes_and_the_archives_they_open_are_named() {
        let _one = one_at_a_time();
        let dir = temp_dir("keys");
        let fake = build(&dir.join("DS3"), &FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
        let k0 = test_key(0).public;
        // key 0 as DER in memory, key 1 only as a big-number structure that points at its modulus
        let der = k0.to_pkcs1_der();
        let modulus_le = test_key(1).public.n.to_bytes_le();
        let mut bn = vec![0u8; 64];
        let ptr = modulus_le.as_ptr() as u64;
        bn[8..16].copy_from_slice(&ptr.to_le_bytes());
        bn[16..20].copy_from_slice(&32u32.to_le_bytes());
        bn[20..24].copy_from_slice(&33u32.to_le_bytes());
        bn[28..32].copy_from_slice(&1u32.to_le_bytes());
        // an unrelated 2048-bit number behind another structure must not be kept
        let mut other = modulus_le.clone();
        other[0] ^= 0x02; // still odd, still 2048 bits, but not a key of this install
        let mut bn_other = vec![0u8; 64];
        bn_other[8..16].copy_from_slice(&(other.as_ptr() as u64).to_le_bytes());
        bn_other[16..20].copy_from_slice(&32u32.to_le_bytes());
        bn_other[20..24].copy_from_slice(&33u32.to_le_bytes());

        let log = Logger::open(&dir.join("harvest.txt"), "t");
        let cache = dir.join("cache");
        let mut h = Harvester::new(&log, &fake.game, &cache);
        let st = h.pass(Duration::from_secs(120));
        let text = std::fs::read_to_string(dir.join("harvest.txt")).unwrap();
        let by_name = |n: &str| h.slots.iter().find(|s| s.name == n).unwrap().key.clone();
        assert_eq!(by_name("Data0").as_deref(), Some("87febfc8"), "{text}");
        assert_eq!(by_name("DLC1").as_deref(), Some("87febfc8"), "{text}");
        assert_eq!(by_name("Data1").as_deref(), Some("b2969406"), "found through the structure: {text}");
        assert!(h.all_covered());
        assert!(st.bn_hits >= 2, "both structures were seen: {text}");
        let pem = std::fs::read_to_string(cache.join("ds3-keys.pem")).unwrap();
        let saved = ashen_ds3data::keys::find_pem_keys(pem.as_bytes());
        assert!(saved.contains(&k0) && saved.contains(&test_key(1).public), "{pem}");
        assert!(!saved.iter().any(|k| k.n.to_bytes_le() == other), "a number no archive proves is not kept");
        assert!(!text.contains("BEGIN"), "no key text in the log");
        drop((der, bn, bn_other, other, modulus_le));
    }

    #[test]
    fn a_modulus_kept_next_to_its_exponent_is_proven_by_the_archive_it_opens() {
        let _one = one_at_a_time();
        let dir = temp_dir("exp");
        let fake = build(&dir.join("DS3"), &FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
        // a plain key structure: 256 bytes of little-endian limbs, then the exponent as a 64-bit word
        let mut structure = vec![0xEEu8; 24];
        structure.extend(test_key(1).public.n.to_bytes_le());
        structure.extend(65537u64.to_le_bytes());
        structure.extend([0x11u8; 24]);
        let log = Logger::open(&dir.join("harvest.txt"), "t");
        let mut h = Harvester::new(&log, &fake.game, &dir.join("cache"));
        let st = h.pass(Duration::from_secs(120));
        let text = std::fs::read_to_string(dir.join("harvest.txt")).unwrap();
        assert_eq!(h.slots.iter().find(|s| s.name == "Data1").unwrap().key.as_deref(), Some("b2969406"), "{text}");
        assert!(st.exp_hits >= 1, "the structure was seen: {text}");
        drop(structure);
    }

    #[test]
    fn the_loop_stops_at_once_when_the_cache_covers_everything_and_when_nothing_is_to_do() {
        let dir = temp_dir("loop");
        let fake = build(&dir.join("DS3"), &FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
        let cache = dir.join("cache");
        std::fs::create_dir_all(cache.join("bhd5")).unwrap();
        for (name, key) in [("Data0", 0usize), ("Data1", 1), ("DLC1", 0)] {
            std::fs::write(cache.join("bhd5").join(format!("{name}.bin")), plain_table(&fake.game, name, key)).unwrap();
        }
        let log = Logger::open(&dir.join("harvest.txt"), "t");
        let t = Instant::now();
        run(&log, &fake.game, &cache, Instant::now());
        assert!(t.elapsed() < Duration::from_secs(10), "no scan was started");
        let text = std::fs::read_to_string(dir.join("harvest.txt")).unwrap();
        assert!(text.contains("the cache already covers every archive"), "{text}");
        // no archives at all
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        run(&log, &empty, &cache, Instant::now());
        assert!(std::fs::read_to_string(dir.join("harvest.txt")).unwrap().contains("nothing to do"));
    }

    #[test]
    fn a_saved_table_for_another_game_version_does_not_count() {
        let dir = temp_dir("stale");
        let fake = build(&dir.join("DS3"), &FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
        let cache = dir.join("cache");
        std::fs::create_dir_all(cache.join("bhd5")).unwrap();
        // Data1's table saved under the name Data0: the name suggests one thing, the size says another
        std::fs::write(cache.join("bhd5").join("Data0.bin"), plain_table(&fake.game, "Data1", 1)).unwrap();
        let log = Logger::open(&dir.join("harvest.txt"), "t");
        let mut h = Harvester::new(&log, &fake.game, &cache);
        h.adopt_saved();
        assert!(h.slots.iter().all(|s| s.header.is_none()), "a table that does not fit the archive of that name is not trusted");
    }

    #[test]
    fn the_description_of_memory_is_short_and_plain() {
        let r = Region { base: 0, size: 0, protect: 0x04, kind: 0x20000 };
        assert_eq!(region_text(&r), "private rw");
        assert_eq!(region_text(&Region { protect: 0x20, kind: 0x1000000, ..r }), "module rx");
        assert_eq!(hex(&[0, 1, 0xAB]), "00 01 ab");
        assert_eq!(pause_after(Duration::from_secs(1)), Duration::from_millis(250));
        assert_eq!(pause_after(Duration::from_secs(60)), Duration::from_secs(5));
        assert_eq!(pause_after(Duration::from_secs(600)), Duration::from_secs(30));
    }
}
