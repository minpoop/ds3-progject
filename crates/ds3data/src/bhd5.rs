//! The `BHD5` header of a Dark Souls III archive (after the RSA layer is removed, see [`crate::rsa`]).
//!
//! Layout as documented by the Souls modding community (little endian, PC):
//!
//! ```text
//! 0x00 "BHD5"   0x04 i8 -1 (little endian)   0x05 u8 unknown   0x06 two zero bytes   0x08 i32 1
//! 0x0C i32 file size   0x10 i32 bucket count   0x14 i32 offset of the buckets   0x18 i32 salt length
//! 0x1C salt (ASCII, no padding)
//! at the buckets offset: per bucket  i32 file header count, i32 offset of the file headers
//! file header (40 bytes): u32 name hash, i32 padded size, i64 offset in the .bdt, i64 offset of the SHA record (0 = none),
//!                         i64 offset of the AES record (0 = none), i64 unpadded size
//! SHA record: 32 bytes hash, i32 range count, ranges (i64 start, i64 end)
//! AES record: 16 bytes key,  i32 range count, ranges (i64 start, i64 end)
//! ```
//!
//! Everything read here is checked against the size the header declares; a damaged header gives an error, never a panic
//! and never an allocation that depends on a number in the file.
use crate::util::{get, i32_le, i64_le, u32_le};
use std::fmt;

pub const FILE_HEADER_LEN: usize = 40;
const FIXED_LEN: usize = 0x1C;
const MAX_BUCKETS: usize = 1 << 22;
const MAX_ENTRIES: usize = 1 << 24;
const MAX_SALT: usize = 4096;
const MAX_RANGES: usize = 1 << 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bhd5Error {
    /// The data does not start with `BHD5` (the key was wrong, or this is not a header).
    NotBhd5,
    /// A big-endian header (console games); only the PC one is read.
    BigEndian,
    /// The header says it is larger than the data.
    Truncated { declared: u64, available: u64 },
    /// Something in the header does not add up.
    Malformed(&'static str),
}

impl fmt::Display for Bhd5Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bhd5Error::NotBhd5 => write!(f, "does not start with BHD5"),
            Bhd5Error::BigEndian => write!(f, "a big-endian header (not the PC format)"),
            Bhd5Error::Truncated { declared, available } => write!(f, "the header says it is {declared} bytes but only {available} are there"),
            Bhd5Error::Malformed(why) => write!(f, "damaged header: {why}"),
        }
    }
}

impl std::error::Error for Bhd5Error {}

/// One file of the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Hash of the file's path, see [`crate::hash::path_hash`].
    pub hash: u32,
    /// Bytes the file takes in the `.bdt` (this is what is read).
    pub padded_size: u32,
    /// Absolute offset of the file in the `.bdt`.
    pub offset: u64,
    /// The size after decryption as stored (`-1` or garbage when the archive does not say).
    pub unpadded_size: i64,
    /// Offset of the SHA record inside the header, 0 for none.
    pub sha_offset: u64,
    /// Offset of the AES record inside the header, 0 for none.
    pub aes_offset: u64,
    /// The bucket the header listed it in.
    pub bucket: u32,
}

impl Entry {
    /// The size the file has after decryption, if the header gives a usable one (`0..=padded_size`).
    pub fn unpadded(&self) -> Option<u64> {
        (0..=i64::from(self.padded_size)).contains(&self.unpadded_size).then_some(self.unpadded_size as u64)
    }

    pub fn is_encrypted(&self) -> bool {
        self.aes_offset != 0
    }
}

/// The AES key of a file and the byte ranges (relative to the file) that are encrypted with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AesRecord {
    pub key: [u8; 16],
    pub ranges: Vec<(i64, i64)>,
}

/// The salted SHA-256 hash of a file and the ranges it covers (not checked by this crate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaRecord {
    pub hash: [u8; 32],
    pub ranges: Vec<(i64, i64)>,
}

/// A parsed header.
#[derive(Clone)]
pub struct Bhd5 {
    data: Vec<u8>,
    pub unk05: u8,
    salt: Vec<u8>,
    bucket_count: usize,
    entries: Vec<Entry>,
}

impl fmt::Debug for Bhd5 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bhd5 {{ buckets: {}, files: {}, salt_len: {} }}", self.bucket_count, self.entries.len(), self.salt.len())
    }
}

/// `count` ranges at `at`: `(start, end)` pairs of i64.
fn ranges_at(data: &[u8], at: usize, count: usize) -> Result<Vec<(i64, i64)>, Bhd5Error> {
    let outside = Bhd5Error::Malformed("ranges outside the header");
    get(data, at, count.checked_mul(16).ok_or(Bhd5Error::Malformed("range count"))?).ok_or(outside.clone())?;
    let mut ranges = Vec::with_capacity(count);
    for i in 0..count {
        let base = at + i * 16;
        let (Some(start), Some(end)) = (i64_le(data, base), i64_le(data, base + 8)) else { return Err(outside) };
        ranges.push((start, end));
    }
    Ok(ranges)
}

/// A record of `fixed` bytes followed by an i32 count and `count` ranges: where it is, how many ranges.
fn record_at(data: &[u8], offset: u64, fixed: usize) -> Result<(usize, usize), Bhd5Error> {
    let at = usize::try_from(offset).map_err(|_| Bhd5Error::Malformed("a record offset is out of range"))?;
    if at < FIXED_LEN {
        return Err(Bhd5Error::Malformed("a record offset points into the fixed header"));
    }
    let count = i32_le(data, at.checked_add(fixed).ok_or(Bhd5Error::Malformed("a record offset is out of range"))?).ok_or(Bhd5Error::Malformed("a record is cut off"))?;
    let count = usize::try_from(count).ok().filter(|c| *c <= MAX_RANGES).ok_or(Bhd5Error::Malformed("a record has an impossible range count"))?;
    let end = at + fixed + 4 + count * 16;
    if end > data.len() {
        return Err(Bhd5Error::Malformed("a record is cut off"));
    }
    Ok((at, count))
}

impl Bhd5 {
    /// Parses a decrypted header. `plain` may be longer than the header (the last RSA block is padded); the size the
    /// header declares decides.
    pub fn parse(mut plain: Vec<u8>) -> Result<Bhd5, Bhd5Error> {
        if !plain.starts_with(b"BHD5") {
            return Err(Bhd5Error::NotBhd5);
        }
        if plain.len() < FIXED_LEN {
            return Err(Bhd5Error::Truncated { declared: FIXED_LEN as u64, available: plain.len() as u64 });
        }
        let (Some(&endian), Some(&unk05), Some(&zero1), Some(&zero2)) = (plain.get(4), plain.get(5), plain.get(6), plain.get(7)) else {
            return Err(Bhd5Error::Truncated { declared: FIXED_LEN as u64, available: plain.len() as u64 });
        };
        if endian == 0 {
            return Err(Bhd5Error::BigEndian);
        }
        if endian != 0xFF {
            return Err(Bhd5Error::Malformed("the byte order mark is neither 0 nor -1"));
        }
        if zero1 != 0 || zero2 != 0 || i32_le(&plain, 8) != Some(1) {
            return Err(Bhd5Error::Malformed("the fixed fields of the header are wrong"));
        }
        let declared = i32_le(&plain, 0x0C).unwrap_or(-1);
        let declared = usize::try_from(declared).ok().filter(|d| *d >= FIXED_LEN).ok_or(Bhd5Error::Malformed("the declared size is impossible"))?;
        if declared > plain.len() {
            return Err(Bhd5Error::Truncated { declared: declared as u64, available: plain.len() as u64 });
        }
        plain.truncate(declared);
        plain.shrink_to_fit();
        let data = plain;

        let bucket_count = i32_le(&data, 0x10).and_then(|v| usize::try_from(v).ok()).filter(|c| *c <= MAX_BUCKETS).ok_or(Bhd5Error::Malformed("the bucket count is impossible"))?;
        let buckets_offset = i32_le(&data, 0x14).and_then(|v| usize::try_from(v).ok()).ok_or(Bhd5Error::Malformed("the bucket table offset is impossible"))?;
        let salt_len = i32_le(&data, 0x18).and_then(|v| usize::try_from(v).ok()).filter(|l| *l <= MAX_SALT).ok_or(Bhd5Error::Malformed("the salt length is impossible"))?;
        let salt = get(&data, FIXED_LEN, salt_len).ok_or(Bhd5Error::Malformed("the salt is cut off"))?.to_vec();
        if buckets_offset < FIXED_LEN + salt_len {
            return Err(Bhd5Error::Malformed("the bucket table overlaps the header"));
        }
        let table_len = bucket_count * 8;
        if buckets_offset.checked_add(table_len).is_none_or(|end| end > data.len()) {
            return Err(Bhd5Error::Malformed("the bucket table is cut off"));
        }

        let mut entries: Vec<Entry> = Vec::new();
        for bucket in 0..bucket_count {
            let at = buckets_offset + bucket * 8;
            let count = i32_le(&data, at).and_then(|v| usize::try_from(v).ok()).ok_or(Bhd5Error::Malformed("a bucket has an impossible file count"))?;
            let headers = i32_le(&data, at + 4).and_then(|v| usize::try_from(v).ok()).ok_or(Bhd5Error::Malformed("a bucket has an impossible offset"))?;
            if count == 0 {
                continue;
            }
            if entries.len() + count > MAX_ENTRIES {
                return Err(Bhd5Error::Malformed("too many files"));
            }
            let span = count.checked_mul(FILE_HEADER_LEN).ok_or(Bhd5Error::Malformed("too many files"))?;
            if headers < FIXED_LEN || headers.checked_add(span).is_none_or(|end| end > data.len()) {
                return Err(Bhd5Error::Malformed("a bucket's file headers are outside the header"));
            }
            entries.reserve(count);
            for i in 0..count {
                let p = headers + i * FILE_HEADER_LEN;
                let (Some(hash), Some(padded), Some(offset), Some(sha), Some(aes), Some(unpadded)) =
                    (u32_le(&data, p), i32_le(&data, p + 4), i64_le(&data, p + 8), i64_le(&data, p + 16), i64_le(&data, p + 24), i64_le(&data, p + 32))
                else {
                    return Err(Bhd5Error::Malformed("a file header is cut off"));
                };
                let padded_size = u32::try_from(padded).map_err(|_| Bhd5Error::Malformed("a file has a negative size"))?;
                let offset = u64::try_from(offset).map_err(|_| Bhd5Error::Malformed("a file has a negative offset"))?;
                let sha_offset = u64::try_from(sha).map_err(|_| Bhd5Error::Malformed("a negative record offset"))?;
                let aes_offset = u64::try_from(aes).map_err(|_| Bhd5Error::Malformed("a negative record offset"))?;
                if sha_offset != 0 {
                    record_at(&data, sha_offset, 32)?;
                }
                if aes_offset != 0 {
                    record_at(&data, aes_offset, 16)?;
                }
                entries.push(Entry { hash, padded_size, offset, unpadded_size: unpadded, sha_offset, aes_offset, bucket: bucket as u32 });
            }
        }
        Ok(Bhd5 { data, unk05, salt, bucket_count, entries })
    }

    /// The header numbers of a decrypted header, for a report when it does not parse: structure only, no file contents.
    pub fn describe(plain: &[u8]) -> String {
        let num = |v: Option<i32>| v.map_or("n/a".to_string(), |v| v.to_string());
        format!(
            "{} bytes; byte order mark {}; unknown byte {}; fixed words {} {} {}; declared size {}; buckets {}; bucket table at {}; salt length {}",
            plain.len(),
            plain.get(4).map_or("n/a".to_string(), |b| format!("{b:#04x}")),
            plain.get(5).map_or("n/a".to_string(), |b| b.to_string()),
            plain.get(6).map_or("n/a".to_string(), |b| b.to_string()),
            plain.get(7).map_or("n/a".to_string(), |b| b.to_string()),
            num(i32_le(plain, 8)),
            num(i32_le(plain, 0x0C)),
            num(i32_le(plain, 0x10)),
            num(i32_le(plain, 0x14)),
            num(i32_le(plain, 0x18))
        )
    }

    /// The salt (ASCII) the SHA hashes of the files are made with.
    pub fn salt(&self) -> &[u8] {
        &self.salt
    }

    pub fn bucket_count(&self) -> usize {
        self.bucket_count
    }

    /// Every file of every bucket, in the order of the header.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The size of the header.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The AES record of an entry of this header, if it has one.
    pub fn aes_record(&self, entry: &Entry) -> Result<Option<AesRecord>, Bhd5Error> {
        if entry.aes_offset == 0 {
            return Ok(None);
        }
        let (at, count) = record_at(&self.data, entry.aes_offset, 16)?;
        let key: [u8; 16] = get(&self.data, at, 16).and_then(|k| k.try_into().ok()).ok_or(Bhd5Error::Malformed("an AES record is cut off"))?;
        Ok(Some(AesRecord { key, ranges: ranges_at(&self.data, at + 20, count)? }))
    }

    /// The SHA record of an entry of this header, if it has one.
    pub fn sha_record(&self, entry: &Entry) -> Result<Option<ShaRecord>, Bhd5Error> {
        if entry.sha_offset == 0 {
            return Ok(None);
        }
        let (at, count) = record_at(&self.data, entry.sha_offset, 32)?;
        let hash: [u8; 32] = get(&self.data, at, 32).and_then(|k| k.try_into().ok()).ok_or(Bhd5Error::Malformed("a SHA record is cut off"))?;
        Ok(Some(ShaRecord { hash, ranges: ranges_at(&self.data, at + 36, count)? }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::archive::{Bhd5Spec, FileSpec};

    fn sample() -> Vec<u8> {
        Bhd5Spec {
            salt: "salty".to_string(),
            buckets: 5,
            files: vec![
                FileSpec::plain(0x0102_0304, 100, 0x10),
                FileSpec::plain(0x0102_0304, 48, 0x90), // same hash: a collision
                FileSpec::plain(77, 16, 0x100).unpadded(-1),
                FileSpec::plain(9, 80, 0x200).unpadded(75).aes([7; 16], &[(0, 32), (48, 64), (-1, -1), (16, 16)]).sha([5; 32], &[(0, 80)]),
            ],
        }
        .build()
    }

    #[test]
    fn parses_what_the_builder_wrote() {
        let h = Bhd5::parse(sample()).unwrap();
        assert_eq!((h.bucket_count(), h.entries().len(), h.salt()), (5, 4, &b"salty"[..]));
        let e = h.entries();
        let mut hashes: Vec<u32> = e.iter().map(|e| e.hash).collect();
        hashes.sort();
        assert_eq!(hashes, vec![9, 77, 0x0102_0304, 0x0102_0304]);
        let last = e.iter().find(|e| e.hash == 9).unwrap();
        assert_eq!((last.padded_size, last.offset, last.unpadded()), (80, 0x200, Some(75)));
        assert!(last.is_encrypted());
        let aes = h.aes_record(last).unwrap().unwrap();
        assert_eq!(aes.key, [7; 16]);
        assert_eq!(aes.ranges, vec![(0, 32), (48, 64), (-1, -1), (16, 16)]);
        let sha = h.sha_record(last).unwrap().unwrap();
        assert_eq!((sha.hash, sha.ranges), ([5; 32], vec![(0, 80)]));
        let none = e.iter().find(|e| e.hash == 77).unwrap();
        assert_eq!((none.unpadded(), none.is_encrypted(), h.aes_record(none).unwrap(), h.sha_record(none).unwrap()), (None, false, None, None));
        // the colliding pair is in the same bucket
        let pair: Vec<&Entry> = e.iter().filter(|e| e.hash == 0x0102_0304).collect();
        assert_eq!(pair.len(), 2);
        assert_eq!(pair[0].bucket, pair[1].bucket);
        assert_ne!(pair[0].offset, pair[1].offset);
    }

    #[test]
    fn a_header_that_is_longer_than_it_declares_is_cut_to_its_size() {
        let mut bytes = sample();
        let declared = bytes.len();
        bytes.extend([0xAA; 300]);
        let h = Bhd5::parse(bytes).unwrap();
        assert_eq!(h.len(), declared);
    }

    #[test]
    fn unpadded_sizes_are_only_trusted_inside_range() {
        let mk = |u: i64| Entry { hash: 0, padded_size: 100, offset: 0, unpadded_size: u, sha_offset: 0, aes_offset: 0, bucket: 0 };
        assert_eq!(mk(0).unpadded(), Some(0));
        assert_eq!(mk(100).unpadded(), Some(100));
        assert_eq!((mk(101).unpadded(), mk(-1).unpadded(), mk(i64::MIN).unpadded(), mk(i64::MAX).unpadded()), (None, None, None, None));
    }

    #[test]
    fn rejects_what_is_not_a_header() {
        assert_eq!(Bhd5::parse(vec![]).unwrap_err(), Bhd5Error::NotBhd5);
        assert_eq!(Bhd5::parse(b"BHF4....".to_vec()).unwrap_err(), Bhd5Error::NotBhd5);
        assert!(matches!(Bhd5::parse(b"BHD5".to_vec()), Err(Bhd5Error::Truncated { .. })));
        let good = sample();
        let mut be = good.clone();
        be[4] = 0;
        assert_eq!(Bhd5::parse(be).unwrap_err(), Bhd5Error::BigEndian);
        for (at, value) in [(4usize, 7u8), (6, 1), (7, 1), (8, 2)] {
            let mut x = good.clone();
            x[at] = value;
            assert!(matches!(Bhd5::parse(x), Err(Bhd5Error::Malformed(_))), "byte {at}");
        }
        // declared size larger than the data
        let mut x = good.clone();
        x[0x0C..0x10].copy_from_slice(&(good.len() as i32 + 1).to_le_bytes());
        assert!(matches!(Bhd5::parse(x), Err(Bhd5Error::Truncated { .. })));
        // impossible numbers in every i32 of the fixed header
        for at in [0x0Cusize, 0x10, 0x14, 0x18] {
            for value in [-1i32, i32::MIN, i32::MAX, 0x7FFF_0000] {
                let mut x = good.clone();
                x[at..at + 4].copy_from_slice(&value.to_le_bytes());
                // may or may not be an error, but never a panic
                let _ = Bhd5::parse(x);
            }
        }
    }

    #[test]
    fn rejects_records_and_headers_outside_the_data() {
        let good = Bhd5::parse(sample()).unwrap();
        let enc = good.entries().iter().find(|e| e.hash == 9).unwrap();
        // point the AES record far away, into the fixed header and at a negative offset
        for bad in [u64::MAX, 1 << 40, 4u64, good.len() as u64 - 3] {
            let mut x = sample();
            patch_entry(&mut x, 9, 24, bad as i64);
            assert!(matches!(Bhd5::parse(x), Err(Bhd5Error::Malformed(_))), "aes offset {bad}");
        }
        let mut x = sample();
        patch_entry(&mut x, 9, 24, -5);
        assert!(Bhd5::parse(x).is_err());
        // a huge range count
        let mut x = sample();
        x[enc.aes_offset as usize + 16..enc.aes_offset as usize + 20].copy_from_slice(&0x7FFF_FFFFi32.to_le_bytes());
        assert!(Bhd5::parse(x).is_err());
        let mut x = sample();
        x[enc.aes_offset as usize + 16..enc.aes_offset as usize + 20].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(Bhd5::parse(x).is_err());
        // a negative size and a negative offset
        let mut x = sample();
        patch_entry(&mut x, 77, 4, -1);
        assert!(Bhd5::parse(x).is_err());
        let mut x = sample();
        patch_entry(&mut x, 77, 8, -16);
        assert!(Bhd5::parse(x).is_err());
    }

    /// Overwrite 8 bytes (or 4 for the size) of the file header of the entry with `hash`, field starting at `field`.
    fn patch_entry(bytes: &mut [u8], hash: u32, field: usize, value: i64) {
        let h = Bhd5::parse(bytes.to_vec()).unwrap();
        // find the header position by scanning for the hash at an aligned file header position
        let buckets_offset = i32_le(bytes, 0x14).unwrap() as usize;
        for b in 0..h.bucket_count() {
            let count = i32_le(bytes, buckets_offset + b * 8).unwrap() as usize;
            let headers = i32_le(bytes, buckets_offset + b * 8 + 4).unwrap() as usize;
            for i in 0..count {
                let p = headers + i * FILE_HEADER_LEN;
                if u32_le(bytes, p) == Some(hash) {
                    if field == 4 {
                        bytes[p + 4..p + 8].copy_from_slice(&(value as i32).to_le_bytes());
                    } else {
                        bytes[p + field..p + field + 8].copy_from_slice(&value.to_le_bytes());
                    }
                    return;
                }
            }
        }
        panic!("no entry with hash {hash}");
    }

    #[test]
    fn a_bucket_count_larger_than_the_data_does_not_allocate_wildly() {
        let mut x = sample();
        x[0x10..0x14].copy_from_slice(&((1i32 << 22) - 1).to_le_bytes());
        assert!(matches!(Bhd5::parse(x), Err(Bhd5Error::Malformed(_))));
        let mut y = sample();
        let off = i32_le(&y, 0x14).unwrap() as usize;
        y[off..off + 4].copy_from_slice(&0x7FFF_FFFFi32.to_le_bytes()); // a bucket claiming 2 billion files
        assert!(Bhd5::parse(y).is_err());
    }
}
