//! Builders for synthetic archives: a plain `BHD5` header, and a whole `.bhd`/`.bdt` pair (header encrypted with a test
//! key, files stored in the `.bdt`, some of them AES-encrypted). Written from the layout facts in [`crate::bhd5`].
use super::keys::TestKey;
use crate::hash::path_hash;
use aes::cipher::{generic_array::GenericArray, BlockEncrypt, KeyInit};
use aes::Aes128;
use std::path::Path;

/// One file header of a synthetic `BHD5`.
#[derive(Clone, Debug)]
pub struct FileSpec {
    pub hash: u32,
    pub padded: u32,
    pub offset: u64,
    pub unpadded: i64,
    pub aes: Option<([u8; 16], Vec<(i64, i64)>)>,
    pub sha: Option<([u8; 32], Vec<(i64, i64)>)>,
}

impl FileSpec {
    pub fn plain(hash: u32, padded: u32, offset: u64) -> FileSpec {
        FileSpec { hash, padded, offset, unpadded: i64::from(padded), aes: None, sha: None }
    }

    pub fn unpadded(mut self, v: i64) -> FileSpec {
        self.unpadded = v;
        self
    }

    pub fn aes(mut self, key: [u8; 16], ranges: &[(i64, i64)]) -> FileSpec {
        self.aes = Some((key, ranges.to_vec()));
        self
    }

    pub fn sha(mut self, hash: [u8; 32], ranges: &[(i64, i64)]) -> FileSpec {
        self.sha = Some((hash, ranges.to_vec()));
        self
    }
}

/// A synthetic `BHD5` header (plain, before the RSA layer).
#[derive(Clone, Debug)]
pub struct Bhd5Spec {
    pub salt: String,
    pub buckets: usize,
    pub files: Vec<FileSpec>,
}

fn put_i32(out: &mut [u8], at: usize, v: i32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_i64(out: &mut [u8], at: usize, v: i64) {
    out[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

impl Bhd5Spec {
    /// The header bytes: fixed part and salt, the bucket table, the file headers bucket by bucket, then for every file its
    /// SHA record and AES record. A file goes into bucket `hash % buckets`.
    pub fn build(&self) -> Vec<u8> {
        assert!(self.buckets >= 1);
        let salt = self.salt.as_bytes();
        let mut out = vec![0u8; 0x1C];
        out[..4].copy_from_slice(b"BHD5");
        out[4] = 0xFF;
        put_i32(&mut out, 8, 1);
        put_i32(&mut out, 0x10, self.buckets as i32);
        put_i32(&mut out, 0x14, (0x1C + salt.len()) as i32);
        put_i32(&mut out, 0x18, salt.len() as i32);
        out.extend_from_slice(salt);
        let table = out.len();
        out.resize(table + self.buckets * 8, 0);

        let mut by_bucket: Vec<Vec<&FileSpec>> = vec![Vec::new(); self.buckets];
        for f in &self.files {
            by_bucket[f.hash as usize % self.buckets].push(f);
        }
        // file headers; the record offsets are filled in below
        let mut header_pos: Vec<(usize, &FileSpec)> = Vec::new();
        for (b, files) in by_bucket.iter().enumerate() {
            let here = out.len() as i32;
            put_i32(&mut out, table + b * 8, files.len() as i32);
            put_i32(&mut out, table + b * 8 + 4, here);
            for f in files {
                let at = out.len();
                out.resize(at + 40, 0);
                out[at..at + 4].copy_from_slice(&f.hash.to_le_bytes());
                put_i32(&mut out, at + 4, f.padded as i32);
                put_i64(&mut out, at + 8, f.offset as i64);
                put_i64(&mut out, at + 32, f.unpadded);
                header_pos.push((at, f));
            }
        }
        for (at, f) in header_pos {
            if let Some((hash, ranges)) = &f.sha {
                let here = out.len() as i64;
                put_i64(&mut out, at + 16, here);
                out.extend_from_slice(hash);
                out.extend_from_slice(&(ranges.len() as i32).to_le_bytes());
                for (s, e) in ranges {
                    out.extend_from_slice(&s.to_le_bytes());
                    out.extend_from_slice(&e.to_le_bytes());
                }
            }
            if let Some((key, ranges)) = &f.aes {
                let here = out.len() as i64;
                put_i64(&mut out, at + 24, here);
                out.extend_from_slice(key);
                out.extend_from_slice(&(ranges.len() as i32).to_le_bytes());
                for (s, e) in ranges {
                    out.extend_from_slice(&s.to_le_bytes());
                    out.extend_from_slice(&e.to_le_bytes());
                }
            }
        }
        let size = out.len() as i32;
        put_i32(&mut out, 0x0C, size);
        out
    }
}

/// AES-128-ECB encryption of `ranges` of `data` in place (the same ranges the reader decrypts).
pub fn aes_encrypt_ranges(data: &mut [u8], key: &[u8; 16], ranges: &[(i64, i64)]) {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    for &(s, e) in ranges {
        if s == -1 || e == -1 || s == e {
            continue;
        }
        for block in data[s as usize..e as usize].chunks_exact_mut(16) {
            cipher.encrypt_block(GenericArray::from_mut_slice(block));
        }
    }
}

struct Stored {
    hash: u32,
    stored: Vec<u8>,
    unpadded: i64,
    aes: Option<([u8; 16], Vec<(i64, i64)>)>,
    sha: bool,
}

/// A whole archive: files in, `.bhd` and `.bdt` bytes out.
pub struct ArchiveBuilder {
    buckets: usize,
    salt: String,
    files: Vec<Stored>,
}

impl ArchiveBuilder {
    pub fn new(buckets: usize) -> ArchiveBuilder {
        ArchiveBuilder { buckets, salt: "FRPGHDRSALT".to_string(), files: Vec::new() }
    }

    /// A file stored as it is, found under the hash of `path`.
    pub fn add(&mut self, path: &str, data: &[u8]) -> &mut Self {
        self.add_hashed(path_hash(path), data)
    }

    /// A file stored as it is under a chosen hash (to make collisions).
    pub fn add_hashed(&mut self, hash: u32, data: &[u8]) -> &mut Self {
        self.files.push(Stored { hash, stored: data.to_vec(), unpadded: data.len() as i64, aes: None, sha: true });
        self
    }

    /// A file whose stored bytes are padded with zeros to a multiple of 16 and encrypted in the given ranges (the
    /// ranges are relative to the stored bytes and must be multiples of 16 long).
    pub fn add_encrypted(&mut self, path: &str, data: &[u8], key: [u8; 16], ranges: &[(i64, i64)]) -> &mut Self {
        let mut stored = data.to_vec();
        stored.resize(data.len().div_ceil(16) * 16, 0);
        aes_encrypt_ranges(&mut stored, &key, ranges);
        self.files.push(Stored { hash: path_hash(path), stored, unpadded: data.len() as i64, aes: Some((key, ranges.to_vec())), sha: true });
        self
    }

    /// `(bhd, bdt)` bytes. The `.bdt` starts with a 16-byte `BDF4` header, files follow at multiples of 16.
    pub fn build(&self, key: &TestKey) -> (Vec<u8>, Vec<u8>) {
        let mut bdt = vec![0u8; 16];
        bdt[..4].copy_from_slice(b"BDF4");
        bdt[8] = 1;
        let mut specs = Vec::new();
        for f in &self.files {
            while bdt.len() % 16 != 0 {
                bdt.push(0);
            }
            let offset = bdt.len() as u64;
            bdt.extend_from_slice(&f.stored);
            let mut spec = FileSpec::plain(f.hash, f.stored.len() as u32, offset).unpadded(f.unpadded);
            if f.sha {
                spec = spec.sha([0x5A; 32], &[(0, f.stored.len() as i64)]);
            }
            if let Some((k, ranges)) = &f.aes {
                spec = spec.aes(*k, ranges);
            }
            specs.push(spec);
        }
        let plain = Bhd5Spec { salt: self.salt.clone(), buckets: self.buckets, files: specs }.build();
        (key.encrypt_header(&plain), bdt)
    }

    /// Writes `<dir>/<name>.bhd` and `<dir>/<name>.bdt`.
    pub fn write(&self, dir: &Path, name: &str, key: &TestKey) {
        let (bhd, bdt) = self.build(key);
        std::fs::create_dir_all(dir).expect("create the archive folder");
        std::fs::write(dir.join(format!("{name}.bhd")), bhd).expect("write the .bhd");
        std::fs::write(dir.join(format!("{name}.bdt")), bdt).expect("write the .bdt");
    }
}
