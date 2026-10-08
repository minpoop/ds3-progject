//! One archive of the game: a `.bhd` header (RSA layer, then [`crate::bhd5`]) and the `.bdt` that holds the files.
//!
//! Both files are only ever opened for reading. The header is decrypted once when the archive is opened (it is a few
//! megabytes); a file is read with one seek and one read of its `padded size` bytes from the `.bdt`, decrypted with its AES
//! key in the ranges the header lists, and cut to its unpadded size. The `.bdt` starts with a small `BDF4` header; the
//! offsets in the `.bhd` are absolute.
use crate::bhd5::{Bhd5, Bhd5Error, Entry};
use crate::keys::{io_reason, RsaPublicKey};
use crate::rsa::{decrypt_complete_blocks, first_block_starts_with, RsaError};
use aes::cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit};
use aes::Aes128;
use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A `.bhd` bigger than this is not read.
pub const MAX_BHD: u64 = 512 << 20;
/// The most `read` will read for one file.
pub const MAX_FILE: u64 = 1 << 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    /// A file could not be opened or read (`what` says which; no paths in messages).
    Io { what: &'static str, reason: String },
    /// None of the keys decrypts the header to something that starts with `BHD5`.
    NoKeyMatched { tried: usize },
    /// The `.bhd` is smaller than one encrypted block.
    HeaderTooShort,
    /// The file is bigger than this program will read.
    TooBig { what: &'static str, size: u64 },
    Rsa(RsaError),
    /// The header decrypted but does not parse; `summary` has its structural numbers (no file contents).
    Header { error: Bhd5Error, summary: String },
    /// A file lies (partly) outside the `.bdt`.
    OutOfRange { offset: u64, size: u32, bdt_size: u64 },
    /// An AES range of a file is not usable.
    BadRange { start: i64, end: i64, size: u32 },
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveError::Io { what, reason } => write!(f, "cannot read the {what}: {reason}"),
            ArchiveError::NoKeyMatched { tried } => write!(f, "no key matched ({tried} tried)"),
            ArchiveError::HeaderTooShort => write!(f, "the .bhd is too short to hold an encrypted header"),
            ArchiveError::TooBig { what, size } => write!(f, "the {what} is too big to read ({size} bytes)"),
            ArchiveError::Rsa(e) => write!(f, "{e}"),
            ArchiveError::Header { error, summary } => write!(f, "{error} [{summary}]"),
            ArchiveError::OutOfRange { offset, size, bdt_size } => write!(f, "the file ({size} bytes at {offset}) lies outside the .bdt ({bdt_size} bytes)"),
            ArchiveError::BadRange { start, end, size } => write!(f, "an encrypted range ({start}..{end}) does not fit the file ({size} bytes)"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<RsaError> for ArchiveError {
    fn from(e: RsaError) -> Self {
        ArchiveError::Rsa(e)
    }
}

/// An opened archive.
pub struct Archive {
    name: String,
    bdt_path: PathBuf,
    bhd_size: u64,
    bdt_size: u64,
    bdt_magic: [u8; 4],
    key_fingerprint: String,
    trailing: usize,
    outside: usize,
    header: Bhd5,
    /// (hash, index into the header's entries), sorted
    index: Vec<(u32, u32)>,
}

impl fmt::Debug for Archive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Archive {{ {}, key {}, {} files }}", self.name, self.key_fingerprint, self.header.entries().len())
    }
}

fn read_all(path: &Path, what: &'static str, max: u64) -> Result<Vec<u8>, ArchiveError> {
    let meta = std::fs::metadata(path).map_err(|e| ArchiveError::Io { what, reason: io_reason(&e) })?;
    if meta.len() > max {
        return Err(ArchiveError::TooBig { what, size: meta.len() });
    }
    std::fs::read(path).map_err(|e| ArchiveError::Io { what, reason: io_reason(&e) })
}

impl Archive {
    /// Opens `<bhd>` with its `<bdt>`: finds which of `keys` decrypts the header (the candidates are tried on the first
    /// block only), decrypts and parses it. Nothing is written anywhere.
    pub fn open(bhd: &Path, bdt: &Path, keys: &[RsaPublicKey]) -> Result<Archive, ArchiveError> {
        let bytes = read_all(bhd, ".bhd file", MAX_BHD)?;
        if !keys.is_empty() && keys.iter().all(|k| bytes.len() < k.modulus_len()) {
            return Err(ArchiveError::HeaderTooShort);
        }
        let key = keys.iter().find(|k| first_block_starts_with(k, &bytes, b"BHD5")).ok_or(ArchiveError::NoKeyMatched { tried: keys.len() })?;
        let name = bhd.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        Archive::from_header_bytes(name, &bytes, key, bdt)
    }

    /// Like [`Archive::open`] with the key known and the `.bhd` bytes already read.
    pub fn from_header_bytes(name: String, bhd_bytes: &[u8], key: &RsaPublicKey, bdt: &Path) -> Result<Archive, ArchiveError> {
        let (plain, trailing) = decrypt_complete_blocks(key, bhd_bytes)?;
        let summary = Bhd5::describe(&plain);
        let header = Bhd5::parse(plain).map_err(|error| ArchiveError::Header { error, summary })?;
        let meta = std::fs::metadata(bdt).map_err(|e| ArchiveError::Io { what: ".bdt file", reason: io_reason(&e) })?;
        let mut bdt_magic = [0u8; 4];
        if let Ok(mut f) = std::fs::File::open(bdt) {
            let _ = f.read_exact(&mut bdt_magic);
        }
        let bdt_size = meta.len();
        let mut index: Vec<(u32, u32)> = header.entries().iter().enumerate().map(|(i, e)| (e.hash, i as u32)).collect();
        index.sort_unstable();
        let outside = header.entries().iter().filter(|e| e.offset.checked_add(u64::from(e.padded_size)).is_none_or(|end| end > bdt_size)).count();
        Ok(Archive {
            name,
            bdt_path: bdt.to_path_buf(),
            bhd_size: bhd_bytes.len() as u64,
            bdt_size,
            bdt_magic,
            key_fingerprint: key.fingerprint(),
            trailing,
            outside,
            header,
            index,
        })
    }

    /// The archive's name, e.g. `Data0` (the file name of the `.bhd` without its extension).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Fingerprint of the key that decrypts the header.
    pub fn key_fingerprint(&self) -> &str {
        &self.key_fingerprint
    }

    pub fn header(&self) -> &Bhd5 {
        &self.header
    }

    pub fn entries(&self) -> &[Entry] {
        self.header.entries()
    }

    pub fn bhd_size(&self) -> u64 {
        self.bhd_size
    }

    pub fn bdt_size(&self) -> u64 {
        self.bdt_size
    }

    /// The first four bytes of the `.bdt` (`BDF4` in the game's archives).
    pub fn bdt_magic(&self) -> [u8; 4] {
        self.bdt_magic
    }

    /// Bytes at the end of the `.bhd` that did not make up a whole encrypted block (0 normally).
    pub fn trailing_bhd_bytes(&self) -> usize {
        self.trailing
    }

    /// How many files lie (partly) outside the `.bdt`; 0 for an intact archive.
    pub fn files_outside_bdt(&self) -> usize {
        self.outside
    }

    /// Every file whose path hashes to `hash` (the 32-bit hash is not unique: look at what you get). The whole file is
    /// searched, not just the bucket `hash % buckets` would suggest.
    pub fn find(&self, hash: u32) -> impl Iterator<Item = &Entry> + '_ {
        let start = self.index.partition_point(|(h, _)| *h < hash);
        self.index[start..].iter().take_while(move |(h, _)| *h == hash).filter_map(|(_, i)| self.header.entries().get(*i as usize))
    }

    /// The file's bytes: read from the `.bdt`, decrypted, cut to the unpadded size. At most [`MAX_FILE`] bytes.
    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>, ArchiveError> {
        self.read_limited(entry, MAX_FILE)
    }

    /// Like [`Archive::read`] but refuses files whose stored size is over `max` bytes.
    pub fn read_limited(&self, entry: &Entry, max: u64) -> Result<Vec<u8>, ArchiveError> {
        let size = u64::from(entry.padded_size);
        if size > max {
            return Err(ArchiveError::TooBig { what: "file", size });
        }
        match entry.offset.checked_add(size) {
            Some(end) if end <= self.bdt_size => {}
            _ => return Err(ArchiveError::OutOfRange { offset: entry.offset, size: entry.padded_size, bdt_size: self.bdt_size }),
        }
        let aes = self.header.aes_record(entry).map_err(|error| ArchiveError::Header { error, summary: format!("the AES record of the file at {}", entry.offset) })?;
        let mut file = std::fs::File::open(&self.bdt_path).map_err(|e| ArchiveError::Io { what: ".bdt file", reason: io_reason(&e) })?;
        file.seek(SeekFrom::Start(entry.offset)).map_err(|e| ArchiveError::Io { what: ".bdt file", reason: io_reason(&e) })?;
        let mut bytes = vec![0u8; entry.padded_size as usize];
        file.read_exact(&mut bytes).map_err(|e| ArchiveError::Io { what: ".bdt file", reason: io_reason(&e) })?;
        if let Some(aes) = aes {
            let cipher = Aes128::new(GenericArray::from_slice(&aes.key));
            for (start, end) in aes.ranges {
                if start == -1 || end == -1 || start == end {
                    continue;
                }
                let range = usize::try_from(start).ok().zip(usize::try_from(end).ok()).filter(|(s, e)| s < e && (e - s) % 16 == 0);
                let part = range.and_then(|(s, e)| bytes.get_mut(s..e));
                let Some(part) = part else { return Err(ArchiveError::BadRange { start, end, size: entry.padded_size }) };
                for block in part.chunks_exact_mut(16) {
                    cipher.decrypt_block(GenericArray::from_mut_slice(block));
                }
            }
        }
        if let Some(n) = entry.unpadded() {
            bytes.truncate(n as usize);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::path_hash;
    use crate::testing::archive::{ArchiveBuilder, Bhd5Spec, FileSpec};
    use crate::testing::keys::test_key;

    fn sample(dir: &Path) -> Vec<(&'static str, Vec<u8>)> {
        let files: Vec<(&'static str, Vec<u8>)> = vec![
            ("/msg/ENGLISH/item.msgbnd.dcx", (0..200u32).map(|i| (i * 3) as u8).collect()),
            ("/regulation.bin", b"plain regulation bytes".to_vec()),
            ("/parts/wp_a_0200.partsbnd.dcx", (0..75u32).map(|i| (i + 1) as u8).collect()),
            ("/empty.bin", Vec::new()),
        ];
        let mut b = ArchiveBuilder::new(7);
        b.add(files[0].0, &files[0].1);
        b.add(files[1].0, &files[1].1);
        b.add_encrypted(files[2].0, &files[2].1, [3; 16], &[(0, 32), (48, 64), (-1, -1), (16, 16)]);
        b.add(files[3].0, &files[3].1);
        b.add_hashed(path_hash(files[1].0), b"a different file with the same hash");
        b.write(dir, "Data0", &test_key(0));
        files
    }

    #[test]
    fn opens_finds_and_reads_plain_and_encrypted_files() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        let files = sample(dir);
        let a = Archive::open(&dir.join("Data0.bhd"), &dir.join("Data0.bdt"), &[test_key(1).public, test_key(0).public]).unwrap();
        assert_eq!((a.name(), a.key_fingerprint()), ("Data0", "87febfc8"));
        assert_eq!(a.entries().len(), 5);
        assert_eq!(&a.bdt_magic(), b"BDF4");
        assert_eq!((a.trailing_bhd_bytes(), a.files_outside_bdt()), (0, 0));
        assert_eq!(a.header().salt(), b"FRPGHDRSALT");
        for (path, data) in &files {
            let hits: Vec<&Entry> = a.find(path_hash(path)).collect();
            assert!(!hits.is_empty(), "{path}");
            let got: Vec<Vec<u8>> = hits.iter().map(|e| a.read(e).unwrap()).collect();
            assert!(got.contains(data), "{path}: {got:?}");
        }
        // the encrypted file really is stored encrypted, and the unpadded size cuts the padding off
        let enc = a.find(path_hash(files[2].0)).next().unwrap();
        assert_eq!((enc.padded_size, enc.unpadded()), (80, Some(75)));
        let raw = std::fs::read(dir.join("Data0.bdt")).unwrap();
        assert_ne!(&raw[enc.offset as usize..enc.offset as usize + 32], &files[2].1[..32]);
        assert_eq!(&raw[enc.offset as usize + 64..enc.offset as usize + 75], &files[2].1[64..75], "ranges not listed stay as they were");
        assert_eq!(a.read(enc).unwrap(), files[2].1);
        // two files share a hash: both are found
        assert_eq!(a.find(path_hash(files[1].0)).count(), 2);
        assert_eq!(a.find(path_hash("/nothing/here")).count(), 0);
        // every entry can be found by its own hash (the index is complete)
        for e in a.entries() {
            assert!(a.find(e.hash).any(|x| x == e));
        }
    }

    #[test]
    fn the_right_key_is_found_among_wrong_ones_and_a_missing_key_is_reported() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        sample(dir);
        let (bhd, bdt) = (dir.join("Data0.bhd"), dir.join("Data0.bdt"));
        assert!(Archive::open(&bhd, &bdt, &[test_key(0).public]).is_ok());
        assert_eq!(Archive::open(&bhd, &bdt, &[test_key(1).public]).unwrap_err(), ArchiveError::NoKeyMatched { tried: 1 });
        assert_eq!(Archive::open(&bhd, &bdt, &[]).unwrap_err(), ArchiveError::NoKeyMatched { tried: 0 });
    }

    #[test]
    fn missing_and_damaged_files_are_errors() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        sample(dir);
        let key = test_key(0).public;
        let keys = std::slice::from_ref(&key);
        assert!(matches!(Archive::open(&dir.join("None.bhd"), &dir.join("None.bdt"), keys), Err(ArchiveError::Io { .. })));
        assert!(matches!(Archive::open(&dir.join("Data0.bhd"), &dir.join("None.bdt"), keys), Err(ArchiveError::Io { .. })));
        let msg = Archive::open(&dir.join("None.bhd"), &dir.join("None.bdt"), keys).unwrap_err().to_string();
        assert!(!msg.contains(dir.to_string_lossy().as_ref()), "{msg}");
        // a .bhd shorter than a block
        std::fs::write(dir.join("Short.bhd"), [1u8; 100]).unwrap();
        std::fs::write(dir.join("Short.bdt"), b"BDF4").unwrap();
        assert_eq!(Archive::open(&dir.join("Short.bhd"), &dir.join("Short.bdt"), keys).unwrap_err(), ArchiveError::HeaderTooShort);
        // a truncated .bdt: files beyond its end are counted and refused
        let bdt = std::fs::read(dir.join("Data0.bdt")).unwrap();
        std::fs::write(dir.join("Cut.bhd"), std::fs::read(dir.join("Data0.bhd")).unwrap()).unwrap();
        std::fs::write(dir.join("Cut.bdt"), &bdt[..bdt.len() / 2]).unwrap();
        let cut = Archive::open(&dir.join("Cut.bhd"), &dir.join("Cut.bdt"), &[key]).unwrap();
        assert!(cut.files_outside_bdt() > 0);
        let outside: Vec<&Entry> = cut.entries().iter().filter(|e| e.offset + u64::from(e.padded_size) > cut.bdt_size()).collect();
        assert!(!outside.is_empty());
        assert!(matches!(cut.read(outside[0]), Err(ArchiveError::OutOfRange { .. })));
    }

    #[test]
    fn a_partial_block_at_the_end_of_the_bhd_is_tolerated_only_when_the_header_does_not_need_it() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        sample(dir);
        let key = test_key(0).public;
        let bhd = std::fs::read(dir.join("Data0.bhd")).unwrap();
        let mut longer = bhd.clone();
        longer.extend([0xEE; 17]);
        std::fs::write(dir.join("Tail.bhd"), &longer).unwrap();
        std::fs::copy(dir.join("Data0.bdt"), dir.join("Tail.bdt")).unwrap();
        let a = Archive::open(&dir.join("Tail.bhd"), &dir.join("Tail.bdt"), std::slice::from_ref(&key)).unwrap();
        assert_eq!(a.trailing_bhd_bytes(), 17);
        // cut into the last block: the header needs it
        std::fs::write(dir.join("Cut.bhd"), &bhd[..bhd.len() - 10]).unwrap();
        std::fs::copy(dir.join("Data0.bdt"), dir.join("Cut.bdt")).unwrap();
        let err = Archive::open(&dir.join("Cut.bhd"), &dir.join("Cut.bdt"), &[key]).unwrap_err();
        assert!(matches!(&err, ArchiveError::Header { error: Bhd5Error::Truncated { .. }, .. }), "{err}");
        assert!(err.to_string().contains("declared size"), "the message carries the header numbers: {err}");
    }

    #[test]
    fn bad_aes_ranges_are_refused_not_applied() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        let key = test_key(0);
        let mut b = ArchiveBuilder::new(3);
        b.add_encrypted("/x.bin", &[7u8; 64], [9; 16], &[(0, 64)]);
        b.write(dir, "A", &key);
        let a = Archive::open(&dir.join("A.bhd"), &dir.join("A.bdt"), std::slice::from_ref(&key.public)).unwrap();
        assert_eq!(a.read(&a.entries()[0]).unwrap(), vec![7u8; 64]);
        // headers whose ranges do not fit a 64-byte file (or are not whole blocks)
        let mut bdt = b"BDF4".to_vec();
        bdt.resize(16, 0);
        bdt.extend([7u8; 64]);
        std::fs::write(dir.join("B.bdt"), &bdt).unwrap();
        for ranges in [vec![(0i64, 80i64)], vec![(8, 20)], vec![(32, 16)], vec![(-5, 16)], vec![(0, 16), (60, 64)], vec![(i64::MAX - 8, i64::MAX)]] {
            let spec = Bhd5Spec { salt: "s".into(), buckets: 3, files: vec![FileSpec::plain(path_hash("/x.bin"), 64, 16).aes([1; 16], &ranges)] };
            std::fs::write(dir.join("B.bhd"), key.encrypt_header(&spec.build())).unwrap();
            let a = Archive::open(&dir.join("B.bhd"), &dir.join("B.bdt"), std::slice::from_ref(&key.public)).unwrap();
            assert!(matches!(a.read(&a.entries()[0]), Err(ArchiveError::BadRange { .. })), "{ranges:?}");
        }
    }

    #[test]
    fn size_limits() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        sample(dir);
        let a = Archive::open(&dir.join("Data0.bhd"), &dir.join("Data0.bdt"), &[test_key(0).public]).unwrap();
        let e = a.find(path_hash("/msg/ENGLISH/item.msgbnd.dcx")).next().unwrap();
        assert!(a.read_limited(e, 100).is_err());
        assert_eq!(a.read_limited(e, 200).unwrap().len(), 200);
    }
}
