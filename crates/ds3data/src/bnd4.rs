//! `BND4`: the container that holds several files (`item.msgbnd.dcx` holds the text tables, `*.partsbnd.dcx` a weapon's
//! model and textures). Little-endian files only, as in Dark Souls III.
//!
//! Layout as documented by the Souls modding community:
//!
//! ```text
//! 0x00 "BND4"  0x04 u8 unk, u8 unk, 2 zero bytes   0x08 u8 0, u8 big endian, u8 "not bit big endian", u8 0
//! 0x0C i32 file count   0x10 i64 0x40   0x18 8 bytes version text   0x20 i64 size of a file header
//! 0x28 i64 end of the headers (includes the hash table)   0x30 u8 unicode names, u8 format, u8 extended, u8 0
//! 0x34 i32 0   0x38 i64 offset of the hash table (extended == 4) or 0
//! 0x40 the file headers, then the names, then (extended == 4) padding to 8 and the hash table, then the file data
//! file header: u8 flags, 3 zero bytes, i32 -1, i64 stored size, [i64 uncompressed size if the format has compression],
//!              data offset (i64 with long offsets, else u32), [i32 id], [u32 name offset], [Names1 only: i32 id, i32 0]
//! hash table:  i64 offset of the hashes, u32 bucket count, bytes 0x10 8 8 0, per bucket (i32 length, i32 index), then per
//!              file (u32 hash of the name, i32 index) - it ends where the headers end
//! ```
//!
//! The format byte and the flag bytes are stored bit-reversed in little-endian files; the fields of [`Format`] are the
//! decoded bits (`IDS` 0x02, `NAMES1` 0x04, `NAMES2` 0x08, `LONG_OFFSETS` 0x10, `COMPRESSION` 0x20). For example the raw
//! byte `0x74` of Dark Souls III's text files is the format `0x2E`: ids, names and the compression fields.
//!
//! [`replace_file`] changes one file of a BND4 and nothing else: it relies on (and checks, see [`Layout`]) that the files
//! are stored in header order, each at the next multiple of one alignment after the previous one, with zeros between.
use crate::util::{align_up, get, i32_le, i64_le, put, u16_le, u32_le};
use std::fmt;

pub const HEADER_LEN: usize = 0x40;
const MAX_FILES: usize = 1 << 16;
const MAX_NAME_UNITS: usize = 1024;
const MAX_HASH_BUCKETS: u32 = 1 << 20;
/// Alignments tried, in this order, when the layout is checked (0x10 is what the game's own tools write).
const ALIGNMENTS: [u64; 13] = [0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400, 0x800, 0x1000, 8, 4, 2, 1];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bnd4Error {
    /// The data does not start with `BND4`.
    NotBnd4,
    /// A big-endian file (console games); only little endian is read.
    BigEndian,
    Truncated { what: &'static str },
    Malformed(&'static str),
    /// The files are not laid out the way [`replace_file`] needs; nothing is patched.
    LayoutNotVerified(String),
    NoSuchFile { index: usize },
    /// The file has its own compression flag; [`replace_file`] does not recompress.
    CompressedEntry { index: usize },
    /// An empty file cannot be replaced, and a file cannot be replaced by nothing.
    EmptyFile { index: usize },
    /// A new data offset does not fit its field.
    OffsetOverflow,
    /// The patched file did not read back as intended (a bug in this crate, caught before anything is used).
    SelfCheck(&'static str),
}

impl fmt::Display for Bnd4Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bnd4Error::NotBnd4 => write!(f, "not a BND4 file"),
            Bnd4Error::BigEndian => write!(f, "a big-endian BND4 (not the PC format)"),
            Bnd4Error::Truncated { what } => write!(f, "the BND4 file is cut off ({what})"),
            Bnd4Error::Malformed(why) => write!(f, "damaged BND4 file: {why}"),
            Bnd4Error::LayoutNotVerified(why) => write!(f, "the files of this BND4 are not laid out the expected way, so it is not patched ({why})"),
            Bnd4Error::NoSuchFile { index } => write!(f, "the BND4 has no file number {index}"),
            Bnd4Error::CompressedEntry { index } => write!(f, "file {index} of the BND4 is compressed on its own, which cannot be patched"),
            Bnd4Error::EmptyFile { index } => write!(f, "file {index} of the BND4 is empty or would become empty"),
            Bnd4Error::OffsetOverflow => write!(f, "a file offset no longer fits its field"),
            Bnd4Error::SelfCheck(why) => write!(f, "internal check failed after patching: {why}"),
        }
    }
}

impl std::error::Error for Bnd4Error {}

/// The decoded format bits of a BND4.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Format(pub u8);

impl Format {
    pub const BIG_ENDIAN: u8 = 0x01;
    pub const IDS: u8 = 0x02;
    pub const NAMES1: u8 = 0x04;
    pub const NAMES2: u8 = 0x08;
    pub const LONG_OFFSETS: u8 = 0x10;
    pub const COMPRESSION: u8 = 0x20;

    pub fn has_ids(self) -> bool {
        self.0 & Format::IDS != 0
    }

    pub fn has_names(self) -> bool {
        self.0 & (Format::NAMES1 | Format::NAMES2) != 0
    }

    pub fn long_offsets(self) -> bool {
        self.0 & Format::LONG_OFFSETS != 0
    }

    /// Whether the file headers have an uncompressed-size field.
    pub fn has_compression_field(self) -> bool {
        self.0 & Format::COMPRESSION != 0
    }

    /// The size of one file header for this format.
    pub fn file_header_len(self) -> usize {
        0x10 + if self.long_offsets() { 8 } else { 4 }
            + if self.has_compression_field() { 8 } else { 0 }
            + if self.has_ids() { 4 } else { 0 }
            + if self.has_names() { 4 } else { 0 }
            + if self.0 == Format::NAMES1 { 8 } else { 0 }
    }

    /// The format byte as the file stores it -> the format bits. Little-endian files store the bits in reverse order,
    /// unless the header says the bit order is big endian or the byte looks like a big-endian one (bit 0 set, bit 7 clear).
    pub fn from_raw(raw: u8, bit_big_endian: bool) -> Format {
        let keep = bit_big_endian || (raw & 0x01 != 0 && raw & 0x80 == 0);
        Format(if keep { raw } else { raw.reverse_bits() })
    }
}

impl fmt::Debug for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        for (bit, name) in [(0x01, "BigEndian"), (0x02, "IDs"), (0x04, "Names1"), (0x08, "Names2"), (0x10, "LongOffsets"), (0x20, "Compression"), (0x40, "Flag6"), (0x80, "Flag7")] {
            if self.0 & bit != 0 {
                parts.push(name);
            }
        }
        write!(f, "{:#04x} ({})", self.0, if parts.is_empty() { "none".to_string() } else { parts.join("|") })
    }
}

/// One file of the container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bnd4File {
    pub index: usize,
    /// The decoded flag bits (bit 0: the file is compressed on its own; `0x02` is set on all files of the game).
    pub flags: u8,
    pub id: Option<i32>,
    pub name: Option<String>,
    pub data_offset: u64,
    pub stored_size: u64,
    /// The uncompressed-size field, if the format has one and it is not -1.
    pub uncompressed_size: Option<u64>,
    /// Where this file's header starts.
    header_pos: usize,
}

impl Bnd4File {
    pub fn is_compressed(&self) -> bool {
        self.flags & 0x01 != 0
    }
}

/// The hash table some BND4s carry after the names (`extended == 4`); it is copied unchanged by [`replace_file`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashTableInfo {
    pub offset: u64,
    pub bucket_count: u32,
    pub hashes_offset: u64,
    /// The table ends where the headers end and its parts follow each other as expected.
    pub consistent: bool,
}

/// How the file data is laid out, and whether [`replace_file`] may rely on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Files in header order, each at the next multiple of `alignment` after the previous one (or after the headers),
    /// zeros between them, and nothing (or zero padding to `alignment`) after the last one.
    pub verified: bool,
    /// The alignment that explains the offsets (0 when none does).
    pub alignment: u64,
    /// The gap before each non-empty file, in header order (the first one is the gap after the headers).
    pub gaps: Vec<u64>,
    /// Bytes after the last file.
    pub tail_len: usize,
    /// What does not fit, in words (empty when `verified`).
    pub issues: Vec<String>,
}

/// A parsed BND4 (the description; the bytes stay with the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bnd4 {
    pub unk04: u8,
    pub unk05: u8,
    pub bit_big_endian: bool,
    /// The 8-byte version text, up to its first zero byte.
    pub version: String,
    pub version_raw: [u8; 8],
    /// The format byte as stored and as decoded.
    pub format_raw: u8,
    pub format: Format,
    pub unicode: bool,
    pub extended: u8,
    pub headers_end: u64,
    pub hash_table: Option<HashTableInfo>,
    pub files: Vec<Bnd4File>,
    pub layout: Layout,
    src_len: usize,
}

fn read_name(b: &[u8], at: usize, unicode: bool) -> Result<String, Bnd4Error> {
    if unicode {
        let mut units = Vec::new();
        let mut p = at;
        loop {
            let u = u16_le(b, p).ok_or(Bnd4Error::Malformed("a name runs past the end of the file"))?;
            if u == 0 {
                break;
            }
            units.push(u);
            if units.len() > MAX_NAME_UNITS {
                return Err(Bnd4Error::Malformed("a name is too long"));
            }
            p += 2;
        }
        Ok(String::from_utf16_lossy(&units))
    } else {
        let rest = b.get(at..).ok_or(Bnd4Error::Malformed("a name starts outside the file"))?;
        let len = rest.iter().position(|c| *c == 0).ok_or(Bnd4Error::Malformed("a name runs past the end of the file"))?;
        if len > MAX_NAME_UNITS {
            return Err(Bnd4Error::Malformed("a name is too long"));
        }
        // Shift JIS is not decoded here; the names this crate cares about are plain ASCII
        Ok(rest[..len].iter().map(|c| if c.is_ascii() { *c as char } else { '\u{FFFD}' }).collect())
    }
}

impl Bnd4 {
    /// Reads the headers of a BND4 and checks every offset and size against the length of `src`.
    pub fn parse(src: &[u8]) -> Result<Bnd4, Bnd4Error> {
        if !src.starts_with(b"BND4") {
            return Err(Bnd4Error::NotBnd4);
        }
        if src.len() < HEADER_LEN {
            return Err(Bnd4Error::Truncated { what: "header" });
        }
        let flag = |at: usize| -> Result<bool, Bnd4Error> {
            match src[at] {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err(Bnd4Error::Malformed("a flag byte of the header is neither 0 nor 1")),
            }
        };
        if src[6] != 0 || src[7] != 0 || src[8] != 0 || src[11] != 0 || src[0x33] != 0 || i32_le(src, 0x34) != Some(0) {
            return Err(Bnd4Error::Malformed("a reserved field of the header is not zero"));
        }
        if flag(9)? {
            return Err(Bnd4Error::BigEndian);
        }
        let bit_big_endian = !flag(10)?;
        let file_count = i32_le(src, 0x0C).and_then(|c| usize::try_from(c).ok()).filter(|c| *c <= MAX_FILES).ok_or(Bnd4Error::Malformed("the file count is impossible"))?;
        if i64_le(src, 0x10) != Some(0x40) {
            return Err(Bnd4Error::Malformed("the header size is not 0x40"));
        }
        let mut version_raw = [0u8; 8];
        version_raw.copy_from_slice(&src[0x18..0x20]);
        let version_len = version_raw.iter().position(|b| *b == 0).unwrap_or(8);
        let version = version_raw[..version_len].iter().map(|b| if b.is_ascii() { *b as char } else { '?' }).collect();
        let file_header_len = i64_le(src, 0x20).and_then(|v| usize::try_from(v).ok()).ok_or(Bnd4Error::Malformed("the file header size is impossible"))?;
        let headers_end = i64_le(src, 0x28).and_then(|v| u64::try_from(v).ok()).ok_or(Bnd4Error::Malformed("the end of the headers is impossible"))?;
        let unicode = flag(0x30)?;
        let format_raw = src[0x31];
        let format = Format::from_raw(format_raw, bit_big_endian);
        let extended = src[0x32];
        if !matches!(extended, 0 | 1 | 4 | 0x80) {
            return Err(Bnd4Error::Malformed("the extended byte has an unknown value"));
        }
        let hash_table_offset = i64_le(src, 0x38).and_then(|v| u64::try_from(v).ok()).ok_or(Bnd4Error::Malformed("the hash table offset is impossible"))?;
        if extended == 4 {
            if hash_table_offset < HEADER_LEN as u64 || hash_table_offset >= headers_end {
                return Err(Bnd4Error::Malformed("the hash table is outside the headers"));
            }
        } else if hash_table_offset != 0 {
            return Err(Bnd4Error::Malformed("there is a hash table offset but no hash table"));
        }
        if file_header_len != format.file_header_len() {
            return Err(Bnd4Error::Malformed("the file header size does not fit the format"));
        }
        let headers_len = file_count.checked_mul(file_header_len).and_then(|n| n.checked_add(HEADER_LEN)).ok_or(Bnd4Error::Malformed("too many files"))?;
        if headers_len > src.len() {
            return Err(Bnd4Error::Truncated { what: "file headers" });
        }
        if headers_end > src.len() as u64 {
            return Err(Bnd4Error::Truncated { what: "headers" });
        }
        if headers_end < headers_len as u64 {
            return Err(Bnd4Error::Malformed("the end of the headers is before the end of the file headers"));
        }

        let mut files = Vec::with_capacity(file_count);
        for index in 0..file_count {
            let at = HEADER_LEN + index * file_header_len;
            let flags_raw = src[at];
            let flags = if bit_big_endian { flags_raw } else { flags_raw.reverse_bits() };
            if src[at + 1..at + 4] != [0, 0, 0] || i32_le(src, at + 4) != Some(-1) {
                return Err(Bnd4Error::Malformed("a reserved field of a file header is wrong"));
            }
            let stored_size = i64_le(src, at + 8).and_then(|v| u64::try_from(v).ok()).ok_or(Bnd4Error::Malformed("a file has a negative size"))?;
            let mut cursor = at + 16;
            let mut uncompressed_size = None;
            if format.has_compression_field() {
                let v = i64_le(src, cursor).ok_or(Bnd4Error::Malformed("a file header is cut off"))?;
                if v < -1 {
                    return Err(Bnd4Error::Malformed("a file has an impossible uncompressed size"));
                }
                uncompressed_size = u64::try_from(v).ok();
                cursor += 8;
            }
            let data_offset = if format.long_offsets() {
                let v = i64_le(src, cursor).and_then(|v| u64::try_from(v).ok()).ok_or(Bnd4Error::Malformed("a file has a negative offset"))?;
                cursor += 8;
                v
            } else {
                let v = u32_le(src, cursor).ok_or(Bnd4Error::Malformed("a file header is cut off"))?;
                cursor += 4;
                u64::from(v)
            };
            let mut id = None;
            if format.has_ids() {
                id = Some(i32_le(src, cursor).ok_or(Bnd4Error::Malformed("a file header is cut off"))?);
                cursor += 4;
            }
            let mut name = None;
            if format.has_names() {
                let name_offset = u32_le(src, cursor).ok_or(Bnd4Error::Malformed("a file header is cut off"))?;
                cursor += 4;
                name = Some(read_name(src, name_offset as usize, unicode)?);
            }
            if format.0 == Format::NAMES1 {
                // the layout of PC save files: an "id" after the name, then a zero
                id = Some(i32_le(src, cursor).ok_or(Bnd4Error::Malformed("a file header is cut off"))?);
                if i32_le(src, cursor + 4) != Some(0) {
                    return Err(Bnd4Error::Malformed("a reserved field of a file header is wrong"));
                }
            }
            match data_offset.checked_add(stored_size) {
                Some(end) if end <= src.len() as u64 => {}
                _ => return Err(Bnd4Error::Malformed("a file lies outside the BND4")),
            }
            files.push(Bnd4File { index, flags, id, name, data_offset, stored_size, uncompressed_size, header_pos: at });
        }

        let hash_table = (extended == 4).then(|| hash_table_info(src, hash_table_offset, headers_end, file_count));
        let layout = check_layout(src, &files, headers_end);
        Ok(Bnd4 { unk04: src[4], unk05: src[5], bit_big_endian, version, version_raw, format_raw, format, unicode, extended, headers_end, hash_table, files, layout, src_len: src.len() })
    }

    /// The header numbers of a BND4, for a report when it does not parse: structure only, no file contents. Never fails.
    pub fn describe_header(src: &[u8]) -> String {
        let num = |v: Option<i64>| v.map_or("n/a".to_string(), |v| format!("{v:#x}"));
        let byte = |at: usize| src.get(at).map_or("n/a".to_string(), |b| format!("{b:#04x}"));
        format!(
            "{} bytes; magic {:?}; bytes 4-11 {} {} {} {} {} {} {} {}; file count {}; header size {}; file header size {}; headers end {}; unicode {}; format byte {}; extended {}; hash table at {}",
            src.len(),
            String::from_utf8_lossy(src.get(..4).unwrap_or(&[])),
            byte(4),
            byte(5),
            byte(6),
            byte(7),
            byte(8),
            byte(9),
            byte(10),
            byte(11),
            i32_le(src, 0x0C).map_or("n/a".to_string(), |v| v.to_string()),
            num(i64_le(src, 0x10)),
            num(i64_le(src, 0x20)),
            num(i64_le(src, 0x28)),
            byte(0x30),
            byte(0x31),
            byte(0x32),
            num(i64_le(src, 0x38))
        )
    }

    /// The bytes of file `index` as stored (compressed on its own if its flag says so).
    pub fn file_bytes<'a>(&self, src: &'a [u8], index: usize) -> Option<&'a [u8]> {
        let f = self.files.get(index)?;
        get(src, usize::try_from(f.data_offset).ok()?, usize::try_from(f.stored_size).ok()?)
    }

    /// The length of the bytes this was parsed from.
    pub fn source_len(&self) -> usize {
        self.src_len
    }
}

fn hash_table_info(src: &[u8], offset: u64, headers_end: u64, file_count: usize) -> HashTableInfo {
    let at = usize::try_from(offset).unwrap_or(usize::MAX);
    let hashes_offset = i64_le(src, at).and_then(|v| u64::try_from(v).ok()).unwrap_or(0);
    let bucket_count = u32_le(src, at.saturating_add(8)).unwrap_or(0);
    let constants = get(src, at.saturating_add(12), 4) == Some(&[0x10, 8, 8, 0][..]);
    let groups_end = offset.saturating_add(16).saturating_add(u64::from(bucket_count) * 8);
    let consistent = constants
        && bucket_count > 0
        && bucket_count <= MAX_HASH_BUCKETS
        && hashes_offset == groups_end
        && hashes_offset.saturating_add(file_count as u64 * 8) == headers_end;
    HashTableInfo { offset, bucket_count, hashes_offset, consistent }
}

fn check_layout(src: &[u8], files: &[Bnd4File], headers_end: u64) -> Layout {
    let non_empty: Vec<&Bnd4File> = files.iter().filter(|f| f.stored_size > 0).collect();
    let mut issues = Vec::new();
    // the first alignment that explains every offset
    let fits = |align: u64| -> bool {
        let mut prev = headers_end;
        for f in &non_empty {
            if align_up(prev, align) != Some(f.data_offset) {
                return false;
            }
            prev = f.data_offset + f.stored_size;
        }
        true
    };
    let alignment = ALIGNMENTS.iter().copied().find(|a| fits(*a)).unwrap_or(0);
    let mut gaps = Vec::new();
    let mut prev = headers_end;
    for f in &non_empty {
        gaps.push(f.data_offset.saturating_sub(prev));
        prev = f.data_offset.saturating_add(f.stored_size);
    }
    let tail_len = (src.len() as u64).saturating_sub(prev) as usize;
    if alignment == 0 {
        // say where the first file is that does not follow the 0x10 rule
        let mut prev = headers_end;
        for f in &non_empty {
            let expected = align_up(prev, 0x10).unwrap_or(u64::MAX);
            if f.data_offset != expected {
                issues.push(format!("file {} starts at {:#x}, but {:#x} was expected (after the previous data, aligned to 0x10)", f.index, f.data_offset, expected));
                break;
            }
            prev = f.data_offset + f.stored_size;
        }
        if issues.is_empty() {
            issues.push("the files are not stored one after the other in header order".to_string());
        }
    } else {
        // zeros in the gaps, and nothing but alignment padding at the end
        let mut prev = headers_end;
        for f in &non_empty {
            let gap = get(src, prev as usize, (f.data_offset - prev) as usize);
            if gap.is_none_or(|g| g.iter().any(|b| *b != 0)) {
                issues.push(format!("the padding before file {} is not zero", f.index));
                break;
            }
            prev = f.data_offset + f.stored_size;
        }
        if tail_len > 0 {
            let padding = align_up(prev, alignment).map(|a| a - prev);
            let tail = get(src, prev as usize, tail_len);
            if padding != Some(tail_len as u64) || tail.is_none_or(|t| t.iter().any(|b| *b != 0)) {
                issues.push(format!("{tail_len} unexplained bytes follow the last file"));
            }
        }
    }
    Layout { verified: issues.is_empty(), alignment, gaps, tail_len, issues }
}

/// A BND4 with the data of file `index` replaced by `new_data`, everything else unchanged: the same headers, names, hash
/// table, the other files' bytes and the same padding rules. Only the size field(s) of that file and the data offsets of
/// the files after it change. Refused (see [`Bnd4Error`]) when the layout was not verified, the file is compressed on its
/// own, or it is empty; the result is parsed again and compared before it is returned.
pub fn replace_file(src: &[u8], parsed: &Bnd4, index: usize, new_data: &[u8]) -> Result<Vec<u8>, Bnd4Error> {
    // the description must be of these very bytes
    if parsed.src_len != src.len() || Bnd4::parse(src).as_ref() != Ok(parsed) {
        return Err(Bnd4Error::Malformed("the description does not belong to these bytes"));
    }
    if !parsed.layout.verified {
        return Err(Bnd4Error::LayoutNotVerified(parsed.layout.issues.join("; ")));
    }
    let target = parsed.files.get(index).ok_or(Bnd4Error::NoSuchFile { index })?;
    if target.is_compressed() {
        return Err(Bnd4Error::CompressedEntry { index });
    }
    if target.stored_size == 0 || new_data.is_empty() {
        return Err(Bnd4Error::EmptyFile { index });
    }
    let has_uncompressed_field = parsed.format.has_compression_field();
    if has_uncompressed_field && target.uncompressed_size != Some(target.stored_size) {
        return Err(Bnd4Error::LayoutNotVerified(format!("file {index} has different stored and uncompressed sizes without being compressed")));
    }
    let align = parsed.layout.alignment;
    let start = usize::try_from(target.data_offset).map_err(|_| Bnd4Error::OffsetOverflow)?;
    let mut out = Vec::with_capacity(src.len().saturating_add(new_data.len()));
    out.extend_from_slice(src.get(..start).ok_or(Bnd4Error::Malformed("a file lies outside the BND4"))?);
    out.extend_from_slice(new_data);
    let mut prev_end = target.data_offset + new_data.len() as u64;
    let mut moved: Vec<(&Bnd4File, u64)> = Vec::new();
    for later in parsed.files.iter().skip(index + 1).filter(|f| f.stored_size > 0) {
        let offset = align_up(prev_end, align).ok_or(Bnd4Error::OffsetOverflow)?;
        out.resize(out.len() + (offset - prev_end) as usize, 0);
        out.extend_from_slice(parsed.file_bytes(src, later.index).ok_or(Bnd4Error::Malformed("a file lies outside the BND4"))?);
        moved.push((later, offset));
        prev_end = offset + later.stored_size;
    }
    if parsed.layout.tail_len > 0 {
        let end = align_up(prev_end, align).ok_or(Bnd4Error::OffsetOverflow)?;
        out.resize(out.len() + (end - prev_end) as usize, 0);
    }
    // the size fields of the replaced file and the offsets of the files that moved: the only header bytes that change
    let mut changed: Vec<(usize, usize)> = Vec::new();
    let size_bytes = (new_data.len() as i64).to_le_bytes();
    put(&mut out, target.header_pos + 8, &size_bytes).ok_or(Bnd4Error::Malformed("a file header is cut off"))?;
    changed.push((target.header_pos + 8, 8));
    if has_uncompressed_field {
        put(&mut out, target.header_pos + 16, &size_bytes).ok_or(Bnd4Error::Malformed("a file header is cut off"))?;
        changed.push((target.header_pos + 16, 8));
    }
    let offset_field = 16 + if has_uncompressed_field { 8 } else { 0 };
    for (file, offset) in moved {
        let at = file.header_pos + offset_field;
        let wrote = if parsed.format.long_offsets() {
            changed.push((at, 8));
            put(&mut out, at, &(offset as i64).to_le_bytes())
        } else {
            changed.push((at, 4));
            put(&mut out, at, &u32::try_from(offset).map_err(|_| Bnd4Error::OffsetOverflow)?.to_le_bytes())
        };
        wrote.ok_or(Bnd4Error::Malformed("a file header is cut off"))?;
    }

    // read it back: it must parse, keep its structure, and everything but those fields and the data must be as before
    let back = Bnd4::parse(&out).map_err(|_| Bnd4Error::SelfCheck("the result does not parse"))?;
    if !back.layout.verified || back.files.len() != parsed.files.len() || back.format_raw != parsed.format_raw || back.version_raw != parsed.version_raw || back.headers_end != parsed.headers_end {
        return Err(Bnd4Error::SelfCheck("the result has another structure"));
    }
    for (old, new) in parsed.files.iter().zip(&back.files) {
        let same_identity = old.flags == new.flags && old.id == new.id && old.name == new.name;
        let same_bytes = if old.index == index { back.file_bytes(&out, new.index) == Some(new_data) } else { back.file_bytes(&out, new.index) == parsed.file_bytes(src, old.index) };
        if !same_identity || !same_bytes {
            return Err(Bnd4Error::SelfCheck("a file differs from what was intended"));
        }
    }
    let untouched_ok = out.iter().zip(src.iter()).take(start).enumerate().all(|(i, (a, b))| a == b || changed.iter().any(|(at, len)| (*at..*at + *len).contains(&i)));
    if !untouched_ok {
        return Err(Bnd4Error::SelfCheck("bytes outside the changed fields were altered"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::bnd4::{sample, sample_files, Bnd4Spec};

    /// Raw format bytes as stored in the files: Dark Souls III's text and model containers (0x74), and the other variants
    /// named in the brief (0x2E, 0x54, 0x70), plus long offsets (0x7C), names without ids (0x2E has none), ids without
    /// names (0x44) and the PC-save layout (0x20, "Names1" only).
    const RAW_FORMATS: [u8; 7] = [0x74, 0x2E, 0x54, 0x70, 0x7C, 0x44, 0x20];

    fn data_of(i: usize) -> Vec<u8> {
        sample_files()[i].2.clone()
    }

    #[test]
    fn the_format_byte_is_stored_bit_reversed_and_decoded() {
        for (raw, logical, header_len) in [
            (0x74u8, 0x2Eu8, 0x24usize), // ids, names, compression fields (the game's text files)
            (0x2E, 0x74, 0x24),          // names, long offsets, compression fields, flag 6
            (0x54, 0x2A, 0x24),          // ids, names (variant 2), compression fields
            (0x70, 0x0E, 0x1C),          // ids, names, no compression fields
            (0x7C, 0x3E, 0x28),          // everything: long offsets, too
            (0x44, 0x22, 0x20),          // ids and compression fields, no names
            (0x20, 0x04, 0x20),          // names only (PC saves): an extra id and a zero follow the name offset
        ] {
            let f = Format::from_raw(raw, false);
            assert_eq!(f.0, logical, "raw {raw:#x}");
            assert_eq!(f.file_header_len(), header_len, "format {logical:#x}");
        }
        assert_eq!(Format::from_raw(0x74, true).0, 0x74, "a big-endian bit order keeps the byte");
        assert_eq!(Format::from_raw(0x01, false).0, 0x01, "bit 0 set and bit 7 clear: kept as it is");
        assert_eq!(Format::from_raw(0x81, false).0, 0x81, "bit 0 and 7 set: reversed (0x81 is its own mirror)");
        let f = Format(0x2E);
        assert!(f.has_ids() && f.has_names() && f.has_compression_field() && !f.long_offsets());
        assert_eq!(format!("{f:?}"), "0x2e (IDs|Names1|Names2|Compression)");
        assert_eq!(format!("{:?}", Format(0)), "0x00 (none)");
    }

    #[test]
    fn parses_every_variant_with_and_without_the_hash_table() {
        for raw in RAW_FORMATS {
            for hash_table in [true, false] {
                let bytes = sample(raw, hash_table);
                let b = Bnd4::parse(&bytes).unwrap_or_else(|e| panic!("raw {raw:#x} hash table {hash_table}: {e}"));
                let ctx = format!("raw {raw:#x} hash table {hash_table}");
                assert_eq!((b.format_raw, b.unicode, b.extended, b.version.as_str()), (raw, true, if hash_table { 4 } else { 0 }, "07D7R6"), "{ctx}");
                assert_eq!(b.files.len(), 5, "{ctx}");
                assert!(b.layout.verified, "{ctx}: {:?}", b.layout.issues);
                assert_eq!((b.layout.alignment, b.layout.tail_len), (0x10, 0), "{ctx}");
                assert_eq!(b.hash_table.as_ref().map(|t| t.consistent), hash_table.then_some(true), "{ctx}");
                for (i, (id, name, data)) in sample_files().into_iter().enumerate() {
                    let f = &b.files[i];
                    assert_eq!(f.index, i);
                    assert_eq!(f.id, if b.format.has_ids() || b.format.0 == Format::NAMES1 { Some(id) } else { None }, "{ctx}");
                    assert_eq!(f.name.as_deref(), if b.format.has_names() { Some(name.as_str()) } else { None }, "{ctx}");
                    assert_eq!(f.stored_size, data.len() as u64, "{ctx}");
                    assert_eq!(f.uncompressed_size, if b.format.has_compression_field() { Some(data.len() as u64) } else { None }, "{ctx}");
                    assert_eq!((f.flags, f.is_compressed()), (0x02, false), "{ctx}");
                    assert_eq!(b.file_bytes(&bytes, i), Some(&data[..]), "{ctx} file {i}");
                    if !data.is_empty() {
                        assert_eq!(f.data_offset % 16, 0, "{ctx}");
                    }
                }
                assert_eq!(b.source_len(), bytes.len());
                assert_eq!(b.file_bytes(&bytes, 99), None);
                assert_eq!(b.layout.gaps.len(), 4, "the empty file has no gap");
            }
        }
    }

    #[test]
    fn replacing_a_file_with_its_own_bytes_changes_nothing() {
        for raw in RAW_FORMATS {
            for hash_table in [true, false] {
                let bytes = sample(raw, hash_table);
                let b = Bnd4::parse(&bytes).unwrap();
                for i in [0usize, 1, 2, 4] {
                    let same = replace_file(&bytes, &b, i, &data_of(i)).unwrap();
                    assert_eq!(same, bytes, "raw {raw:#x} hash table {hash_table} file {i}");
                }
            }
        }
    }

    #[test]
    fn replacing_a_file_with_other_lengths_keeps_everything_else() {
        for raw in RAW_FORMATS {
            for hash_table in [true, false] {
                let bytes = sample(raw, hash_table);
                let b = Bnd4::parse(&bytes).unwrap();
                for i in [0usize, 1, 2, 4] {
                    for new_len in [1usize, 15, 16, 17, 33, 100, 4000] {
                        let new: Vec<u8> = (0..new_len).map(|k| (k * 11 + i) as u8 | 0x80).collect();
                        let out = replace_file(&bytes, &b, i, &new).unwrap_or_else(|e| panic!("raw {raw:#x} file {i} len {new_len}: {e}"));
                        let ctx = format!("raw {raw:#x} hash table {hash_table} file {i} len {new_len}");
                        let after = Bnd4::parse(&out).unwrap_or_else(|e| panic!("{ctx}: {e}"));
                        assert!(after.layout.verified, "{ctx}");
                        assert_eq!(after.layout.alignment, 0x10, "{ctx}");
                        assert_eq!(after.files.len(), 5);
                        for j in 0..5 {
                            let expected = if j == i { &new[..] } else { &data_of(j)[..] };
                            assert_eq!(after.file_bytes(&out, j), Some(expected), "{ctx} file {j}");
                            // names, ids and flags are as before
                            assert_eq!((&after.files[j].name, after.files[j].id, after.files[j].flags), (&b.files[j].name, b.files[j].id, b.files[j].flags), "{ctx}");
                            if after.format.has_compression_field() {
                                assert_eq!(after.files[j].uncompressed_size, Some(expected.len() as u64), "{ctx}");
                            }
                        }
                        // the offsets of the files before the changed one did not move, the later ones are consistent
                        for j in 0..i {
                            assert_eq!(after.files[j].data_offset, b.files[j].data_offset, "{ctx}");
                        }
                        // the headers (and the hash table) are byte-identical except the fields that had to change
                        let headers = b.headers_end as usize;
                        assert_eq!(after.headers_end, b.headers_end);
                        let changed = (0..headers).filter(|k| out[*k] != bytes[*k]).count();
                        assert!(changed <= 8 * 5, "{ctx}: {changed} header bytes changed");
                        // the last file ends the data, as before
                        let last = after.files.iter().rev().find(|f| f.stored_size > 0).unwrap();
                        assert_eq!(out.len() as u64, last.data_offset + last.stored_size, "{ctx}");
                        // replacing it back gives the original file
                        let back = replace_file(&out, &after, i, &data_of(i)).unwrap();
                        assert_eq!(back, bytes, "{ctx}: and back again");
                    }
                }
            }
        }
    }

    #[test]
    fn files_with_other_alignments_and_padded_ends_are_patched_the_same_way() {
        for (alignment, tail_padding) in [(0x10u64, true), (0x40, false), (0x40, true), (0x80, true), (4, false), (1, false)] {
            let mut spec = Bnd4Spec::new(0x74);
            spec.alignment = alignment;
            spec.tail_padding = tail_padding;
            for (id, name, data) in sample_files() {
                spec = spec.file(id, &name, &data);
            }
            let bytes = spec.build();
            let b = Bnd4::parse(&bytes).unwrap();
            assert!(b.layout.verified, "alignment {alignment:#x}: {:?}", b.layout.issues);
            // a layout that fits several alignments reports the first that does; patching must keep it valid either way
            for i in [0usize, 2, 4] {
                assert_eq!(replace_file(&bytes, &b, i, &data_of(i)).unwrap(), bytes, "alignment {alignment:#x} tail {tail_padding}");
                let new = vec![0xAB; 333];
                let out = replace_file(&bytes, &b, i, &new).unwrap();
                let after = Bnd4::parse(&out).unwrap();
                assert!(after.layout.verified);
                assert_eq!(after.file_bytes(&out, i), Some(&new[..]));
                if tail_padding {
                    assert_eq!(out.len() as u64 % after.layout.alignment, 0, "the padded end stays padded");
                }
            }
        }
        // an alignment that is not a power of two is not understood, and nothing is patched
        let mut odd = Bnd4Spec::new(0x74);
        odd.alignment = 24;
        for (id, name, data) in sample_files() {
            odd = odd.file(id, &name, &data);
        }
        let bytes = odd.build();
        let b = Bnd4::parse(&bytes).unwrap();
        assert!(!b.layout.verified && b.layout.alignment == 0, "{:?}", b.layout);
        assert!(!b.layout.issues.is_empty());
        assert!(matches!(replace_file(&bytes, &b, 1, b"new"), Err(Bnd4Error::LayoutNotVerified(_))));
    }

    #[test]
    fn a_layout_that_is_not_the_expected_one_is_reported_and_refused() {
        let good = sample(0x74, true);
        let b = Bnd4::parse(&good).unwrap();
        let second = b.files[1].data_offset as usize;
        // 1. non-zero padding between files (the first file is 37 bytes, so it is followed by padding)
        let mut x = good.clone();
        let first_end = (b.files[0].data_offset + b.files[0].stored_size) as usize;
        assert!(first_end < second);
        x[first_end] = 0xFF;
        let bx = Bnd4::parse(&x).unwrap();
        assert!(!bx.layout.verified && bx.layout.issues.iter().any(|i| i.contains("padding")), "{:?}", bx.layout);
        assert!(matches!(replace_file(&x, &bx, 0, b"zz"), Err(Bnd4Error::LayoutNotVerified(_))));
        // 2. a file stored further away than the rule says (header offset moved by 0x10, bytes appended)
        let mut y = good.clone();
        let hdr = HEADER_LEN + 2 * b.format.file_header_len();
        let off_field = hdr + 16 + 8;
        let moved = b.files[2].data_offset + 0x10;
        y[off_field..off_field + 4].copy_from_slice(&(moved as u32).to_le_bytes());
        y.extend([0u8; 0x20]);
        let by = Bnd4::parse(&y).unwrap();
        assert!(!by.layout.verified && by.layout.issues.iter().any(|i| i.contains("file 2")), "{:?}", by.layout);
        assert!(matches!(replace_file(&y, &by, 1, b"zz"), Err(Bnd4Error::LayoutNotVerified(_))));
        // 3. unexplained bytes at the end
        let mut z = good.clone();
        z.extend([1, 2, 3]);
        let bz = Bnd4::parse(&z).unwrap();
        assert!(!bz.layout.verified && bz.layout.issues.iter().any(|i| i.contains("follow the last file")));
        assert_eq!(bz.layout.tail_len, 3);
        // 4. files in another order than the headers
        let mut w = good.clone();
        let (o0, o1) = (b.files[0].data_offset, b.files[1].data_offset);
        let f0 = HEADER_LEN + 16 + 8;
        let f1 = HEADER_LEN + b.format.file_header_len() + 16 + 8;
        w[f0..f0 + 4].copy_from_slice(&(o1 as u32).to_le_bytes());
        w[f1..f1 + 4].copy_from_slice(&(o0 as u32).to_le_bytes());
        let bw = Bnd4::parse(&w).unwrap();
        assert!(!bw.layout.verified);
        // 5. the description of other bytes is not accepted
        assert!(matches!(replace_file(&z, &b, 1, b"zz"), Err(Bnd4Error::Malformed(_))));
        assert!(matches!(replace_file(&sample(0x74, false), &b, 1, b"zz"), Err(Bnd4Error::Malformed(_))));
    }

    #[test]
    fn some_files_cannot_be_replaced() {
        let bytes = sample(0x74, true);
        let b = Bnd4::parse(&bytes).unwrap();
        assert_eq!(replace_file(&bytes, &b, 5, b"zz"), Err(Bnd4Error::NoSuchFile { index: 5 }));
        assert_eq!(replace_file(&bytes, &b, 3, b"zz"), Err(Bnd4Error::EmptyFile { index: 3 }), "an empty file");
        assert_eq!(replace_file(&bytes, &b, 1, b""), Err(Bnd4Error::EmptyFile { index: 1 }), "nothing as the new data");
        // a file with its own compression flag
        let mut spec = Bnd4Spec::new(0x74);
        for (id, name, data) in sample_files() {
            spec = spec.file(id, &name, &data);
        }
        spec.files[1].flags = 0x03;
        let compressed = spec.build();
        let bc = Bnd4::parse(&compressed).unwrap();
        assert!(bc.files[1].is_compressed() && !bc.files[0].is_compressed());
        assert_eq!(replace_file(&compressed, &bc, 1, b"zz"), Err(Bnd4Error::CompressedEntry { index: 1 }));
        assert!(replace_file(&compressed, &bc, 0, b"zz").is_ok(), "the others can be replaced");
        // stored and uncompressed sizes that differ without a compression flag are not guessed at
        let mut odd = bytes.clone();
        let at = HEADER_LEN + b.format.file_header_len() + 16;
        odd[at..at + 8].copy_from_slice(&7i64.to_le_bytes());
        let bo = Bnd4::parse(&odd).unwrap();
        assert!(matches!(replace_file(&odd, &bo, 1, b"zz"), Err(Bnd4Error::LayoutNotVerified(_))));
    }

    #[test]
    fn what_is_not_a_little_endian_bnd4_is_refused() {
        let good = sample(0x74, true);
        assert_eq!(Bnd4::parse(b"").unwrap_err(), Bnd4Error::NotBnd4);
        assert_eq!(Bnd4::parse(b"BND3....").unwrap_err(), Bnd4Error::NotBnd4);
        assert!(matches!(Bnd4::parse(b"BND4"), Err(Bnd4Error::Truncated { .. })));
        let mut be = good.clone();
        be[9] = 1;
        assert_eq!(Bnd4::parse(&be).unwrap_err(), Bnd4Error::BigEndian);
        for (at, value) in [(9usize, 2u8), (10, 2), (6, 1), (7, 1), (8, 1), (11, 1), (0x30, 2), (0x32, 3), (0x33, 1), (0x34, 1)] {
            let mut x = good.clone();
            x[at] = value;
            assert!(matches!(Bnd4::parse(&x), Err(Bnd4Error::Malformed(_))), "byte {at:#x}");
        }
        // sizes and counts
        for (at, bytes) in [
            (0x0Cusize, (-1i32).to_le_bytes().to_vec()),
            (0x0C, 0x7FFF_FFFFi32.to_le_bytes().to_vec()),
            (0x0C, 100_000i32.to_le_bytes().to_vec()),
            (0x10, 0x41i64.to_le_bytes().to_vec()),
            (0x20, 0x29i64.to_le_bytes().to_vec()),
            (0x28, (-1i64).to_le_bytes().to_vec()),
            (0x28, i64::MAX.to_le_bytes().to_vec()),
            (0x38, 0i64.to_le_bytes().to_vec()),
            (0x38, 0x10i64.to_le_bytes().to_vec()),
            (0x38, i64::MAX.to_le_bytes().to_vec()),
        ] {
            let mut x = good.clone();
            x[at..at + bytes.len()].copy_from_slice(&bytes);
            assert!(Bnd4::parse(&x).is_err(), "field {at:#x} = {bytes:?}");
        }
        // a hash table offset without the extended byte, and the extended byte without an offset
        let no_table = sample(0x74, false);
        let mut x = no_table.clone();
        x[0x38] = 0x50;
        assert!(Bnd4::parse(&x).is_err());
        // file header fields
        let hdr = HEADER_LEN;
        for (rel, bytes) in [
            (1usize, vec![1u8]),                       // reserved zero bytes
            (4, vec![0, 0, 0, 0]),                     // the -1
            (8, (-1i64).to_le_bytes().to_vec()),       // negative size
            (8, i64::MAX.to_le_bytes().to_vec()),      // huge size
            (16, (-2i64).to_le_bytes().to_vec()),      // impossible uncompressed size
            (24, 0xFFFF_FFFFu32.to_le_bytes().to_vec()), // offset far outside
        ] {
            let mut x = good.clone();
            x[hdr + rel..hdr + rel + bytes.len()].copy_from_slice(&bytes);
            assert!(Bnd4::parse(&x).is_err(), "file header field {rel} = {bytes:?}");
        }
        // names: offset outside, and a name without its terminator
        let name_field = hdr + 16 + 8 + 4 + 4;
        let mut x = good.clone();
        x[name_field..name_field + 4].copy_from_slice(&0xFFFF_FF00u32.to_le_bytes());
        assert!(Bnd4::parse(&x).is_err());
        // a name that never ends: every 16-bit unit from the first name to the end of the file is non-zero
        let b = Bnd4::parse(&good).unwrap();
        let mut y = good.clone();
        let names_start = b.headers_end as usize - 8 * 5 - 16 - 8 * 2 - 100;
        for chunk in y[names_start..].chunks_exact_mut(2) {
            chunk.copy_from_slice(&[0x41, 0x00]);
        }
        let _ = Bnd4::parse(&y); // an error or not, but no panic and no endless loop
    }

    #[test]
    fn the_hash_table_is_checked_but_only_reported() {
        let mut bytes = sample(0x74, true);
        let b = Bnd4::parse(&bytes).unwrap();
        let t = b.hash_table.clone().unwrap();
        assert!(t.consistent && t.bucket_count >= 2 && t.offset % 8 == 0 && t.hashes_offset > t.offset);
        // damage the constants of the table: still parses, but is reported as not consistent
        let at = t.offset as usize + 12;
        bytes[at] = 0x11;
        let b2 = Bnd4::parse(&bytes).unwrap();
        assert!(!b2.hash_table.unwrap().consistent);
        // and replace_file does not touch the table
        assert!(replace_file(&bytes, &b, 1, b"x").is_err(), "the description is of other bytes now");
    }

    #[test]
    fn names_that_are_not_unicode_are_read_as_ascii() {
        let mut spec = Bnd4Spec::new(0x74);
        spec.unicode = false;
        spec.hash_table = false;
        let spec = spec.file(1, "plain.txt", b"abc").file(2, "other.txt", b"defgh");
        let bytes = spec.build();
        let b = Bnd4::parse(&bytes).unwrap();
        assert!(!b.unicode);
        assert_eq!(b.files[0].name.as_deref(), Some("plain.txt"));
        assert_eq!(b.files[1].name.as_deref(), Some("other.txt"));
        assert_eq!(replace_file(&bytes, &b, 0, b"abc").unwrap(), bytes);
        let longer = replace_file(&bytes, &b, 0, &[7u8; 50]).unwrap();
        assert_eq!(Bnd4::parse(&longer).unwrap().file_bytes(&longer, 1), Some(&b"defgh"[..]));
    }

    #[test]
    fn nothing_panics_on_damaged_input() {
        let mut seed = 0x1234_5678_9ABC_DEF0u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for raw in RAW_FORMATS {
            let good = sample(raw, true);
            let b = Bnd4::parse(&good).unwrap();
            // every truncation
            for cut in 0..good.len() {
                if let Ok(p) = Bnd4::parse(&good[..cut]) {
                    let _ = replace_file(&good[..cut], &p, 1, b"zz");
                }
            }
            // random damage: flip bytes, write wild numbers over header words
            for _ in 0..600 {
                let mut x = good.clone();
                for _ in 0..(1 + next() % 4) {
                    let at = (next() as usize) % b.headers_end as usize;
                    match next() % 3 {
                        0 => x[at] = next() as u8,
                        1 => x[at] ^= 1 << (next() % 8),
                        _ => {
                            let wild = [0u32, 1, 0x7FFF_FFFF, 0xFFFF_FFFF, 0x8000_0000, 0x10][(next() % 6) as usize].to_le_bytes();
                            for (k, w) in wild.iter().enumerate() {
                                if let Some(slot) = x.get_mut(at + k) {
                                    *slot = *w;
                                }
                            }
                        }
                    }
                }
                if let Ok(p) = Bnd4::parse(&x) {
                    for i in 0..p.files.len().min(6) {
                        let _ = p.file_bytes(&x, i);
                        let _ = replace_file(&x, &p, i, b"patched data!");
                    }
                }
            }
        }
    }
}
