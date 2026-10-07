//! FromSoftware `.fmg` text tables as Dark Souls III keeps them in memory: id -> text. Pure parsing (no game access)
//! so it is tested on any OS. Layout (little endian, 64-bit offsets, "version 2"):
//!
//! ```text
//! 0x00 u8 0, u8 big_endian, u8 version, u8 0     0x04 i32 file size      0x08 i32 1
//! 0x0C i32 group count    0x10 i32 string count  0x14 i32 0xFF
//! 0x18 i64 offset of the string offset table     0x20 i64 0
//! 0x28 groups: i32 first index, i32 first id, i32 last id, i32 0          (16 bytes each)
//! string offset table: i64 per string (0 = none), each an absolute offset to a NUL-terminated UTF-16 string
//! ```

pub const HEADER_LEN: usize = 0x28;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fmg {
    pub file_size: usize,
    pub version: u8,
    pub group_count: usize,
    pub string_count: usize,
    /// (id, text), in table order; strings that are absent in the file are skipped
    pub entries: Vec<(u32, String)>,
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn u64le(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Cheap check on the first 0x28 bytes: could an FMG start here? Returns the file size the header claims.
///
/// Strict on purpose: raw memory is full of `00 00 02 00` by chance, so every constant of the header is checked
/// (little endian, version 2, the fixed 1 and 0xFF fields, the zero padding, and the offset table following the
/// groups directly).
pub fn header_file_size(h: &[u8]) -> Option<usize> {
    if h.len() < HEADER_LEN || h[0] != 0 || h[1] != 0 || h[2] != 2 || h[3] != 0 {
        return None;
    }
    if u32le(h, 8)? != 1 || u32le(h, 0x14)? != 0xFF || u64le(h, 0x20)? != 0 {
        return None;
    }
    let size = u32le(h, 4)? as usize;
    let groups = u32le(h, 0x0C)? as usize;
    let strings = u32le(h, 0x10)? as usize;
    let table = u64le(h, 0x18)? as usize;
    if !(HEADER_LEN..=64 << 20).contains(&size) || groups == 0 || groups > 20_000 || strings == 0 || strings > 400_000 {
        return None;
    }
    let after_groups = HEADER_LEN + groups * 16;
    if table < after_groups || table > after_groups + 16 || table.checked_add(strings * 8)? > size {
        return None;
    }
    Some(size)
}

/// Parse a whole FMG. `None` if it does not hold together (so memory that merely looks like one is rejected).
pub fn parse(b: &[u8]) -> Option<Fmg> {
    parse_at(b, 0)
}

/// Like [`parse`] for a table that sits in memory at address `base`: a game may have turned the string offsets into
/// absolute pointers when it loaded the file, so an offset that points inside `base..base+size` is accepted too.
pub fn parse_at(b: &[u8], base: u64) -> Option<Fmg> {
    let size = header_file_size(b)?;
    if b.len() < size {
        return None;
    }
    let b = &b[..size];
    let groups = u32le(b, 0x0C)? as usize;
    let strings = u32le(b, 0x10)? as usize;
    let table = u64le(b, 0x18)? as usize;
    let mut entries = Vec::with_capacity(strings);
    let mut seen_index = vec![false; strings];
    for g in 0..groups {
        let at = HEADER_LEN + g * 16;
        let first_index = u32le(b, at)? as usize;
        let first_id = u32le(b, at + 4)?;
        let last_id = u32le(b, at + 8)?;
        if last_id < first_id {
            return None;
        }
        let n = (last_id - first_id) as usize + 1;
        if first_index.checked_add(n)? > strings {
            return None;
        }
        for k in 0..n {
            let idx = first_index + k;
            seen_index[idx] = true;
            let raw = u64le(b, table + idx * 8)?;
            if raw == 0 {
                continue;
            }
            let off = if (raw as usize) < size {
                raw as usize
            } else if base != 0 && raw >= base && raw < base + size as u64 {
                (raw - base) as usize
            } else {
                return None;
            };
            // NUL-terminated UTF-16
            let mut units = Vec::new();
            let mut p = off;
            loop {
                let u = u16::from_le_bytes([*b.get(p)?, *b.get(p + 1)?]);
                if u == 0 {
                    break;
                }
                units.push(u);
                p += 2;
                if units.len() > 20_000 {
                    return None;
                }
            }
            entries.push((first_id + k as u32, String::from_utf16_lossy(&units)));
        }
    }
    Some(Fmg { file_size: size, version: b[2], group_count: groups, string_count: strings, entries })
}

/// Build an FMG (for tests and for patching tables later).
pub fn build(groups: &[(u32, &[Option<&str>])]) -> Vec<u8> {
    let string_count: usize = groups.iter().map(|(_, s)| s.len()).sum();
    let table = HEADER_LEN + groups.len() * 16;
    let pool_start = table + string_count * 8;
    let mut pool: Vec<u8> = Vec::new();
    let mut offsets: Vec<u64> = Vec::new();
    for (_, strs) in groups {
        for s in *strs {
            match s {
                None => offsets.push(0),
                Some(t) => {
                    offsets.push((pool_start + pool.len()) as u64);
                    for u in t.encode_utf16().chain(Some(0)) {
                        pool.extend(u.to_le_bytes());
                    }
                }
            }
        }
    }
    let size = pool_start + pool.len();
    let mut out = vec![0u8; HEADER_LEN];
    out[2] = 2;
    out[4..8].copy_from_slice(&(size as u32).to_le_bytes());
    out[8..12].copy_from_slice(&1u32.to_le_bytes());
    out[0x0C..0x10].copy_from_slice(&(groups.len() as u32).to_le_bytes());
    out[0x10..0x14].copy_from_slice(&(string_count as u32).to_le_bytes());
    out[0x14..0x18].copy_from_slice(&0xFFu32.to_le_bytes());
    out[0x18..0x20].copy_from_slice(&(table as u64).to_le_bytes());
    let mut index = 0u32;
    for (first_id, strs) in groups {
        out.extend(index.to_le_bytes());
        out.extend(first_id.to_le_bytes());
        out.extend((first_id + strs.len() as u32 - 1).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        index += strs.len() as u32;
    }
    for o in offsets {
        out.extend(o.to_le_bytes());
    }
    out.extend(pool);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_groups_and_missing_strings() {
        let a = [Some("Dagger"), None, Some("Parrying Dagger")];
        let b = [Some("Straight Sword")];
        let bytes = build(&[(1_000_000, &a), (2_000_000, &b)]);
        assert_eq!(header_file_size(&bytes), Some(bytes.len()));
        let f = parse(&bytes).unwrap();
        assert_eq!((f.group_count, f.string_count, f.version), (2, 4, 2));
        assert_eq!(
            f.entries,
            vec![(1_000_000, "Dagger".to_string()), (1_000_002, "Parrying Dagger".to_string()), (2_000_000, "Straight Sword".to_string())]
        );
    }

    #[test]
    fn random_looking_memory_is_not_mistaken_for_a_table() {
        // a deterministic pseudo-random block with plenty of 00 00 02 00 sprinkled in
        let mut x: u64 = 0x1234_5678_9ABC_DEF0;
        let mut block = vec![0u8; 1 << 20];
        for b in block.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = if x % 5 == 0 { 0 } else { (x >> 24) as u8 };
        }
        let mut hits = 0;
        for p in 0..block.len() - HEADER_LEN {
            if block[p..p + 4] == [0, 0, 2, 0] {
                block[p + 8] = 1; // even with one more field right, the rest must still reject it
            }
            if header_file_size(&block[p..p + HEADER_LEN]).is_some() {
                hits += 1;
            }
        }
        assert_eq!(hits, 0);
    }

    #[test]
    fn accepts_string_pointers_that_the_game_made_absolute() {
        let a = [Some("Dagger"), Some("Parrying Dagger")];
        let mut bytes = build(&[(1_000_000, &a)]);
        let base: u64 = 0x1_6000_0000;
        let table = u64::from_le_bytes(bytes[0x18..0x20].try_into().unwrap()) as usize;
        for i in 0..2 {
            let off = u64::from_le_bytes(bytes[table + i * 8..table + i * 8 + 8].try_into().unwrap());
            bytes[table + i * 8..table + i * 8 + 8].copy_from_slice(&(base + off).to_le_bytes());
        }
        assert!(parse(&bytes).is_none(), "without knowing where it sits, absolute pointers look like garbage");
        let f = parse_at(&bytes, base).unwrap();
        assert_eq!(f.entries, vec![(1_000_000, "Dagger".to_string()), (1_000_001, "Parrying Dagger".to_string())]);
        assert!(parse_at(&bytes, base + 0x100000).is_none(), "pointers outside the table are still rejected");
    }

    #[test]
    fn handles_non_ascii_text() {
        let s = [Some("\u{30c0}\u{30fc}\u{30af}\u{30bd}\u{30fc}\u{30eb}\u{30ba}")];
        let f = parse(&build(&[(5, &s)])).unwrap();
        assert_eq!(f.entries[0].1, "\u{30c0}\u{30fc}\u{30af}\u{30bd}\u{30fc}\u{30eb}\u{30ba}");
    }

    #[test]
    fn rejects_things_that_only_look_like_an_fmg() {
        assert!(parse(&[0u8; 100]).is_none());
        let good = build(&[(1, &[Some("x")])]);
        // truncated
        assert!(parse(&good[..good.len() - 2]).is_none());
        // a string offset pointing outside the file
        let mut bad = good.clone();
        let table = u64::from_le_bytes(bad[0x18..0x20].try_into().unwrap()) as usize;
        bad[table..table + 8].copy_from_slice(&(good.len() as u64 + 100).to_le_bytes());
        assert!(parse(&bad).is_none());
        // wrong version byte
        let mut v = good.clone();
        v[2] = 7;
        assert!(header_file_size(&v).is_none());
        // every fixed field matters
        for (at, val) in [(8usize, 2u8), (0x14, 0), (0x20, 1), (1, 1), (3, 1)] {
            let mut x = good.clone();
            x[at] = val;
            assert!(header_file_size(&x).is_none(), "byte {at:#x}");
        }
        // a group that runs past the string count
        let mut g = good.clone();
        g[HEADER_LEN + 8..HEADER_LEN + 12].copy_from_slice(&50u32.to_le_bytes());
        assert!(parse(&g).is_none());
    }
}
