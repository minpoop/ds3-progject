//! Finding the game's text files by what they contain, not by the hash of their path.
//!
//! The usual way to find `msg/engUS/item.msgbnd.dcx` is the hash of its path ([`crate::hash`]). If that finds nothing - the
//! folder may be called something else, or the hash rule may differ from the one documented - this looks at the start of
//! the files themselves, the way a person would: does the file start like a compressed file (`DCX`)? Does that hold a `BND4`
//! container? Do the names of the files inside end in `.fmg` (text tables)? Only a few KB of each file are read (the
//! 16-byte blocks that cover the start), and nothing is written anywhere.
//!
//! What it returns is a list of *text bundles* with the hash of their (unknown) path. The caller decides which one is the
//! English item text - by looking at the text inside, not at the name - and what to do with it.
use crate::archive::Archive;
use crate::bnd4;
use crate::dcx::{self, DcxError};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// How hard to look.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Files with a stored size outside `min_stored..=max_stored` are not looked at (text bundles are 10 KB to a few MB).
    pub min_stored: u32,
    pub max_stored: u32,
    /// Bytes read from the start of each file.
    pub head: usize,
    /// Bytes of the decoded data that are looked at (the container's header and the names of its files).
    pub inflate: usize,
    /// Stop after this long; the result says so.
    pub budget: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { min_stored: 4 << 10, max_stored: 24 << 20, head: 16 << 10, inflate: 64 << 10, budget: Duration::from_secs(240) }
    }
}

/// A container whose files are text tables (`.fmg`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub archive: String,
    /// Index into the archive's `entries()`.
    pub entry_index: usize,
    pub hash: u32,
    pub stored: u32,
    pub unpadded: Option<u64>,
    /// The file is wrapped in DCX (as the game's are); `None` for a bare container.
    pub dcx_variant: Option<String>,
    pub decoded_size: Option<u32>,
    pub file_count: usize,
    /// The names of the files inside, as far as they could be read.
    pub names: Vec<String>,
    /// How many of `names` end in `.fmg`.
    pub tables: usize,
}

/// What was seen in one archive.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ArchiveStats {
    pub name: String,
    pub files: usize,
    /// Files whose start was read.
    pub looked_at: usize,
    /// Files that were not looked at because of their size.
    pub skipped_by_size: usize,
    /// Files whose start could not be read.
    pub unreadable: usize,
    /// Start like a DCX file of the usual kind (deflate).
    pub dcx: usize,
    /// DCX files of another kind, or with an unusual header.
    pub other_dcx: usize,
    /// DCX files that could not be inflated at all.
    pub damaged_dcx: usize,
    /// Containers (`BND4`), wrapped or bare.
    pub bnd4: usize,
    pub bundles: usize,
    /// What the files start with: the first four bytes as text (`BND4`, `TPF.`, `FSB5`, ...) or in hex, with `DCX>` in front
    /// for what is inside a DCX file.
    pub kinds: BTreeMap<String, usize>,
}

/// Everything the search saw.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Search {
    pub archives: Vec<ArchiveStats>,
    pub bundles: Vec<Bundle>,
    pub elapsed: Duration,
    /// Why the search stopped before the end (time ran out).
    pub stopped: Option<String>,
}

impl Search {
    pub fn looked_at(&self) -> usize {
        self.archives.iter().map(|a| a.looked_at).sum()
    }
}

fn is_table_name(name: &str) -> bool {
    name.to_lowercase().ends_with(".fmg")
}

/// The first four bytes of something, as text when they are letters (`BND4`, `TPF.`), else in hex.
fn magic(bytes: &[u8]) -> String {
    let Some(m) = bytes.get(..4) else { return "(under 4 bytes)".to_string() };
    if m.iter().all(|b| b.is_ascii_graphic() || *b == 0) && m.iter().any(|b| b.is_ascii_graphic()) {
        m.iter().map(|b| if *b == 0 { '.' } else { *b as char }).collect()
    } else {
        format!("0x{}", crate::util::hex(m))
    }
}

/// What one file is, from its start. `Some(bundle)` for a container of text tables.
fn classify(archive: &Archive, index: usize, head: &[u8], limits: &Limits, stats: &mut ArchiveStats) -> Option<Bundle> {
    let entry = archive.entries().get(index)?;
    let (start, variant, decoded_size) = if dcx::is_dcx(head) {
        match dcx::peek(head, limits.inflate) {
            Ok(p) => {
                stats.dcx += 1;
                *stats.kinds.entry(format!("DCX>{}", magic(&p.start))).or_default() += 1;
                (p.start, Some(p.info.variant_name()), Some(p.info.declared_uncompressed))
            }
            Err(DcxError::Unsupported { .. } | DcxError::BadHeader { .. } | DcxError::Truncated { .. }) => {
                stats.other_dcx += 1;
                *stats.kinds.entry("DCX (another kind)".to_string()).or_default() += 1;
                return None;
            }
            Err(_) => {
                stats.damaged_dcx += 1;
                *stats.kinds.entry("DCX (damaged)".to_string()).or_default() += 1;
                return None;
            }
        }
    } else {
        *stats.kinds.entry(magic(head)).or_default() += 1;
        (head.to_vec(), None, None)
    };
    let peek = bnd4::peek(&start).ok()?;
    stats.bnd4 += 1;
    let tables = peek.names.iter().filter(|n| is_table_name(n)).count();
    if tables == 0 {
        return None;
    }
    stats.bundles += 1;
    Some(Bundle {
        archive: archive.name().to_string(),
        entry_index: index,
        hash: entry.hash,
        stored: entry.padded_size,
        unpadded: entry.unpadded(),
        dcx_variant: variant,
        decoded_size,
        file_count: peek.file_count,
        names: peek.names,
        tables,
    })
}

/// Looks at the start of every file of `archives` and lists the containers of text tables. `progress` hears about each archive.
pub fn find_text_bundles(archives: &[&Archive], limits: &Limits, progress: &mut dyn FnMut(&str)) -> Search {
    let started = Instant::now();
    let mut search = Search::default();
    'archives: for archive in archives {
        let mut stats = ArchiveStats { name: archive.name().to_string(), files: archive.entries().len(), ..ArchiveStats::default() };
        let mut reader = match archive.bdt_reader() {
            Ok(r) => r,
            Err(_) => {
                stats.unreadable = stats.files;
                search.archives.push(stats);
                continue;
            }
        };
        // in the order of their place in the .bdt: the disk moves forward instead of jumping about
        let mut order: Vec<usize> = (0..archive.entries().len()).collect();
        order.sort_by_key(|i| archive.entries().get(*i).map_or(0, |e| e.offset));
        for (n, index) in order.into_iter().enumerate() {
            let Some(entry) = archive.entries().get(index) else { continue };
            if entry.padded_size < limits.min_stored || entry.padded_size > limits.max_stored {
                stats.skipped_by_size += 1;
                continue;
            }
            if n % 256 == 0 && started.elapsed() > limits.budget {
                search.stopped = Some(format!("the time budget of {} s ran out while looking at {}", limits.budget.as_secs(), archive.name()));
                search.archives.push(stats);
                break 'archives;
            }
            let head = match reader.head(entry, limits.head) {
                Ok(h) => h,
                Err(_) => {
                    stats.unreadable += 1;
                    continue;
                }
            };
            stats.looked_at += 1;
            if let Some(bundle) = classify(archive, index, &head, limits, &mut stats) {
                search.bundles.push(bundle);
            }
        }
        progress(&format!("looked at {}: {} files, {} containers of text tables", archive.name(), stats.looked_at, stats.bundles));
        search.archives.push(stats);
    }
    search.elapsed = started.elapsed();
    search
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dcx::DcxInfo;
    use crate::hash::path_hash;
    use crate::testing::archive::ArchiveBuilder;
    use crate::testing::bnd4::Bnd4Spec;
    use crate::testing::keys::test_key;
    use std::path::Path;

    /// Bytes that do not compress (the files must stay bigger than the smallest size that is looked at).
    fn noise(seed: u32, len: usize) -> Vec<u8> {
        let mut x = seed | 1;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 11) as u8
            })
            .collect()
    }

    fn text_bundle(lang: &str, extra: usize) -> Vec<u8> {
        let mut spec = Bnd4Spec::new(0x74);
        for (id, name) in [(10, "GoodsName"), (11, "WeaponName"), (12, "WeaponInfo")] {
            spec = spec.file(id, &format!("N:\\FDP\\data\\INTERROOT_win64\\msg\\{lang}\\{name}.fmg"), &noise(id as u32, 2500 + extra));
        }
        dcx::encode(&spec.build(), &DcxInfo::ds3_default()).unwrap()
    }

    fn model_bundle() -> Vec<u8> {
        let mut spec = Bnd4Spec::new(0x74);
        for (id, name) in [(1, "wp_a_0200.flver"), (2, "wp_a_0200.tpf"), (3, "wp_a_0200.hkx")] {
            spec = spec.file(id, &format!("N:\\FDP\\data\\INTERROOT_win64\\parts\\{name}"), &noise(id as u32 + 50, 2500));
        }
        dcx::encode(&spec.build(), &DcxInfo::ds3_default()).unwrap()
    }

    fn build(dir: &Path) -> Archive {
        let mut b = ArchiveBuilder::new(7);
        b.add("/msg/ENGLISH/item.msgbnd.dcx", &text_bundle("ENGLISH", 3000));
        b.add("/msg/ENGLISH/menu.msgbnd.dcx", &text_bundle("ENGLISH", 100));
        b.add("/a/path/nobody/knows.msgbnd.dcx", &text_bundle("FRENCH", 500));
        b.add_encrypted("/msg/GERMAN/item.msgbnd.dcx", &text_bundle("GERMAN", 700), [5; 16], &[(0, 48), (64, 96)]);
        b.add("/parts/wp_a_0200.partsbnd.dcx", &model_bundle());
        b.add("/other/small.bin", b"too small to be looked at");
        b.add("/other/big.bin", &vec![0x55u8; 9000]);
        // DCX of another kind: the header says KRAK
        let mut krak = text_bundle("ENGLISH", 1000);
        krak[0x28..0x2C].copy_from_slice(b"KRAK");
        b.add("/other/krak.dcx", &krak);
        // a damaged DCX: the zlib stream is garbage
        let mut broken = text_bundle("ENGLISH", 1000);
        broken[0x4C] = 0;
        b.add("/other/broken.dcx", &broken);
        // a bare container of text tables (no DCX around it)
        let mut spec = Bnd4Spec::new(0x74);
        spec = spec.file(1, "x\\Table.fmg", &noise(77, 5000));
        b.add("/other/bare.bnd", &spec.build());
        b.write(dir, "Data1", &test_key(0));
        Archive::open(&dir.join("Data1.bhd"), &dir.join("Data1.bdt"), &[test_key(0).public]).unwrap()
    }

    #[test]
    fn finds_text_bundles_by_content_and_names_their_hash() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path());
        let mut lines = Vec::new();
        let s = find_text_bundles(&[&a], &Limits::default(), &mut |l| lines.push(l.to_string()));
        assert_eq!(s.stopped, None);
        let mut hashes: Vec<u32> = s.bundles.iter().map(|b| b.hash).collect();
        hashes.sort_unstable();
        let mut expected: Vec<u32> =
            ["/msg/ENGLISH/item.msgbnd.dcx", "/msg/ENGLISH/menu.msgbnd.dcx", "/a/path/nobody/knows.msgbnd.dcx", "/msg/GERMAN/item.msgbnd.dcx", "/other/bare.bnd"].iter().map(|p| path_hash(p)).collect();
        expected.sort_unstable();
        assert_eq!(hashes, expected, "{:?}", s.bundles.iter().map(|b| (&b.archive, b.hash, &b.names)).collect::<Vec<_>>());
        let english = s.bundles.iter().find(|b| b.hash == path_hash("/msg/ENGLISH/item.msgbnd.dcx")).unwrap();
        assert_eq!((english.archive.as_str(), english.file_count, english.tables), ("Data1", 3, 3));
        assert_eq!(english.dcx_variant.as_deref(), Some("DCX_DFLT_10000_44_9"));
        assert!(english.names[1].ends_with("WeaponName.fmg"), "{:?}", english.names);
        assert_eq!(a.entries()[english.entry_index].hash, english.hash);
        // the encrypted one was decrypted on the way
        assert!(s.bundles.iter().any(|b| b.hash == path_hash("/msg/GERMAN/item.msgbnd.dcx") && b.tables == 3));
        // the bare container has no DCX around it
        let bare = s.bundles.iter().find(|b| b.hash == path_hash("/other/bare.bnd")).unwrap();
        assert_eq!((bare.dcx_variant.as_deref(), bare.decoded_size, bare.tables), (None, None, 1));
        // the counts
        let st = &s.archives[0];
        assert_eq!((st.name.as_str(), st.files), ("Data1", 10));
        assert_eq!(st.skipped_by_size, 1, "the 25-byte file");
        assert_eq!(st.looked_at, 9);
        assert_eq!((st.dcx, st.other_dcx, st.damaged_dcx), (5, 1, 1), "{st:?}");
        assert_eq!((st.bnd4, st.bundles), (6, 5), "{st:?}");
        assert!(lines.iter().any(|l| l.contains("Data1")), "{lines:?}");
        assert_eq!(s.looked_at(), 9);
        assert_eq!(st.kinds.get("DCX>BND4"), Some(&5), "{:?}", st.kinds);
        assert_eq!(st.kinds.get("BND4"), Some(&1), "{:?}", st.kinds);
        assert_eq!(st.kinds.get("DCX (another kind)"), Some(&1));
        assert_eq!(st.kinds.get("DCX (damaged)"), Some(&1));
        assert_eq!(st.kinds.values().sum::<usize>(), 9, "every file that was looked at is counted once");
        assert_eq!(magic(&[0x7f, 0x80, 1, 2]), "0x7f800102");
        assert_eq!(magic(b"TPF\0"), "TPF.");
        assert_eq!(magic(&[0, 0, 0, 0]), "0x00000000");
        assert_eq!(magic(b"ab"), "(under 4 bytes)");
    }

    #[test]
    fn nothing_is_found_where_there_is_nothing() {
        let t = tempfile::tempdir().unwrap();
        let mut b = ArchiveBuilder::new(3);
        b.add("/parts/wp_a_0200.partsbnd.dcx", &model_bundle());
        b.add("/other/big.bin", &vec![1u8; 9000]);
        b.write(t.path(), "Data2", &test_key(0));
        let a = Archive::open(&t.path().join("Data2.bhd"), &t.path().join("Data2.bdt"), &[test_key(0).public]).unwrap();
        let s = find_text_bundles(&[&a], &Limits::default(), &mut |_| {});
        assert!(s.bundles.is_empty());
        assert_eq!(s.archives[0].bnd4, 1);
        assert!(find_text_bundles(&[], &Limits::default(), &mut |_| {}).bundles.is_empty());
    }

    #[test]
    fn the_time_budget_stops_the_search_and_says_so() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path());
        let limits = Limits { budget: Duration::ZERO, ..Limits::default() };
        let s = find_text_bundles(&[&a], &limits, &mut |_| {});
        assert!(s.stopped.as_deref().is_some_and(|w| w.contains("time budget")), "{:?}", s.stopped);
        assert!(s.bundles.len() < 5);
    }

    #[test]
    fn size_limits_skip_files() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path());
        let limits = Limits { min_stored: 1 << 20, ..Limits::default() };
        let s = find_text_bundles(&[&a], &limits, &mut |_| {});
        assert!(s.bundles.is_empty());
        assert_eq!(s.archives[0].skipped_by_size, 10);
    }

    #[test]
    fn a_missing_bdt_is_counted_not_fatal() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path());
        std::fs::remove_file(t.path().join("Data1.bdt")).unwrap();
        let s = find_text_bundles(&[&a], &Limits::default(), &mut |_| {});
        assert!(s.bundles.is_empty());
        assert_eq!(s.archives[0].unreadable, 10);
    }
}
