//! `DCX`: the compression wrapper around most of Dark Souls III's files (`item.msgbnd.dcx`, `wp_a_0200.partsbnd.dcx`, ...).
//!
//! Layout as documented by the Souls modding community (all numbers big endian), for the deflate variant `DFLT`:
//!
//! ```text
//! 0x00 "DCX\0"   0x04 u32 0x10000 or 0x11000   0x08 u32 0x18   0x0C u32 0x24   0x10 u32 0x24 or 0x44
//! 0x14 u32 0x2C (if 0x10 is 0x24) or 0x4C      0x18 "DCS\0"    0x1C u32 uncompressed size   0x20 u32 compressed size
//! 0x24 "DCP\0"   0x28 "DFLT"   0x2C u32 0x20   0x30 u8 level (8 or 9), 3 zero bytes   0x34 u32 0
//! 0x38 u8 0 or 15, 3 zero bytes   0x3C u32 0   0x40 u32 0x00010100   0x44 "DCA\0"   0x48 u32 8
//! 0x4C the zlib stream (0x78 0xDA ... then the big-endian Adler-32), `compressed size` bytes
//! ```
//!
//! Decoding checks every constant of the header, the length of the result against the declared size and the Adler-32.
//! The other variants (`KRAK` and `ZSTD` of later games, `EDGE`) are named in an [`DcxError::Unsupported`] error.
//! [`encode`] writes the same header again (new sizes) in front of a fresh zlib stream, so a file that was decoded can be
//! written back; the compressed bytes differ from the original encoder's but decode to the same data.
use crate::util::{get, u32_be};
use std::borrow::Cow;
use std::fmt;

/// The largest result `decode` will make (the declared size decides, nothing is allocated from the compressed data's size).
pub const MAX_DECODED: u64 = 1 << 30;
const HEADER_LEN: usize = 0x4C;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DcxError {
    /// The data does not start with `DCX\0`.
    NotDcx,
    /// The data ends too early.
    Truncated { what: &'static str },
    /// A constant of the header has another value than in the files this reader knows.
    BadHeader { at: usize, found: u32 },
    /// A compression variant this crate does not read (`KRAK`, `ZSTD`, `EDGE`, ...).
    Unsupported { variant: String },
    /// The declared size is over the limit.
    TooLarge { declared: u64 },
    /// The deflate data is damaged.
    Inflate(&'static str),
    /// The data inside is shorter than the header says.
    SizeMismatch { declared: u64, actual: u64 },
    /// The compressed data holds more than the header says.
    TooMuchData { declared: u64 },
    /// The Adler-32 at the end of the stream does not match the data.
    ChecksumMismatch { stored: u32, computed: u32 },
}

impl fmt::Display for DcxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DcxError::NotDcx => write!(f, "not a DCX file"),
            DcxError::Truncated { what } => write!(f, "the DCX file is cut off ({what})"),
            DcxError::BadHeader { at, found } => write!(f, "unexpected value {found:#x} in the DCX header at offset {at:#x}"),
            DcxError::Unsupported { variant } => write!(f, "unsupported DCX compression \"{variant}\""),
            DcxError::TooLarge { declared } => write!(f, "the DCX file says it holds {declared} bytes, which is too much"),
            DcxError::Inflate(why) => write!(f, "the compressed data is damaged ({why})"),
            DcxError::SizeMismatch { declared, actual } => write!(f, "the DCX header says {declared} bytes but the data has {actual}"),
            DcxError::TooMuchData { declared } => write!(f, "the DCX header says {declared} bytes but the data has more"),
            DcxError::ChecksumMismatch { stored, computed } => write!(f, "the checksum of the data is wrong (stored {stored:#010x}, computed {computed:#010x})"),
        }
    }
}

impl std::error::Error for DcxError {}

/// What a DCX file's header said, so that [`encode`] can write an equivalent one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DcxInfo {
    /// The word at 0x04 (`0x10000` or `0x11000`).
    pub unk04: u32,
    /// The word at 0x10 (`0x24` or `0x44`); the word at 0x14 follows from it.
    pub unk10: u32,
    /// The compression level byte at 0x30 (8 or 9).
    pub level: u8,
    /// The byte at 0x38 (0 or 15).
    pub unk38: u8,
    /// Sizes the header declared: uncompressed, compressed (the zlib stream) - informational.
    pub declared_uncompressed: u32,
    pub declared_compressed: u32,
    /// Bytes after the zlib stream in the file that was read (not rewritten).
    pub trailing: usize,
}

impl DcxInfo {
    /// The variant Dark Souls III's own files use (`DCX_DFLT_10000_44_9`).
    pub fn ds3_default() -> DcxInfo {
        DcxInfo { unk04: 0x10000, unk10: 0x44, level: 9, unk38: 0, declared_uncompressed: 0, declared_compressed: 0, trailing: 0 }
    }

    /// The word at 0x14.
    fn unk14(&self) -> u32 {
        if self.unk10 == 0x24 {
            0x2C
        } else {
            0x4C
        }
    }

    /// The variant's name as the community writes it, e.g. `DCX_DFLT_10000_44_9`.
    pub fn variant_name(&self) -> String {
        match (self.unk04, self.unk10, self.level, self.unk38) {
            (0x10000, 0x24, 9, 0) => "DCX_DFLT_10000_24_9".to_string(),
            (0x10000, 0x44, 9, 0) => "DCX_DFLT_10000_44_9".to_string(),
            (0x11000, 0x44, 8, 0) => "DCX_DFLT_11000_44_8".to_string(),
            (0x11000, 0x44, 9, 0) => "DCX_DFLT_11000_44_9".to_string(),
            (0x11000, 0x44, 9, 15) => "DCX_DFLT_11000_44_9_15".to_string(),
            (a, b, c, d) => format!("DCX_DFLT_{a:X}_{b:X}_{c}_{d}"),
        }
    }
}

/// Does this look like a DCX file (`DCX\0`, or the older `DCP\0` start)?
pub fn is_dcx(bytes: &[u8]) -> bool {
    bytes.starts_with(b"DCX\0") || bytes.starts_with(b"DCP\0")
}

/// The Adler-32 checksum (RFC 1950).
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for byte in chunk {
            a += u32::from(*byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

fn printable(bytes: &[u8]) -> String {
    bytes.iter().map(|b| if b.is_ascii_graphic() { *b as char } else { '?' }).collect()
}

/// Decodes a DCX file: the decoded bytes and what the header said.
pub fn decode(bytes: &[u8]) -> Result<(Vec<u8>, DcxInfo), DcxError> {
    decode_limited(bytes, MAX_DECODED)
}

/// Like [`decode`], refusing files that declare more than `max_out` bytes.
pub fn decode_limited(bytes: &[u8], max_out: u64) -> Result<(Vec<u8>, DcxInfo), DcxError> {
    if bytes.starts_with(b"DCP\0") {
        return Err(DcxError::Unsupported { variant: "DCP header without DCX".to_string() });
    }
    if !bytes.starts_with(b"DCX\0") {
        return Err(DcxError::NotDcx);
    }
    let truncated_header = DcxError::Truncated { what: "header" };
    let method = get(bytes, 0x28, 4).ok_or(truncated_header.clone())?;
    if method != b"DFLT" {
        return Err(DcxError::Unsupported { variant: printable(method) });
    }
    // all the offsets below are inside this array, so nothing here can run past the data
    let header: &[u8; HEADER_LEN] = bytes.get(..HEADER_LEN).and_then(|h| h.try_into().ok()).ok_or(truncated_header)?;
    let word = |at: usize| u32_be(header, at).unwrap_or(0);
    let bad = |at: usize| DcxError::BadHeader { at, found: word(at) };
    let (unk04, unk10, level, unk38) = (word(0x04), word(0x10), header[0x30], header[0x38]);
    if unk04 != 0x10000 && unk04 != 0x11000 {
        return Err(bad(0x04));
    }
    if word(0x08) != 0x18 {
        return Err(bad(0x08));
    }
    if word(0x0C) != 0x24 {
        return Err(bad(0x0C));
    }
    if unk10 != 0x24 && unk10 != 0x44 {
        return Err(bad(0x10));
    }
    if word(0x14) != if unk10 == 0x24 { 0x2C } else { 0x4C } {
        return Err(bad(0x14));
    }
    if &header[0x18..0x1C] != b"DCS\0" {
        return Err(bad(0x18));
    }
    if &header[0x24..0x28] != b"DCP\0" {
        return Err(bad(0x24));
    }
    if word(0x2C) != 0x20 {
        return Err(bad(0x2C));
    }
    if !(level == 8 || level == 9) || header[0x31..0x34] != [0, 0, 0] {
        return Err(bad(0x30));
    }
    if word(0x34) != 0 {
        return Err(bad(0x34));
    }
    if !(unk38 == 0 || unk38 == 15) || header[0x39..0x3C] != [0, 0, 0] {
        return Err(bad(0x38));
    }
    if word(0x3C) != 0 {
        return Err(bad(0x3C));
    }
    if word(0x40) != 0x0001_0100 {
        return Err(bad(0x40));
    }
    if &header[0x44..0x48] != b"DCA\0" {
        return Err(bad(0x44));
    }
    if word(0x48) != 8 {
        return Err(bad(0x48));
    }

    let uncompressed = word(0x1C);
    let compressed = word(0x20);
    if u64::from(uncompressed) > max_out {
        return Err(DcxError::TooLarge { declared: u64::from(uncompressed) });
    }
    if compressed < 6 {
        return Err(DcxError::Truncated { what: "zlib stream" });
    }
    let stream = get(bytes, HEADER_LEN, compressed as usize).ok_or(DcxError::Truncated { what: "compressed data" })?;
    let trailing = bytes.len().saturating_sub(HEADER_LEN).saturating_sub(compressed as usize);
    // zlib: CMF = deflate with a window of at most 32 KiB, a multiple of 31 with FLG, no preset dictionary
    let (Some(&cmf), Some(&flg)) = (stream.first(), stream.get(1)) else { return Err(DcxError::Truncated { what: "zlib stream" }) };
    if cmf & 0x0F != 8 || cmf >> 4 > 7 || (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0 {
        return Err(DcxError::Inflate("not a zlib stream"));
    }
    if flg & 0x20 != 0 {
        return Err(DcxError::Inflate("the zlib stream wants a preset dictionary"));
    }
    let adler_at = stream.len().saturating_sub(4);
    let body = stream.get(2..adler_at).ok_or(DcxError::Truncated { what: "zlib stream" })?;
    let stored_adler = u32_be(stream, adler_at).ok_or(DcxError::Truncated { what: "zlib stream" })?;
    let out = miniz_oxide::inflate::decompress_to_vec_with_limit(body, uncompressed as usize).map_err(|e| match e.status {
        miniz_oxide::inflate::TINFLStatus::HasMoreOutput => DcxError::TooMuchData { declared: u64::from(uncompressed) },
        miniz_oxide::inflate::TINFLStatus::NeedsMoreInput | miniz_oxide::inflate::TINFLStatus::FailedCannotMakeProgress => DcxError::Inflate("the compressed data ends early"),
        _ => DcxError::Inflate("invalid compressed data"),
    })?;
    if out.len() as u64 != u64::from(uncompressed) {
        return Err(DcxError::SizeMismatch { declared: u64::from(uncompressed), actual: out.len() as u64 });
    }
    let computed = adler32(&out);
    if computed != stored_adler {
        return Err(DcxError::ChecksumMismatch { stored: stored_adler, computed });
    }
    Ok((out, DcxInfo { unk04, unk10, level, unk38, declared_uncompressed: uncompressed, declared_compressed: compressed, trailing }))
}

/// For input that may or may not be wrapped: DCX is decoded, anything else is returned as it is with `None`.
pub fn decode_if_dcx(bytes: &[u8]) -> Result<(Cow<'_, [u8]>, Option<DcxInfo>), DcxError> {
    if is_dcx(bytes) {
        let (out, info) = decode(bytes)?;
        Ok((Cow::Owned(out), Some(info)))
    } else {
        Ok((Cow::Borrowed(bytes), None))
    }
}

/// Wraps `data` in a DCX file with the header values of `info` (the sizes are the new ones) and a fresh zlib stream
/// (header `78 DA`, DEFLATE at the highest level, big-endian Adler-32).
pub fn encode(data: &[u8], info: &DcxInfo) -> Result<Vec<u8>, DcxError> {
    let uncompressed = u32::try_from(data.len()).map_err(|_| DcxError::TooLarge { declared: data.len() as u64 })?;
    let raw = miniz_oxide::deflate::compress_to_vec(data, 9);
    let mut stream = Vec::with_capacity(raw.len() + 6);
    stream.extend_from_slice(&[0x78, 0xDA]);
    stream.extend_from_slice(&raw);
    stream.extend_from_slice(&adler32(data).to_be_bytes());
    let compressed = u32::try_from(stream.len()).map_err(|_| DcxError::TooLarge { declared: stream.len() as u64 })?;

    let mut out = Vec::with_capacity(HEADER_LEN + stream.len());
    let mut word = |v: u32| out.extend_from_slice(&v.to_be_bytes());
    out_magic(&mut word, b"DCX\0");
    word(info.unk04);
    word(0x18);
    word(0x24);
    word(info.unk10);
    word(info.unk14());
    out_magic(&mut word, b"DCS\0");
    word(uncompressed);
    word(compressed);
    out_magic(&mut word, b"DCP\0");
    out_magic(&mut word, b"DFLT");
    word(0x20);
    word(u32::from(info.level) << 24);
    word(0);
    word(u32::from(info.unk38) << 24);
    word(0);
    word(0x0001_0100);
    out_magic(&mut word, b"DCA\0");
    word(8);
    out.extend_from_slice(&stream);
    Ok(out)
}

fn out_magic(word: &mut impl FnMut(u32), magic: &[u8; 4]) {
    word(u32::from_be_bytes(*magic));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(len: usize) -> Vec<u8> {
        // compressible, but not trivially so
        (0..len).map(|i| ((i * i / 7) % 251) as u8 ^ (i / 300) as u8).collect()
    }

    #[test]
    fn adler32_matches_known_values() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"a"), 0x0062_0062);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        // longer than one 5552 block, with values that would overflow without the reduction
        assert_eq!(adler32(&vec![0xFF; 100_000]), {
            let (mut a, mut b) = (1u64, 0u64);
            for _ in 0..100_000 {
                a = (a + 255) % 65521;
                b = (b + a) % 65521;
            }
            ((b << 16) | a) as u32
        });
    }

    #[test]
    fn round_trips_many_sizes_and_keeps_every_header_value() {
        for len in [0usize, 1, 2, 15, 16, 17, 255, 256, 4095, 4096, 65535, 65536, 70_000, 300_000] {
            let data = sample(len);
            for info in [
                DcxInfo::ds3_default(),
                DcxInfo { unk04: 0x10000, unk10: 0x24, level: 9, unk38: 0, ..DcxInfo::ds3_default() },
                DcxInfo { unk04: 0x11000, unk10: 0x44, level: 8, unk38: 0, ..DcxInfo::ds3_default() },
                DcxInfo { unk04: 0x11000, unk10: 0x44, level: 9, unk38: 15, ..DcxInfo::ds3_default() },
            ] {
                let bytes = encode(&data, &info).unwrap();
                assert!(is_dcx(&bytes));
                let (back, got) = decode(&bytes).unwrap();
                assert_eq!(back, data, "length {len}");
                assert_eq!((got.unk04, got.unk10, got.level, got.unk38), (info.unk04, info.unk10, info.level, info.unk38));
                assert_eq!(got.declared_uncompressed as usize, len);
                assert_eq!(got.declared_compressed as usize + HEADER_LEN, bytes.len());
                assert_eq!(got.trailing, 0);
                // writing it again with what was read gives a file that reads the same
                let again = encode(&back, &got).unwrap();
                assert_eq!(decode(&again).unwrap().0, data);
                assert_eq!(again, bytes, "the same encoder gives the same bytes");
            }
        }
    }

    #[test]
    fn the_header_has_exactly_the_documented_bytes() {
        let bytes = encode(b"hello hello hello hello", &DcxInfo::ds3_default()).unwrap();
        let mut expected = Vec::new();
        for w in [0x4443_5800u32, 0x10000, 0x18, 0x24, 0x44, 0x4C, 0x4443_5300, 23, (bytes.len() - 0x4C) as u32, 0x4443_5000, 0x4446_4C54, 0x20, 0x0900_0000, 0, 0, 0, 0x0001_0100, 0x4443_4100, 8] {
            expected.extend(w.to_be_bytes());
        }
        assert_eq!(&bytes[..0x4C], &expected[..]);
        assert_eq!(&bytes[0x4C..0x4E], &[0x78, 0xDA], "zlib header for the highest level");
        assert_eq!(&bytes[bytes.len() - 4..], &adler32(b"hello hello hello hello").to_be_bytes());
        assert_eq!(DcxInfo::ds3_default().variant_name(), "DCX_DFLT_10000_44_9");
        let other = DcxInfo { unk04: 0x11000, unk10: 0x24, level: 8, unk38: 15, ..DcxInfo::ds3_default() };
        assert_eq!(other.variant_name(), "DCX_DFLT_11000_24_8_15");
        let bytes = encode(b"x", &DcxInfo { unk10: 0x24, ..DcxInfo::ds3_default() }).unwrap();
        assert_eq!(&bytes[0x10..0x18], &[0, 0, 0, 0x24, 0, 0, 0, 0x2C], "the word at 0x14 follows from the one at 0x10");
    }

    #[test]
    fn reads_streams_made_by_other_encoders_and_trailing_bytes() {
        let data = sample(10_000);
        let header = |compressed: usize| {
            let mut h = encode(&data, &DcxInfo::ds3_default()).unwrap();
            h.truncate(HEADER_LEN);
            h[0x20..0x24].copy_from_slice(&(compressed as u32).to_be_bytes());
            h
        };
        for level in [0u8, 1, 6, 10] {
            let mut stream = vec![0x78, if level >= 7 { 0xDA } else { 0x9C }];
            stream.extend(miniz_oxide::deflate::compress_to_vec(&data, level));
            stream.extend(adler32(&data).to_be_bytes());
            let mut file = header(stream.len());
            file.extend(&stream);
            assert_eq!(decode(&file).unwrap().0, data, "level {level}");
            file.extend([0u8; 13]);
            let (back, info) = decode(&file).unwrap();
            assert_eq!((back, info.trailing), (data.clone(), 13), "trailing bytes are allowed and counted");
        }
    }

    #[test]
    fn other_variants_are_named() {
        let good = encode(b"abc", &DcxInfo::ds3_default()).unwrap();
        for variant in ["KRAK", "ZSTD", "EDGE", "LZMA"] {
            let mut x = good.clone();
            x[0x28..0x2C].copy_from_slice(variant.as_bytes());
            assert_eq!(decode(&x).unwrap_err(), DcxError::Unsupported { variant: variant.to_string() });
        }
        let mut weird = good.clone();
        weird[0x28..0x2C].copy_from_slice(&[0, 1, b'A', 0xFF]);
        assert_eq!(decode(&weird).unwrap_err(), DcxError::Unsupported { variant: "??A?".to_string() });
        assert_eq!(decode(b"DCP\0DFLT").unwrap_err(), DcxError::Unsupported { variant: "DCP header without DCX".to_string() });
        assert!(DcxError::Unsupported { variant: "KRAK".into() }.to_string().contains("\"KRAK\""));
    }

    #[test]
    fn passes_other_data_through() {
        assert!(!is_dcx(b"BND4...."));
        assert!(!is_dcx(b""));
        assert!(!is_dcx(b"DCX"));
        assert_eq!(decode(b"BND4").unwrap_err(), DcxError::NotDcx);
        assert_eq!(decode(b"").unwrap_err(), DcxError::NotDcx);
        let (plain, info) = decode_if_dcx(b"BND4 stuff").unwrap();
        assert_eq!((&plain[..], info), (&b"BND4 stuff"[..], None));
        assert!(matches!(plain, Cow::Borrowed(_)));
        let wrapped = encode(b"inside", &DcxInfo::ds3_default()).unwrap();
        let (inside, info) = decode_if_dcx(&wrapped).unwrap();
        assert_eq!((&inside[..], info.map(|i| i.unk04)), (&b"inside"[..], Some(0x10000)));
    }

    #[test]
    fn every_damage_is_an_error() {
        let data = sample(2000);
        let good = encode(&data, &DcxInfo::ds3_default()).unwrap();
        // cut at every length
        for cut in 0..good.len() {
            assert!(decode(&good[..cut]).is_err(), "cut at {cut}");
        }
        // every header byte matters
        for at in 0..HEADER_LEN {
            if matches!(at, 0x1C..=0x23) {
                continue; // the sizes are tested below
            }
            let mut x = good.clone();
            x[at] ^= 0x01;
            if at == 0x30 {
                assert_eq!(decode(&x).unwrap().1.level, 8, "level 9 with one bit flipped is the other known level");
            } else {
                assert!(decode(&x).is_err(), "flipped byte {at:#x}");
            }
        }
        // declared sizes: smaller, larger, huge
        for declared in [0u32, 1999, 2001, u32::MAX] {
            let mut x = good.clone();
            x[0x1C..0x20].copy_from_slice(&declared.to_be_bytes());
            assert!(decode(&x).is_err(), "declared {declared}");
        }
        let mut huge = good.clone();
        huge[0x1C..0x20].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(decode(&huge), Err(DcxError::TooLarge { .. })));
        assert!(matches!(decode_limited(&good, 1999), Err(DcxError::TooLarge { .. })));
        assert!(decode_limited(&good, 2000).is_ok());
        let mut x = good.clone();
        x[0x20..0x24].copy_from_slice(&5u32.to_be_bytes());
        assert!(decode(&x).is_err());
        let mut x = good.clone();
        x[0x20..0x24].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(decode(&x), Err(DcxError::Truncated { .. })));
        // damaged data: the checksum or the stream notices
        let mut x = good.clone();
        let last = x.len() - 1;
        x[last] ^= 0x80;
        assert!(matches!(decode(&x), Err(DcxError::ChecksumMismatch { .. })));
        for at in (HEADER_LEN..good.len()).step_by(7) {
            let mut x = good.clone();
            x[at] ^= 0x55;
            assert!(decode(&x).is_err(), "flipped data byte {at}");
        }
        // a stream that is not zlib, or wants a dictionary
        let mut x = good.clone();
        x[0x4C] = 0x79;
        assert!(matches!(decode(&x), Err(DcxError::Inflate(_))));
        let mut x = good.clone();
        x[0x4D] = 0xFA; // FDICT set, still a multiple of 31 with 0x78
        assert!(matches!(decode(&x), Err(DcxError::Inflate(_))));
    }

    #[test]
    fn more_output_than_declared_is_caught_without_allocating_it() {
        // a 1 MB run of zeros deflates to about 1 KB; the header claims 100 bytes
        let big = vec![0u8; 1 << 20];
        let mut x = encode(&big, &DcxInfo::ds3_default()).unwrap();
        x[0x1C..0x20].copy_from_slice(&100u32.to_be_bytes());
        assert_eq!(decode(&x).unwrap_err(), DcxError::TooMuchData { declared: 100 });
    }
}
