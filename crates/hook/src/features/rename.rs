//! The in-memory rename (private test kits, `--rename`). The item text of Dark Souls III is in archives this kit cannot always
//! open, so the new names (Chainsword, Bolter, Bolt Rounds) are written over the old ones in the running game instead.
//!
//! What the game does with its text (seen in the logs of kits 0.4 and 0.8): it keeps every name as its own zero-ended UTF-16
//! string in a heap block, 8 bytes after the start of the block (the block's header), at an address divisible by 8, with many
//! pointers to it; and it builds the strings again when a world is loaded (they move). So this thread looks through the
//! process's own private, writable memory - with `ReadProcessMemory`, which cannot fault - for the old names as WHOLE strings:
//! the exact text, a zero right after it, and no printable character right before it (that would make it the end of a longer
//! text). Each one is overwritten in place with the new name, which has exactly as many characters (shorter names are padded
//! with spaces, see `ashen_common::weapons::same_length`), so a length kept next to the text stays true and nothing that
//! follows is touched. The write is `WriteProcessMemory`: a failure is an error code, not a crash.
//!
//! It calls no game function and changes nothing else. The scan repeats, because the strings are built again (and moved) when a
//! world loads; every find is logged to `logs/rename.txt` with the bytes around it, so that the layout can be studied.
use super::memscan::{self, Region};
use ashen_common::logging::Logger;
use ashen_common::weapons::{live_names, sheet_weapons, LiveName};
use ashen_common::{config::HookConfig, VERSION};
use std::collections::{BTreeSet, HashSet};
use std::ops::Range;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};

/// Size of the pieces of memory that are searched, and how much of each piece is searched again with the next one (more than
/// the longest name plus its zero plus the character before it).
const CHUNK: usize = 8 << 20;
const OVERLAP: usize = 128;
/// `MEM_PRIVATE`, and the page protections that allow writing (`PAGE_READWRITE`, `PAGE_WRITECOPY` and the executable kinds).
const PRIVATE: u32 = 0x20000;
const WRITABLE: u32 = 0x04 | 0x08 | 0x40 | 0x80;
/// The most finds that are described with the bytes around them (the layout is the same for all of them).
const MAX_DESCRIBED: usize = 8;
/// Every this many passes all of the memory is searched; the passes in between only look at the places where something was
/// found before (the game keeps its strings together).
const FULL_EVERY: u32 = 4;

/// One place where an old name stands as a whole string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub addr: usize,
    /// Which name (index into the list).
    pub name: usize,
    /// Up to 32 bytes before the text and up to 64 after its zero, for the log.
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

fn le_bytes(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|u| u.to_le_bytes()).collect()
}

fn overlaps(skip: &[Range<usize>], addr: usize, len: usize) -> bool {
    skip.iter().any(|r| addr < r.end && addr + len > r.start)
}

/// The places in `data` (which starts at address `base`) where one of the old names is a whole string. `first_of_region` says
/// that nothing is before `data` (so the character before the text is unknown only at its very start). Places inside the
/// ranges in `skip` (this thread's own copies of the names) are left out.
///
/// A place counts when: the text matches exactly; the address is divisible by 8 (as the game's strings are); the two bytes right
/// after the text are zero; the two bytes right before it are not a printable ASCII character (so it is not the tail of a longer
/// text); and all of that can be seen in `data` - a place too near its end is left to the next piece, which overlaps this one.
pub fn candidates(data: &[u8], base: usize, names: &[LiveName], skip: &[Range<usize>], first_of_region: bool) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let needle = le_bytes(&name.old);
        if needle.is_empty() {
            continue;
        }
        for at in memchr::memmem::find_iter(data, &needle) {
            let addr = base + at;
            let end = at + needle.len();
            if !addr.is_multiple_of(8) || end + 2 > data.len() || overlaps(skip, addr, needle.len() + 2) {
                continue;
            }
            if data[end] != 0 || data[end + 1] != 0 {
                continue;
            }
            match at.checked_sub(2) {
                Some(p) => {
                    let before = u16::from_le_bytes([data[p], data[p + 1]]);
                    if (0x20..=0x7E).contains(&before) {
                        continue;
                    }
                }
                None if first_of_region => {}
                None => continue,
            }
            out.push(Candidate {
                addr,
                name: index,
                before: data[at.saturating_sub(32)..at].to_vec(),
                after: data[end + 2..(end + 2 + 64).min(data.len())].to_vec(),
            });
        }
    }
    out
}

/// What one pass found and did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PassOutcome {
    pub full: bool,
    pub regions: usize,
    pub bytes: u64,
    /// Whole-string finds per name (index = the name's index).
    pub found: Vec<usize>,
    pub written: usize,
    pub failed: usize,
    pub cut: bool,
}

pub struct Renamer<'a> {
    log: &'a Logger,
    names: Vec<LiveName>,
    /// The new text of every name as bytes.
    new_bytes: Vec<Vec<u8>>,
    /// Where something was found (region base addresses): the passes between the full ones look only there.
    hot: BTreeSet<usize>,
    /// Every address that was written, with the name written.
    written: Vec<(usize, usize)>,
    described: usize,
    pass_id: u32,
}

impl<'a> Renamer<'a> {
    pub fn new(log: &'a Logger, names: Vec<LiveName>) -> Renamer<'a> {
        let new_bytes = names.iter().map(|n| le_bytes(&n.new)).collect();
        Renamer { log, names, new_bytes, hot: BTreeSet::new(), written: Vec::new(), described: 0, pass_id: 0 }
    }

    /// How many strings were written so far (over the whole run).
    pub fn written_total(&self) -> usize {
        self.written.len()
    }

    fn describe(&mut self, region: &Region, c: &Candidate) {
        if self.described >= MAX_DESCRIBED {
            return;
        }
        self.described += 1;
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ");
        self.log.log(&format!(
            "found \"{}\" as a whole string at 0x{:X} ({}); the 32 bytes before it: {} | the 64 bytes after its zero: {}",
            String::from_utf16_lossy(&self.names[c.name].old),
            c.addr,
            region_text(region),
            hex(&c.before),
            hex(&c.after)
        ));
    }

    /// One pass over the memory (within `budget`). `full`: all of it, else only the places that had a find.
    pub fn pass(&mut self, full: bool, budget: Duration) -> PassOutcome {
        let started = Instant::now();
        self.pass_id += 1;
        let mut out = PassOutcome { full, found: vec![0; self.names.len()], ..PassOutcome::default() };
        let mut buf = vec![0u8; CHUNK];
        // this thread's own copies of the names must not be taken for the game's
        let mut skip: Vec<Range<usize>> = Vec::new();
        skip.push(buf.as_ptr() as usize..buf.as_ptr() as usize + buf.len());
        for n in &self.names {
            skip.push(n.old.as_ptr() as usize..n.old.as_ptr() as usize + n.old.len() * 2);
            skip.push(n.new.as_ptr() as usize..n.new.as_ptr() as usize + n.new.len() * 2);
        }
        for b in &self.new_bytes {
            skip.push(b.as_ptr() as usize..b.as_ptr() as usize + b.len());
        }
        let mut seen: HashSet<usize> = HashSet::new();
        'regions: for r in memscan::regions(1 << 30) {
            if r.kind != PRIVATE || r.protect & WRITABLE == 0 {
                continue;
            }
            if !full && !self.hot.contains(&r.base) {
                continue;
            }
            out.regions += 1;
            let mut off = 0usize;
            while off < r.size {
                if started.elapsed() > budget {
                    out.cut = true;
                    break 'regions;
                }
                let want = CHUNK.min(r.size - off);
                let got = memscan::read_into(r.base + off, &mut buf[..want]);
                if got > 0 {
                    out.bytes += got as u64;
                    for c in candidates(&buf[..got], r.base + off, &self.names, &skip, off == 0) {
                        if !seen.insert(c.addr) {
                            continue;
                        }
                        out.found[c.name] += 1;
                        self.hot.insert(r.base);
                        self.describe(&r, &c);
                        self.write(&c, &mut out);
                    }
                }
                if off + want >= r.size {
                    break;
                }
                off += want.saturating_sub(OVERLAP).max(1);
            }
        }
        out
    }

    fn write(&mut self, c: &Candidate, out: &mut PassOutcome) {
        let new = &self.new_bytes[c.name];
        let name = &self.names[c.name];
        // the text may have changed since it was read: write only if it is still the old name
        let still = memscan::read(c.addr, new.len() + 2).is_some_and(|now| now[..new.len()] == le_bytes(&name.old)[..] && now[new.len()] == 0 && now[new.len() + 1] == 0);
        if !still {
            out.failed += 1;
            self.log.log(&format!("0x{:X}: the text there changed before it could be written; left alone", c.addr));
            return;
        }
        if memscan::write(c.addr, new) && memscan::read(c.addr, new.len()).is_some_and(|now| now == *new) {
            out.written += 1;
            self.written.push((c.addr, c.name));
            self.log.log(&format!("WROTE \"{}\" over \"{}\" at 0x{:X}", String::from_utf16_lossy(&name.new).trim_end(), String::from_utf16_lossy(&name.old), c.addr));
        } else {
            out.failed += 1;
            self.log.log(&format!("could not write at 0x{:X}: the memory refused (or the text reads differently afterwards)", c.addr));
        }
    }
}

fn region_text(r: &Region) -> String {
    format!("{}, {} bytes", if r.kind == PRIVATE { "private" } else { "other" }, r.size)
}

/// The text is not in memory in the first seconds of the game; looking earlier only costs time while it loads.
const START_DELAY: Duration = Duration::from_secs(6);

/// The pause after a pass: quick while nothing is found yet, slower after.
pub fn pause_after(run_time: Duration, written: usize) -> Duration {
    match (written, run_time.as_secs()) {
        (0, 0..=119) => Duration::from_secs(4),
        (0, _) => Duration::from_secs(15),
        (_, 0..=179) => Duration::from_secs(5),
        _ => Duration::from_secs(20),
    }
}

/// Runs on its own thread, for as long as the game runs.
pub fn thread_body(cfg: HookConfig) {
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
    let logs = Path::new(&cfg.log_file).parent().map(Path::to_path_buf).unwrap_or_else(|| std::path::PathBuf::from("."));
    let log = Logger::open(&logs.join("rename.txt"), "rename");
    log.log(&format!("rename v{VERSION} starting: it writes new names over the old ones in the game's memory, in place, and nothing else"));
    let (names, left_out) = live_names(&sheet_weapons());
    for l in left_out {
        log.log(&format!("NOT renamed: {l}"));
    }
    if names.is_empty() {
        log.log("no names to write");
        return;
    }
    for n in &names {
        log.log(&format!("will write: {}: \"{}\" -> \"{}\"", n.what, String::from_utf16_lossy(&n.old), String::from_utf16_lossy(&n.new)));
    }
    run(&log, names, Instant::now());
}

fn run(log: &Logger, names: Vec<LiveName>, started: Instant) {
    let mut r = Renamer::new(log, names);
    let mut pass_no = 0u32;
    std::thread::sleep(START_DELAY);
    loop {
        pass_no += 1;
        let t = started.elapsed();
        let full = pass_no % FULL_EVERY == 1 || r.hot.is_empty();
        let before = r.written_total();
        match catch_unwind(AssertUnwindSafe(|| r.pass(full, Duration::from_secs(30)))) {
            Ok(o) => {
                let found: u64 = o.found.iter().map(|n| *n as u64).sum();
                if r.written_total() != before || o.failed > 0 || pass_no <= 3 || pass_no.is_multiple_of(20) || o.cut {
                    log.log(&format!(
                        "pass {pass_no} at {:.1} s ({} search of {} places, {} MB{}): {found} whole strings found ({}), {} written, {} not; {} written so far",
                        t.as_secs_f32(),
                        if full { "full" } else { "quick" },
                        o.regions,
                        o.bytes >> 20,
                        if o.cut { ", CUT SHORT" } else { "" },
                        r.names.iter().zip(&o.found).map(|(n, c)| format!("{} {c}", String::from_utf16_lossy(&n.old))).collect::<Vec<_>>().join(", "),
                        o.written,
                        o.failed,
                        r.written_total()
                    ));
                }
            }
            Err(p) => log.log(&format!("pass {pass_no} crashed ({}); trying again", panic_text(&*p))),
        }
        std::thread::sleep(pause_after(started.elapsed(), r.written_total()));
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The tests plant strings in this process's own memory and the real pass searches all of it: one test at a time, and every
    /// buffer is zeroed before it is freed (a freed block still holds its text, and the pass would find it).
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
    }

    struct Zeroing(Vec<u8>);

    impl std::ops::Deref for Zeroing {
        type Target = Vec<u8>;
        fn deref(&self) -> &Vec<u8> {
            &self.0
        }
    }

    impl std::ops::DerefMut for Zeroing {
        fn deref_mut(&mut self) -> &mut Vec<u8> {
            &mut self.0
        }
    }

    impl Drop for Zeroing {
        fn drop(&mut self) {
            self.0.fill(0);
        }
    }

    fn zeroed(len: usize) -> Zeroing {
        Zeroing(vec![0u8; len])
    }

    fn names() -> Vec<LiveName> {
        live_names(&sheet_weapons()).0
    }

    fn utf16z(s: &str) -> Vec<u8> {
        s.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect()
    }

    /// A block like the game's: an 8-byte header, the zero-ended text, zeros up to the next block. `at` is the offset of the
    /// block in the buffer (the text starts 8 bytes later).
    fn plant(buf: &mut [u8], at: usize, header: [u8; 8], text: &str) {
        buf[at..at + 8].copy_from_slice(&header);
        let t = utf16z(text);
        buf[at + 8..at + 8 + t.len()].copy_from_slice(&t);
    }

    #[test]
    fn a_whole_string_with_a_header_before_it_is_a_candidate() {
        let _one = one_at_a_time();
        let n = names();
        let mut data = zeroed(512);
        plant(&mut data, 56, [0xfc, 0x20, 0xd4, 0x44, 0x00, 0xa6, 0x02, 0x80], "Shortsword"); // the header seen in kit 0.4 (title screen)
        plant(&mut data, 128, [0x80, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], "Standard Bolt"); // a header that ends in zeros
        plant(&mut data, 256, [1, 2, 3, 4, 5, 6, 7, 8], "Avelyn");
        let found = candidates(&data, 0x1000, &n, &[], false);
        let mut got: Vec<(usize, usize)> = found.iter().map(|c| (c.addr, c.name)).collect();
        got.sort();
        assert_eq!(got, vec![(0x1000 + 64, 0), (0x1000 + 136, 2), (0x1000 + 264, 1)]);
        let first = found.iter().find(|c| c.addr == 0x1000 + 64).unwrap();
        assert_eq!(first.before.len(), 32);
        assert_eq!(&first.before[24..], &[0xfc, 0x20, 0xd4, 0x44, 0x00, 0xa6, 0x02, 0x80]);
        assert_eq!(first.after.len(), 64);
    }

    #[test]
    fn text_that_is_only_part_of_something_else_is_not() {
        let _one = one_at_a_time();
        let n = names();
        let mut data = zeroed(1024);
        // the end of a longer text (a printable character right before)
        plant(&mut data, 0, [0; 8], "xShortsword");
        // a longer text that starts with the name (no zero right after)
        plant(&mut data, 64, [0; 8], "Shortsword of the North");
        // the name inside a sentence
        plant(&mut data, 160, [0; 8], "A Shortsword is a sword.");
        // a space right before: still part of a longer text
        let mut spaced = vec![0u8; 64];
        spaced[6..8].copy_from_slice(&0x20u16.to_le_bytes());
        spaced[8..8 + 20].copy_from_slice(&utf16z("Shortsword")[..20]);
        data[320..384].copy_from_slice(&spaced);
        // a string that is not divisible by 8 in address
        let t = utf16z("Avelyn");
        data[450 + 2..450 + 2 + t.len()].copy_from_slice(&t);
        // at the very end of the piece: the zero cannot be seen
        let end = data.len();
        data[end - 8 - 12..end - 8].copy_from_slice(&t[..12]);
        assert_eq!(candidates(&data, 0x2000, &n, &[], false), vec![], "none of these is a whole string");
        // ... and with the right header it IS found
        plant(&mut data, 640, [0; 8], "Shortsword");
        assert_eq!(candidates(&data, 0x2000, &n, &[], false).len(), 1);
    }

    #[test]
    fn the_start_of_a_piece_needs_the_character_before_it_unless_it_is_the_start_of_a_region() {
        let _one = one_at_a_time();
        let n = names();
        let mut data = zeroed(128);
        let t = utf16z("Avelyn");
        data[0..t.len()].copy_from_slice(&t);
        assert_eq!(candidates(&data, 0x4000, &n, &[], false).len(), 0, "the previous piece sees it");
        assert_eq!(candidates(&data, 0x4000, &n, &[], true).len(), 1, "nothing is before a region");
    }

    #[test]
    fn our_own_copies_are_skipped() {
        let _one = one_at_a_time();
        let n = names();
        let mut data = zeroed(256);
        plant(&mut data, 56, [0; 8], "Shortsword");
        assert_eq!(candidates(&data, 0x1000, &n, &[], false).len(), 1);
        let (over, elsewhere) = (0x1000 + 60..0x1000 + 70, 0x1000 + 100..0x1000 + 200);
        assert_eq!(candidates(&data, 0x1000, &n, std::slice::from_ref(&over), false).len(), 0, "a range that covers part of it");
        assert_eq!(candidates(&data, 0x1000, &n, std::slice::from_ref(&elsewhere), false).len(), 1, "a range elsewhere");
    }

    #[test]
    fn the_pause_gets_longer_once_names_are_written() {
        assert_eq!(pause_after(Duration::from_secs(10), 0), Duration::from_secs(4));
        assert_eq!(pause_after(Duration::from_secs(600), 0), Duration::from_secs(15));
        assert_eq!(pause_after(Duration::from_secs(10), 3), Duration::from_secs(5));
        assert_eq!(pause_after(Duration::from_secs(600), 3), Duration::from_secs(20));
    }

    /// Strings planted in this process's own heap are found by a real pass, overwritten in place at their own length, and the
    /// decoys next to them are left alone.
    #[test]
    fn a_real_pass_renames_the_strings_in_memory_and_only_those() {
        let _one = one_at_a_time();
        let dir = std::env::temp_dir().join(format!("ashen-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let expect = |s: &str| s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>();
        // 8-aligned storage
        let mut pool: Vec<u64> = vec![0; 8192];
        let bytes_of = |pool: &Vec<u64>| unsafe { std::slice::from_raw_parts(pool.as_ptr() as *const u8, pool.len() * 8) }.to_vec();
        {
            let bytes = unsafe { std::slice::from_raw_parts_mut(pool.as_mut_ptr() as *mut u8, pool.len() * 8) };
            plant(bytes, 0, [0xfc, 0x20, 0xd4, 0x44, 0x00, 0xa6, 0x02, 0x80], "Shortsword");
            plant(bytes, 64, [0x40, 0x46, 0xdc, 0x76, 0xcd, 0x01, 0x00, 0x00], "Avelyn");
            plant(bytes, 128, [0x10, 0x12, 0x10, 0x79, 0xcd, 0x01, 0x00, 0x00], "Standard Bolt");
            // the neighbours the real game has: other names, longer texts
            plant(bytes, 256, [0; 8], "Longsword");
            plant(bytes, 320, [0; 8], "Shortsword +1");
            plant(bytes, 400, [0; 8], "xShortsword");
            plant(bytes, 512, [0; 8], "Straight Sword");
        }
        let before = bytes_of(&pool);
        let log = Logger::open(&dir.join("rename.txt"), "t");
        let mut r = Renamer::new(&log, names());
        let out = r.pass(true, Duration::from_secs(120));
        let text = std::fs::read_to_string(dir.join("rename.txt")).unwrap();
        let after = bytes_of(&pool);
        // (other copies of the names may lie around in the test process; the planted ones are what is checked)
        assert!(out.found.iter().all(|n| *n >= 1), "{out:?}\n{text}");
        assert_eq!(out.written + out.failed, out.found.iter().sum::<usize>(), "{out:?}");
        // the three names, at their own length (the last padded), everything else byte for byte as it was
        assert_eq!(&after[8..28], &expect("Chainsword")[..]);
        assert_eq!(&after[72..84], &expect("Bolter")[..]);
        assert_eq!(&after[136..136 + 26], &expect("Bolt Rounds  ")[..]);
        let mut restored = after.clone();
        restored[8..28].copy_from_slice(&before[8..28]);
        restored[72..84].copy_from_slice(&before[72..84]);
        restored[136..162].copy_from_slice(&before[136..162]);
        assert_eq!(restored, before, "nothing else changed: the terminating zeros, the headers, the decoys");
        assert!(text.contains("WROTE \"Chainsword\" over \"Shortsword\"") && text.contains("found \"Shortsword\" as a whole string"), "{text}");
        // a second pass leaves the planted block alone (the old names are gone from it)
        let written_after_first = r.written_total();
        let _ = r.pass(true, Duration::from_secs(120));
        assert_eq!(bytes_of(&pool), after);
        assert!(r.written_total() >= written_after_first);
        // the game builds its strings again somewhere else: found and written there, too
        let mut moved: Vec<u64> = vec![0; 4096];
        {
            let bytes = unsafe { std::slice::from_raw_parts_mut(moved.as_mut_ptr() as *mut u8, moved.len() * 8) };
            plant(bytes, 960, [0; 8], "Shortsword");
        }
        let total = r.written_total();
        let third = r.pass(true, Duration::from_secs(120));
        assert!(third.written >= 1 && r.written_total() > total, "{third:?}");
        let moved_bytes = unsafe { std::slice::from_raw_parts(moved.as_ptr() as *const u8, moved.len() * 8) };
        assert_eq!(&moved_bytes[968..988], &expect("Chainsword")[..]);
        // a quick pass looks only where something was found before
        let quick = r.pass(false, Duration::from_secs(120));
        assert!(quick.regions >= 1 && quick.regions <= r.hot.len(), "{quick:?}");
        pool.fill(0);
        moved.fill(0);
        drop((pool, moved));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
