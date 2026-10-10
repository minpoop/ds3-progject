//! `FMG`: the text tables of the game (item names, descriptions, ...), read and written.
//!
//! The same layout as `ashen_common::fmg` reads out of the running game (which only parses; this module also writes and
//! keeps the strings that are absent). Layout as documented by the Souls modding community (little endian, 64-bit offsets,
//! "version 2", Dark Souls III):
//!
//! ```text
//! 0x00 bytes 00 00 02 00   0x04 u32 file size   0x08 u32 1   0x0C u32 group count   0x10 u32 string count
//! 0x14 u32 0xFF   0x18 i64 offset of the string offset table   0x20 i64 0
//! 0x28 groups, 16 bytes each: u32 index of the first string, u32 first id, u32 last id, u32 0
//! the string offset table: one i64 per string (0 = no string), then the strings: UTF-16LE with a 2-byte NUL
//! ```
//!
//! A group is a run of consecutive ids whose strings are consecutive in the table. [`FmgFile::to_bytes`] writes the groups
//! as maximal runs of consecutive ids in ascending order and the strings in table order, directly after the table.
//!
//! The game's own tables are laid out differently (kit 0.9 found 0 of 47 byte-identical to what `to_bytes` writes), so a table
//! that is only to have a few texts changed is changed with [`replace_texts`]: the new strings are put at the end of the file,
//! the entries of the string table that name them are pointed there, and nothing else about the table moves.
use std::collections::BTreeMap;
use std::fmt;

pub const HEADER_LEN: usize = 0x28;
const MAX_GROUPS: usize = 1 << 20;
const MAX_STRINGS: usize = 1 << 22;
const MAX_STRING_UNITS: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FmgError {
    /// The data is not a little-endian version 2 FMG.
    NotFmg,
    Truncated(&'static str),
    Malformed(&'static str),
    /// The same id appears twice.
    DuplicateId(u32),
    /// An id that is to be changed is not in the table.
    NoSuchId(u32),
    /// A string is not valid UTF-16.
    BadText,
}

impl fmt::Display for FmgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FmgError::NotFmg => write!(f, "not a text table (FMG)"),
            FmgError::Truncated(what) => write!(f, "the text table is cut off ({what})"),
            FmgError::Malformed(why) => write!(f, "damaged text table: {why}"),
            FmgError::DuplicateId(id) => write!(f, "the text table has id {id} twice"),
            FmgError::NoSuchId(id) => write!(f, "the text table has no id {id}"),
            FmgError::BadText => write!(f, "a string of the text table is not valid UTF-16"),
        }
    }
}

impl std::error::Error for FmgError {}

/// What the header of a table says, after the checks every reader of a table makes.
struct Header {
    /// the declared file size (never more than the bytes that were given)
    size: usize,
    groups: usize,
    strings: usize,
    /// where the table of string offsets starts
    table: usize,
}

fn read_header(bytes: &[u8]) -> Result<Header, FmgError> {
    use crate::util::{i64_le, u32_le};
    if bytes.get(..4) != Some(&[0, 0, 2, 0][..]) {
        return Err(FmgError::NotFmg);
    }
    if bytes.len() < HEADER_LEN {
        return Err(FmgError::Truncated("header"));
    }
    let size = u32_le(bytes, 4).map(|v| v as usize).ok_or(FmgError::Truncated("header"))?;
    if size < HEADER_LEN || size > bytes.len() {
        return Err(FmgError::Truncated("file size"));
    }
    let b = bytes.get(..size).ok_or(FmgError::Truncated("file size"))?;
    if u32_le(b, 8) != Some(1) || u32_le(b, 0x14) != Some(0xFF) || i64_le(b, 0x20) != Some(0) {
        return Err(FmgError::Malformed("a fixed field of the header is wrong"));
    }
    let groups = u32_le(b, 0x0C).map(|v| v as usize).filter(|g| *g <= MAX_GROUPS).ok_or(FmgError::Malformed("the group count is impossible"))?;
    let strings = u32_le(b, 0x10).map(|v| v as usize).filter(|s| *s <= MAX_STRINGS).ok_or(FmgError::Malformed("the string count is impossible"))?;
    let table = i64_le(b, 0x18).and_then(|v| usize::try_from(v).ok()).ok_or(FmgError::Malformed("the string table offset is impossible"))?;
    let groups_end = HEADER_LEN + groups * 16;
    if groups_end > size {
        return Err(FmgError::Truncated("groups"));
    }
    if table < groups_end || table.checked_add(strings * 8).is_none_or(|end| end > size) {
        return Err(FmgError::Malformed("the string table is outside the file"));
    }
    Ok(Header { size, groups, strings, table })
}

/// Changes the texts of some ids of the table in `bytes` and nothing else about it: each new text is put at the end of the file
/// (after the declared size), the entry of the string table that belongs to the id is pointed at it, the declared size grows,
/// and the old string is wiped with zeros unless another entry still points at it or into it (strings shared by several ids,
/// or the tail of one string being another). The groups, the order and the places of every other string stay as they are,
/// whatever layout the game's tool chose. An id that is not in the table is an error; so is an id named twice.
pub fn replace_texts(bytes: &[u8], edits: &[(u32, &str)]) -> Result<Vec<u8>, FmgError> {
    use crate::util::{i64_le, u16_le, u32_le};
    let Header { size, groups, strings, table } = read_header(bytes)?;
    let b = bytes.get(..size).ok_or(FmgError::Truncated("file size"))?;
    for (i, (id, _)) in edits.iter().enumerate() {
        if edits[..i].iter().any(|(other, _)| other == id) {
            return Err(FmgError::Malformed("an id is edited twice"));
        }
    }
    let mut offsets: Vec<i64> = Vec::with_capacity(strings);
    for index in 0..strings {
        offsets.push(i64_le(b, table + index * 8).ok_or(FmgError::Truncated("string table"))?);
    }
    let index_of = |id: u32| -> Result<usize, FmgError> {
        for g in 0..groups {
            let at = HEADER_LEN + g * 16;
            let (Some(first_index), Some(first_id), Some(last_id)) = (u32_le(b, at), u32_le(b, at + 4), u32_le(b, at + 8)) else {
                return Err(FmgError::Truncated("groups"));
            };
            if (first_id..=last_id).contains(&id) {
                let index = first_index as usize + (id - first_id) as usize;
                return if index < strings { Ok(index) } else { Err(FmgError::Malformed("a group lies outside the string table")) };
            }
        }
        Err(FmgError::NoSuchId(id))
    };
    let strings_start = table + strings * 8;
    let mut out = b.to_vec();
    for (id, text) in edits {
        let index = index_of(*id)?;
        let old = offsets[index];
        if old != 0 {
            let start = usize::try_from(old).ok().filter(|o| *o >= HEADER_LEN && *o < size).ok_or(FmgError::Malformed("a string starts outside the file"))?;
            let mut end = start;
            loop {
                let unit = u16_le(b, end).ok_or(FmgError::Truncated("string"))?;
                end += 2;
                if unit == 0 {
                    break;
                }
            }
            // wiped only if it lies in the area of the strings and no other entry points at it or into it
            let shared = offsets.iter().enumerate().any(|(k, o)| k != index && *o > 0 && (*o as usize) >= start && (*o as usize) < end);
            if start >= strings_start && !shared {
                out[start..end].fill(0);
            }
        }
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let new_offset = out.len();
        for unit in text.encode_utf16().chain(Some(0)) {
            out.extend(unit.to_le_bytes());
        }
        let at = table + index * 8;
        out[at..at + 8].copy_from_slice(&(new_offset as i64).to_le_bytes());
        offsets[index] = new_offset as i64;
    }
    let new_size = u32::try_from(out.len()).map_err(|_| FmgError::Malformed("the table would be too big"))?;
    out[4..8].copy_from_slice(&new_size.to_le_bytes());
    Ok(out)
}

/// A text table: id -> text, where the text may be absent (null) for an id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FmgFile {
    entries: BTreeMap<u32, Option<String>>,
}

impl FmgFile {
    pub fn new() -> FmgFile {
        FmgFile::default()
    }

    /// Reads a table. Strings that are absent in the file stay absent; every other string must be valid UTF-16.
    pub fn parse(bytes: &[u8]) -> Result<FmgFile, FmgError> {
        use crate::util::{get, i64_le, u32_le};
        let Header { size, groups, strings, table } = read_header(bytes)?;
        let b = bytes.get(..size).ok_or(FmgError::Truncated("file size"))?;

        let mut entries = BTreeMap::new();
        let mut total = 0usize;
        // every string is read once; the text of all of them together cannot be longer than a small multiple of the file
        let mut budget = size.saturating_mul(4).saturating_add(1 << 16);
        for g in 0..groups {
            let at = HEADER_LEN + g * 16;
            let (Some(first_index), Some(first_id), Some(last_id), Some(zero)) = (u32_le(b, at), u32_le(b, at + 4), u32_le(b, at + 8), u32_le(b, at + 12)) else {
                return Err(FmgError::Truncated("groups"));
            };
            if zero != 0 || last_id < first_id {
                return Err(FmgError::Malformed("a group is not valid"));
            }
            let count = (last_id - first_id) as usize + 1;
            total += count;
            if total > strings || (first_index as usize).checked_add(count).is_none_or(|end| end > strings) {
                return Err(FmgError::Malformed("a group lies outside the string table"));
            }
            for k in 0..count {
                let id = first_id + k as u32;
                let index = first_index as usize + k;
                let offset = i64_le(b, table + index * 8).ok_or(FmgError::Truncated("string table"))?;
                let text = if offset == 0 {
                    None
                } else {
                    let start = usize::try_from(offset).ok().filter(|o| *o >= HEADER_LEN && *o < size).ok_or(FmgError::Malformed("a string starts outside the file"))?;
                    let mut units: Vec<u16> = Vec::new();
                    let mut p = start;
                    loop {
                        let pair = get(b, p, 2).ok_or(FmgError::Truncated("string"))?;
                        let u = u16::from_le_bytes([pair[0], pair[1]]);
                        if u == 0 {
                            break;
                        }
                        units.push(u);
                        budget = budget.checked_sub(1).ok_or(FmgError::Malformed("the strings overlap or are far too long"))?;
                        if units.len() > MAX_STRING_UNITS {
                            return Err(FmgError::Malformed("a string is too long"));
                        }
                        p += 2;
                    }
                    Some(String::from_utf16(&units).map_err(|_| FmgError::BadText)?)
                };
                if entries.insert(id, text).is_some() {
                    return Err(FmgError::DuplicateId(id));
                }
            }
        }
        Ok(FmgFile { entries })
    }

    /// The header numbers of a table, for a report when it does not parse: structure only, no text. Never fails.
    pub fn describe_header(bytes: &[u8]) -> String {
        use crate::util::{i64_le, u32_le};
        let word = |at: usize| u32_le(bytes, at).map_or("n/a".to_string(), |v| format!("{v:#x}"));
        format!(
            "{} bytes; first bytes {:02x?}; declared size {}; word at 8 {}; groups {}; strings {}; word at 0x14 {}; string table at {}; word at 0x20 {}",
            bytes.len(),
            bytes.get(..4).unwrap_or(&[]),
            word(4),
            word(8),
            word(0x0C),
            word(0x10),
            word(0x14),
            i64_le(bytes, 0x18).map_or("n/a".to_string(), |v| format!("{v:#x}")),
            i64_le(bytes, 0x20).map_or("n/a".to_string(), |v| format!("{v:#x}"))
        )
    }

    /// The text of `id`; `None` if the id is absent or its string is null.
    pub fn get(&self, id: u32) -> Option<&str> {
        self.entries.get(&id)?.as_deref()
    }

    /// Is `id` in the table (even with a null string)?
    pub fn contains(&self, id: u32) -> bool {
        self.entries.contains_key(&id)
    }

    /// Sets the text of `id`, adding the id if the table does not have it.
    pub fn set(&mut self, id: u32, text: &str) {
        self.entries.insert(id, Some(text.to_string()));
    }

    /// Makes the string of `id` null (adding the id if needed).
    pub fn set_null(&mut self, id: u32) {
        self.entries.insert(id, None);
    }

    /// Removes `id` from the table.
    pub fn remove(&mut self, id: u32) -> bool {
        self.entries.remove(&id).is_some()
    }

    /// Number of ids, null strings included.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of ids with a text.
    pub fn text_count(&self) -> usize {
        self.entries.values().filter(|t| t.is_some()).count()
    }

    /// `(id, text)` in ascending id order; the text is `None` for a null string.
    pub fn iter(&self) -> impl Iterator<Item = (u32, Option<&str>)> + '_ {
        self.entries.iter().map(|(id, text)| (*id, text.as_deref()))
    }

    /// Lowest and highest id.
    pub fn id_range(&self) -> Option<(u32, u32)> {
        Some((*self.entries.keys().next()?, *self.entries.keys().next_back()?))
    }

    /// The table as file bytes: groups of consecutive ids, then the offset table, then the strings in table order.
    pub fn to_bytes(&self) -> Vec<u8> {
        // runs of consecutive ids: (first id, last id, index of the first string)
        let mut groups: Vec<(u32, u32, u32)> = Vec::new();
        for (index, id) in self.entries.keys().enumerate() {
            match groups.last_mut() {
                Some((_, last, _)) if last.checked_add(1) == Some(*id) => *last = *id,
                _ => groups.push((*id, *id, index as u32)),
            }
        }
        let strings = self.entries.len();
        let table = HEADER_LEN + groups.len() * 16;
        let pool_start = table + strings * 8;
        let mut offsets: Vec<u8> = Vec::with_capacity(strings * 8);
        let mut pool: Vec<u8> = Vec::new();
        for text in self.entries.values() {
            match text {
                None => offsets.extend(0i64.to_le_bytes()),
                Some(t) => {
                    offsets.extend(((pool_start + pool.len()) as i64).to_le_bytes());
                    for u in t.encode_utf16().chain(Some(0)) {
                        pool.extend(u.to_le_bytes());
                    }
                }
            }
        }
        let size = pool_start + pool.len();
        let mut out = Vec::with_capacity(size);
        out.extend([0, 0, 2, 0]);
        out.extend(u32::try_from(size).unwrap_or(u32::MAX).to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend((groups.len() as u32).to_le_bytes());
        out.extend((strings as u32).to_le_bytes());
        out.extend(0xFFu32.to_le_bytes());
        out.extend((table as i64).to_le_bytes());
        out.extend(0i64.to_le_bytes());
        for (first, last, index) in &groups {
            out.extend(index.to_le_bytes());
            out.extend(first.to_le_bytes());
            out.extend(last.to_le_bytes());
            out.extend(0u32.to_le_bytes());
        }
        out.extend(offsets);
        out.extend(pool);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FmgFile {
        let mut f = FmgFile::new();
        for (id, text) in [(100u32, "Estus Flask"), (101, "Ashen Estus Flask"), (103, "Ember"), (2_000_000, "Shortsword"), (2_000_001, "Longsword"), (14_090_000, "Avelyn")] {
            f.set(id, text);
        }
        f.set_null(102);
        f.set_null(2_000_002);
        f.set(5, "\u{30c0}\u{30fc}\u{30af}\u{30bd}\u{30fc}\u{30eb}\u{30ba} and \u{1f600} and a\nnewline");
        f
    }

    #[test]
    fn builds_parses_modifies_and_writes_again() {
        let f = sample();
        let bytes = f.to_bytes();
        assert_eq!(&bytes[..4], &[0, 0, 2, 0]);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len(), "the size field is the file size");
        assert_eq!(&bytes[8..12], &[1, 0, 0, 0]);
        assert_eq!(&bytes[0x14..0x18], &[0xFF, 0, 0, 0]);
        // groups: 5 | 100..=103 | 2000000..=2000002 | 14090000
        assert_eq!(u32::from_le_bytes(bytes[0x0C..0x10].try_into().unwrap()), 4);
        assert_eq!(u32::from_le_bytes(bytes[0x10..0x14].try_into().unwrap()), 9);
        assert_eq!(i64::from_le_bytes(bytes[0x18..0x20].try_into().unwrap()), 0x28 + 4 * 16);
        let group = |g: usize| -> [u32; 4] { std::array::from_fn(|k| u32::from_le_bytes(bytes[0x28 + g * 16 + k * 4..0x28 + g * 16 + k * 4 + 4].try_into().unwrap())) };
        assert_eq!(group(0), [0, 5, 5, 0]);
        assert_eq!(group(1), [1, 100, 103, 0]);
        assert_eq!(group(2), [5, 2_000_000, 2_000_002, 0]);
        assert_eq!(group(3), [8, 14_090_000, 14_090_000, 0]);
        let back = FmgFile::parse(&bytes).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.get(102), None);
        assert!(back.contains(102), "a null string keeps its id");
        assert_eq!(back.get(2_000_000), Some("Shortsword"));
        assert_eq!(back.get(7), None);
        assert!(!back.contains(7));
        assert_eq!((back.len(), back.text_count()), (9, 7));
        assert_eq!(back.id_range(), Some((5, 14_090_000)));
        // unmodified: writing gives the same bytes again
        assert_eq!(back.to_bytes(), bytes);
        // modify: replace one, add one, null one; write; read
        let mut m = back.clone();
        m.set(2_000_000, "Chainsword");
        m.set(2_000_003, "A new one");
        m.set_null(103);
        m.set(100, "");
        let again = FmgFile::parse(&m.to_bytes()).unwrap();
        assert_eq!(again, m);
        assert_eq!(again.get(2_000_000), Some("Chainsword"));
        assert_eq!(again.get(2_000_003), Some("A new one"));
        assert_eq!(again.get(103), None);
        assert_eq!(again.get(100), Some(""), "an empty string is not a null string");
        assert_eq!(again.get(2_000_001), Some("Longsword"), "the others are untouched");
        // the groups follow the ids: 2000003 joined the run
        let bytes = m.to_bytes();
        assert_eq!(u32::from_le_bytes(bytes[0x0C..0x10].try_into().unwrap()), 4);
        assert!(m.remove(2_000_003) && !m.remove(2_000_003));
    }

    #[test]
    fn the_empty_table_and_odd_ids() {
        let empty = FmgFile::new();
        let bytes = empty.to_bytes();
        assert_eq!(bytes.len(), HEADER_LEN);
        assert_eq!(FmgFile::parse(&bytes).unwrap(), empty);
        assert_eq!(empty.id_range(), None);
        let mut f = FmgFile::new();
        f.set(0, "zero");
        f.set(u32::MAX, "max");
        f.set(u32::MAX - 1, "almost max");
        let back = FmgFile::parse(&f.to_bytes()).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.iter().map(|(id, _)| id).collect::<Vec<_>>(), vec![0, u32::MAX - 1, u32::MAX]);
    }

    #[test]
    fn reads_the_layout_of_the_game_and_not_only_its_own_output() {
        // The layout is the one documented in `ashen_common::fmg` (which cannot be a dependency of this crate; the setup
        // crate's tests check the two against each other): a hand-made file in that layout, with strings in an order that
        // is not the table order and a gap before the table.
        let mut b = vec![0u8; HEADER_LEN];
        b[2] = 2;
        b[8] = 1;
        b[0x0C] = 1; // one group
        b[0x10] = 3; // three strings
        b[0x14] = 0xFF;
        let table = HEADER_LEN + 16 + 8; // 8 bytes of padding after the group
        b[0x18..0x20].copy_from_slice(&(table as i64).to_le_bytes());
        b.extend([0, 0, 0, 0, 7, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0]); // group: index 0, ids 7..=9
        b.extend([0xAA; 8]);
        let pool = table + 24;
        let second = pool + 6; // "ab\0" is 6 bytes
        b.extend((second as i64).to_le_bytes()); // id 7 -> the second string in the pool
        b.extend(0i64.to_le_bytes()); // id 8 null
        b.extend((pool as i64).to_le_bytes()); // id 9 -> the first string in the pool
        for u in "ab".encode_utf16().chain(Some(0)).chain("cde".encode_utf16()).chain(Some(0)) {
            b.extend(u.to_le_bytes());
        }
        let size = b.len() as u32;
        b[4..8].copy_from_slice(&size.to_le_bytes());
        let f = FmgFile::parse(&b).unwrap();
        assert_eq!((f.get(7), f.get(8), f.get(9)), (Some("cde"), None, Some("ab")));
        assert!(f.contains(8));
        // trailing bytes after the declared size are ignored
        let mut longer = b.clone();
        longer.extend([1, 2, 3]);
        assert_eq!(FmgFile::parse(&longer).unwrap(), f);
    }

    /// A table laid out as a tool of the game might: a gap before the offset table, the strings in another order than the table,
    /// a null string, two groups. Returns the bytes and the place of the offset table.
    fn game_like() -> (Vec<u8>, usize) {
        let mut b = vec![0u8; HEADER_LEN];
        b[2] = 2;
        b[8] = 1;
        b[0x0C] = 2; // two groups
        b[0x10] = 4; // four strings
        b[0x14] = 0xFF;
        let table = HEADER_LEN + 32 + 8; // 8 bytes of padding after the groups
        b[0x18..0x20].copy_from_slice(&(table as i64).to_le_bytes());
        b.extend([0, 0, 0, 0, 7, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0]); // group 0: index 0, ids 7..=9
        b.extend([3, 0, 0, 0, 20, 0, 0, 0, 20, 0, 0, 0, 0, 0, 0, 0]); // group 1: index 3, id 20
        b.extend([0xAA; 8]);
        let pool = table + 32;
        let (ab, cde, last) = (pool, pool + 6, pool + 6 + 8); // "ab\0" 6 bytes, "cde\0" 8 bytes
        b.extend((cde as i64).to_le_bytes()); // id 7 -> the second string of the pool
        b.extend(0i64.to_le_bytes()); // id 8 null
        b.extend((ab as i64).to_le_bytes()); // id 9 -> the first string
        b.extend((last as i64).to_le_bytes()); // id 20
        for text in ["ab", "cde", "twenty"] {
            for u in text.encode_utf16().chain(Some(0)) {
                b.extend(u.to_le_bytes());
            }
        }
        let size = b.len() as u32;
        b[4..8].copy_from_slice(&size.to_le_bytes());
        (b, table)
    }

    #[test]
    fn replacing_texts_changes_what_it_is_told_to_and_nothing_else_about_a_table_of_the_games_layout() {
        let (original, table) = game_like();
        let before = FmgFile::parse(&original).unwrap();
        assert_eq!((before.get(7), before.get(8), before.get(9), before.get(20)), (Some("cde"), None, Some("ab"), Some("twenty")));
        let out = replace_texts(&original, &[(9, "A longer text"), (20, "x")]).unwrap();
        let after = FmgFile::parse(&out).unwrap();
        assert_eq!((after.get(7), after.get(8), after.get(9), after.get(20)), (Some("cde"), None, Some("A longer text"), Some("x")), "the others stay: the null one, too");
        assert_eq!(after.len(), before.len());
        // everything before the strings is the same but the declared size and the two entries that were repointed
        let declared = u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize;
        assert_eq!(declared, out.len(), "the declared size follows");
        assert_eq!(&out[..4], &original[..4]);
        assert_eq!(&out[8..table], &original[8..table], "the header and the groups, and the gap (0xAA) with them");
        let entry = |bytes: &[u8], i: usize| i64::from_le_bytes(bytes[table + i * 8..table + i * 8 + 8].try_into().unwrap());
        assert_eq!((entry(&out, 0), entry(&out, 1)), (entry(&original, 0), entry(&original, 1)), "id 7 and the null id 8 keep their entries");
        assert_ne!(entry(&out, 2), entry(&original, 2));
        assert_ne!(entry(&out, 3), entry(&original, 3));
        // the new strings are at the end, in the order they were given, and the old ones are wiped
        let (a, b) = (entry(&out, 2) as usize, entry(&out, 3) as usize);
        assert!(a >= original.len() && b > a && a % 2 == 0 && b % 2 == 0);
        let utf16 = |s: &str| -> Vec<u8> { s.encode_utf16().chain(Some(0)).flat_map(|u| u.to_le_bytes()).collect() };
        assert_eq!(&out[a..a + utf16("A longer text").len()], utf16("A longer text").as_slice());
        assert_eq!(out.len(), b + utf16("x").len());
        let pool = table + 32;
        assert_eq!(&out[pool..pool + 6], &[0u8; 6], "the old \"ab\" is gone");
        assert_eq!(&out[pool + 6..pool + 14], utf16("cde").as_slice(), "\"cde\" was not touched");
        assert_eq!(&out[pool + 14..pool + 14 + 14], &[0u8; 14], "the old \"twenty\" is gone");
        for gone in ["twenty", "ab"] {
            assert!(!out.windows(utf16(gone).len()).any(|w| w == utf16(gone).as_slice()), "the old text {gone:?} is not left anywhere in the table");
        }
        // a null string can be given a text, and a text can be replaced again
        let again = replace_texts(&out, &[(8, "now it has one"), (9, "second change")]).unwrap();
        let again_parsed = FmgFile::parse(&again).unwrap();
        assert_eq!((again_parsed.get(8), again_parsed.get(9), again_parsed.get(7), again_parsed.get(20)), (Some("now it has one"), Some("second change"), Some("cde"), Some("x")));
        // nothing to change: the very same bytes
        assert_eq!(replace_texts(&original, &[]).unwrap(), original);
    }

    #[test]
    fn a_string_that_another_entry_uses_is_not_wiped() {
        let (original, table) = game_like();
        // id 20 now points at the very same string as id 9 ("ab"), and id 7 at the tail of "twenty" ("enty")
        let mut shared = original.clone();
        let ab = i64::from_le_bytes(shared[table + 16..table + 24].try_into().unwrap());
        shared[table + 24..table + 32].copy_from_slice(&ab.to_le_bytes());
        let twenty = i64::from_le_bytes(original[table + 24..table + 32].try_into().unwrap());
        // "twenty": the tail "enty" starts two characters (4 bytes) in
        shared[table..table + 8].copy_from_slice(&(twenty + 4).to_le_bytes());
        let f = FmgFile::parse(&shared).unwrap();
        assert_eq!((f.get(9), f.get(20), f.get(7)), (Some("ab"), Some("ab"), Some("enty")));
        // changing id 9 leaves id 20 with its text: the string was shared
        let out = replace_texts(&shared, &[(9, "changed")]).unwrap();
        let g = FmgFile::parse(&out).unwrap();
        assert_eq!((g.get(9), g.get(20), g.get(7)), (Some("changed"), Some("ab"), Some("enty")));
        // and a string whose tail is another entry's string is kept whole as well: wiping it would cut "enty" off
        let h = FmgFile::parse(&replace_texts(&shared, &[(20, "other")]).unwrap()).unwrap();
        assert_eq!((h.get(9), h.get(20)), (Some("ab"), Some("other")));
        // here nothing else points at the string of id 9, so it is wiped; but changing the one whose tail is used is not wiped
        let mut tail_user = original.clone();
        let tail_at = i64::from_le_bytes(original[table + 24..table + 32].try_into().unwrap()) + 4;
        tail_user[table + 16..table + 24].copy_from_slice(&tail_at.to_le_bytes()); // id 9 -> "enty"
        let k = FmgFile::parse(&tail_user).unwrap();
        assert_eq!((k.get(9), k.get(20)), (Some("enty"), Some("twenty")));
        let out = replace_texts(&tail_user, &[(20, "short")]).unwrap();
        let k2 = FmgFile::parse(&out).unwrap();
        assert_eq!((k2.get(9), k2.get(20)), (Some("enty"), Some("short")), "\"twenty\" was not wiped: id 9 still reads into it");
    }

    #[test]
    fn replacing_texts_refuses_what_it_cannot_do_and_pads_an_odd_size() {
        let (original, _) = game_like();
        assert_eq!(replace_texts(&original, &[(8_000, "x")]).unwrap_err(), FmgError::NoSuchId(8_000));
        assert_eq!(replace_texts(&original, &[(9, "x"), (9, "y")]).unwrap_err(), FmgError::Malformed("an id is edited twice"));
        assert_eq!(replace_texts(b"not a table", &[(9, "x")]).unwrap_err(), FmgError::NotFmg);
        assert!(replace_texts(&original[..original.len() - 3], &[(9, "x")]).is_err(), "a cut-off table");
        // a declared size that is odd: the new string still starts on an even offset
        let mut odd = original.clone();
        odd.push(0x55);
        let size = odd.len() as u32;
        odd[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(FmgFile::parse(&odd).unwrap().get(9), Some("ab"));
        let out = replace_texts(&odd, &[(9, "padded")]).unwrap();
        let (_, table) = game_like();
        let at = i64::from_le_bytes(out[table + 16..table + 24].try_into().unwrap()) as usize;
        assert_eq!(at % 2, 0);
        assert_eq!(FmgFile::parse(&out).unwrap().get(9), Some("padded"));
    }

    #[test]
    fn replacing_texts_never_panics_on_damaged_tables() {
        let (good, _) = game_like();
        let mut seed = 0x1234_5678_9ABC_DEF1u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..3000 {
            let mut x = good.clone();
            for _ in 0..(1 + next() % 5) {
                let at = (next() as usize) % x.len();
                match next() % 3 {
                    0 => x[at] = next() as u8,
                    1 => x[at] ^= 1 << (next() % 8),
                    _ => {
                        for (k, w) in [0xFFu8, 0xFF, 0xFF, 0x7F].iter().enumerate() {
                            if let Some(slot) = x.get_mut(at + k) {
                                *slot = *w;
                            }
                        }
                    }
                }
            }
            if let Ok(out) = replace_texts(&x, &[(9, "new"), (20, "text")]) {
                // whatever was changed reads back, unless the damage was somewhere the reader does not look at
                if let (Ok(before), Ok(after)) = (FmgFile::parse(&x), FmgFile::parse(&out)) {
                    assert_eq!((after.get(9), after.get(20)), (Some("new"), Some("text")));
                    assert_eq!(after.len(), before.len());
                }
            }
        }
    }

    #[test]
    fn rejects_what_is_not_a_table() {
        let good = sample().to_bytes();
        assert_eq!(FmgFile::parse(b"").unwrap_err(), FmgError::NotFmg);
        assert_eq!(FmgFile::parse(b"BND4").unwrap_err(), FmgError::NotFmg);
        assert_eq!(FmgFile::parse(&[0, 0, 2]).unwrap_err(), FmgError::NotFmg);
        assert!(matches!(FmgFile::parse(&[0, 0, 2, 0, 1, 2]), Err(FmgError::Truncated(_))));
        for cut in 0..good.len() {
            assert!(FmgFile::parse(&good[..cut]).is_err(), "cut at {cut}");
        }
        // other versions and byte orders
        for (at, v) in [(0usize, 1u8), (1, 1), (2, 1), (2, 3), (3, 1)] {
            let mut x = good.clone();
            x[at] = v;
            assert!(FmgFile::parse(&x).is_err(), "byte {at}={v}");
        }
        // fixed fields
        for at in [8usize, 0x14, 0x20] {
            let mut x = good.clone();
            x[at] ^= 1;
            assert!(FmgFile::parse(&x).is_err(), "byte {at:#x}");
        }
        // the declared size, counts and offsets
        for (at, value) in [(4usize, 0u32), (4, good.len() as u32 + 1), (0x0C, u32::MAX), (0x10, u32::MAX), (0x10, 3), (0x10, 0)] {
            let mut x = good.clone();
            x[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(FmgFile::parse(&x).is_err(), "field {at:#x} = {value}");
        }
        // no groups at all is an empty table, whatever the string count says
        let mut x = good.clone();
        x[0x0C..0x10].copy_from_slice(&0u32.to_le_bytes());
        assert!(FmgFile::parse(&x).unwrap().is_empty());
        let mut x = good.clone();
        x[0x18..0x20].copy_from_slice(&(good.len() as i64).to_le_bytes());
        assert!(FmgFile::parse(&x).is_err(), "table at the end");
        let mut x = good.clone();
        x[0x18..0x20].copy_from_slice(&(-8i64).to_le_bytes());
        assert!(FmgFile::parse(&x).is_err());
        let mut x = good.clone();
        x[0x18..0x20].copy_from_slice(&0x30i64.to_le_bytes());
        assert!(FmgFile::parse(&x).is_err(), "table overlapping the groups");
        // a group: last before first, a nonzero reserved word, past the string table, the same id twice
        let g = HEADER_LEN;
        let mut x = good.clone();
        x[g + 8..g + 12].copy_from_slice(&1u32.to_le_bytes());
        assert!(FmgFile::parse(&x).is_err());
        let mut x = good.clone();
        x[g + 12] = 1;
        assert!(FmgFile::parse(&x).is_err());
        let mut x = good.clone();
        x[g + 8..g + 12].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert!(FmgFile::parse(&x).is_err());
        let mut x = good.clone();
        x[g + 16 + 4..g + 16 + 8].copy_from_slice(&5u32.to_le_bytes()); // group 1 now starts at id 5 (as group 0 does)
        x[g + 16 + 8..g + 16 + 12].copy_from_slice(&8u32.to_le_bytes());
        assert!(matches!(FmgFile::parse(&x), Err(FmgError::DuplicateId(5)) | Err(FmgError::Malformed(_))));
        // string offsets: outside, into the header, negative, unterminated
        let table = i64::from_le_bytes(good[0x18..0x20].try_into().unwrap()) as usize;
        for bad in [good.len() as i64, good.len() as i64 + 10, 4, 0x27, -2, i64::MAX, (good.len() - 1) as i64] {
            let mut x = good.clone();
            x[table..table + 8].copy_from_slice(&bad.to_le_bytes());
            assert!(FmgFile::parse(&x).is_err(), "offset {bad}");
        }
        // unpaired surrogate
        let mut f = FmgFile::new();
        f.set(1, "ab");
        let mut x = f.to_bytes();
        let n = x.len();
        x[n - 6..n - 4].copy_from_slice(&0xD800u16.to_le_bytes());
        assert_eq!(FmgFile::parse(&x).unwrap_err(), FmgError::BadText);
    }

    #[test]
    fn overlapping_strings_cannot_make_the_reader_do_unbounded_work() {
        // 100000 ids whose offsets all point at the same long string
        let mut b = vec![0u8; HEADER_LEN];
        b[2] = 2;
        b[8] = 1;
        let count = 100_000usize;
        b[0x0C] = 1;
        b[0x10..0x14].copy_from_slice(&(count as u32).to_le_bytes());
        b[0x14] = 0xFF;
        let table = HEADER_LEN + 16;
        b[0x18..0x20].copy_from_slice(&(table as i64).to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend((count as u32).to_le_bytes());
        b.extend(0u32.to_le_bytes());
        let pool = table + count * 8;
        for _ in 0..count {
            b.extend((pool as i64).to_le_bytes());
        }
        b.extend(std::iter::repeat_n([0x41u8, 0], 200_000).flatten());
        b.extend([0, 0]);
        let size = b.len() as u32;
        b[4..8].copy_from_slice(&size.to_le_bytes());
        let started = std::time::Instant::now();
        assert!(FmgFile::parse(&b).is_err());
        assert!(started.elapsed().as_secs() < 5, "bounded work");
    }

    #[test]
    fn nothing_panics_on_damaged_tables() {
        let good = sample().to_bytes();
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..3000 {
            let mut x = good.clone();
            for _ in 0..(1 + next() % 5) {
                let at = (next() as usize) % x.len();
                match next() % 3 {
                    0 => x[at] = next() as u8,
                    1 => x[at] ^= 1 << (next() % 8),
                    _ => {
                        for (k, w) in [0xFFu8, 0xFF, 0xFF, 0x7F].iter().enumerate() {
                            if let Some(slot) = x.get_mut(at + k) {
                                *slot = *w;
                            }
                        }
                    }
                }
            }
            if let Ok(f) = FmgFile::parse(&x) {
                // whatever was read can be written and read again
                assert_eq!(FmgFile::parse(&f.to_bytes()).unwrap(), f);
            }
        }
    }
}
