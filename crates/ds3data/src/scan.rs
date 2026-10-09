//! Looking for what the game's archives need inside raw bytes: the memory of the running game, or its program file.
//!
//! Dark Souls III keeps the table of contents of every archive (`Data0.bhd` ...) encrypted on disk. The game must read it
//! though, so while it runs the numbers exist somewhere in its memory. This module only *recognises* them in bytes it is
//! given (the caller reads the memory); it never touches a process or a file itself.
//!
//! * [`Scanner::keys`] - RSA public keys in the shapes programs keep them: PEM text (also as UTF-16), bare base64, DER
//!   `RSAPublicKey` / `SubjectPublicKeyInfo`, Windows CNG and CryptoAPI key blobs
//! * [`Scanner::bignums`] - big-number structures (OpenSSL `BIGNUM`, mbed TLS `mpi`) that point at a 2048-bit number; a
//!   *candidate* modulus (read it at the pointer, then see if it opens a real archive: [`modulus_key`])
//! * [`Scanner::headers`] and [`check_image`] - a decrypted `BHD5` table of contents lying in memory, and the proof that
//!   it is one (it parses, every file lies inside the `.bdt`) and which archive it belongs to ([`pairs_with`])
//! * [`Scanner::entry_hits`] - where the 32-bit hash of a known path is followed by a plausible size and offset (only
//!   used to learn how the game stores its table when no whole table was found)
//!
//! Everything is bounds-checked: any bytes may be passed in and nothing panics.
use crate::bhd5::Bhd5;
use crate::keys::{scan_pem, RsaPublicKey, MAX_PEM_BLOCK};
use crate::util::{get, i32_le, i64_le, u32_le};
use memchr::memmem::Finder;
use num_bigint::BigUint;

/// One encrypted block of an archive header (the modulus length of the 2048-bit archive keys).
pub const BLOCK: usize = 256;

/// How a key was written where it was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyForm {
    /// `-----BEGIN RSA PUBLIC KEY-----` text.
    Pem,
    /// The same as UTF-16 text.
    PemWide,
    /// Base64 of a DER `RSAPublicKey` without the PEM lines.
    Base64,
    /// DER `RSAPublicKey`.
    Pkcs1Der,
    /// DER `SubjectPublicKeyInfo`.
    SpkiDer,
    /// Windows CNG `BCRYPT_RSAKEY_BLOB` (`RSA1`, big-endian numbers).
    CngBlob,
    /// CryptoAPI `RSAPUBKEY` (`RSA1`, little-endian modulus).
    CapiBlob,
}

impl KeyForm {
    pub fn name(self) -> &'static str {
        match self {
            KeyForm::Pem => "PEM text",
            KeyForm::PemWide => "PEM text (UTF-16)",
            KeyForm::Base64 => "bare base64",
            KeyForm::Pkcs1Der => "DER RSAPublicKey",
            KeyForm::SpkiDer => "DER SubjectPublicKeyInfo",
            KeyForm::CngBlob => "CNG key blob",
            KeyForm::CapiBlob => "CryptoAPI key blob",
        }
    }
}

/// A public key found in some bytes.
#[derive(Clone, Debug)]
pub struct FoundKey {
    pub form: KeyForm,
    /// Where in the scanned bytes it starts.
    pub offset: usize,
    pub key: RsaPublicKey,
}

/// What [`Scanner::keys`] found in one piece of data.
#[derive(Clone, Debug, Default)]
pub struct KeyScan {
    pub keys: Vec<FoundKey>,
    /// `-----BEGIN ... PUBLIC KEY-----` blocks that were looked at (ASCII and UTF-16), and how many were not usable keys.
    pub pem_blocks: usize,
    pub pem_rejected: usize,
}

/// The kind of big-number structure that points at a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BnKind {
    /// OpenSSL `BIGNUM { BN_ULONG *d; int top; int dmax; int neg; int flags; }`.
    OpenSsl,
    /// mbed TLS 2.x `mbedtls_mpi { int s; size_t n; mbedtls_mpi_uint *p; }`.
    MbedOld,
    /// mbed TLS 3.x `mbedtls_mpi { signed short s; unsigned short n; mbedtls_mpi_uint *p; }`.
    MbedNew,
}

impl BnKind {
    pub fn name(self) -> &'static str {
        match self {
            BnKind::OpenSsl => "OpenSSL BIGNUM",
            BnKind::MbedOld => "mbed TLS mpi (2.x)",
            BnKind::MbedNew => "mbed TLS mpi (3.x)",
        }
    }
}

/// A big-number structure that says "a 2048-bit number is at `ptr`".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BnHit {
    /// Where the structure starts in the scanned bytes.
    pub offset: usize,
    /// The address the structure points at; the number (little-endian, 256 bytes) is there.
    pub ptr: usize,
    pub kind: BnKind,
}

/// A decrypted `BHD5` header lying somewhere in the scanned bytes (not yet proven to be one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeaderHit {
    pub offset: usize,
    /// The size the header declares for itself.
    pub declared: usize,
    pub buckets: usize,
    /// The byte before `BHD5` is zero: the game may have written each 255-byte block into a 256-byte slot.
    pub zero_before: bool,
}

/// A place where the 32-bit hash of a path is followed by a size and an offset that fit an archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryHit {
    pub offset: usize,
    pub padded_size: u32,
    pub file_offset: u64,
}

/// What a decrypted header turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    pub declared: usize,
    pub buckets: usize,
    pub entries: usize,
    /// The end of the file that reaches furthest into the `.bdt`.
    pub max_end: u64,
    /// Files that lie (partly) outside the `.bdt` of `bdt_len` bytes, if that was given.
    pub outside: usize,
}

const PEM_WIDE_BEGIN: &[u8] = b"-\0-\0-\0-\0-\0B\0E\0G\0I\0N\0 \0";
/// Base64 of `30 82 01 0A 02 82 01 01 00`: how a 2048-bit `RSAPublicKey` starts.
const B64_RSA2048: &[u8] = b"MIIBCgKCAQEA";
/// DER of the start of a 2048-bit modulus: `INTEGER`, 257 bytes, leading zero.
const DER_MODULUS_2048: &[u8] = &[0x02, 0x82, 0x01, 0x01, 0x00];
/// DER of `SEQUENCE { OID rsaEncryption, NULL }`.
const DER_RSA_ALGORITHM: &[u8] = &[0x30, 0x0D, 0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01, 0x05, 0x00];
/// `RSA1` and a bit length of 2048, the start of both Windows key blobs.
const RSA1_2048: &[u8] = &[0x52, 0x53, 0x41, 0x31, 0x00, 0x08, 0x00, 0x00];
const MAX_BN_HITS: usize = 100_000;
/// 65537 as a 64-bit little-endian word: how a big-number library or a plain key structure holds the usual exponent.
const EXPONENT_WORD: &[u8] = &[0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
/// A random 256-byte number has about 162 different byte values; text, tables and padding have far fewer.
const MIN_DISTINCT_BYTES: usize = 110;

/// The largest header the scanner takes seriously.
pub const MAX_IMAGE: usize = 64 << 20;

/// The finders, built once; every scan method takes `&self`.
pub struct Scanner {
    pem_wide: Finder<'static>,
    b64: Finder<'static>,
    modulus: Finder<'static>,
    rsa1: Finder<'static>,
    bhd5: Finder<'static>,
    exponent: Finder<'static>,
}

impl Default for Scanner {
    fn default() -> Self {
        Scanner::new()
    }
}

/// The length of the DER `SEQUENCE` at `at`, header included (only the `30 82 hh ll` form real keys use).
fn seq_total(buf: &[u8], at: usize) -> Option<usize> {
    if *buf.get(at)? != 0x30 || *buf.get(at + 1)? != 0x82 {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([*buf.get(at + 2)?, *buf.get(at + 3)?]));
    (len >= 0x40).then_some(len + 4)
}

fn base64_char(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes base64 quads until the text stops being base64 (padding ends it). Text after the key is ignored.
fn base64_prefix(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for quad in text.chunks_exact(4) {
        let pad = quad.iter().rev().take_while(|c| **c == b'=').count();
        let mut v = [0u8; 4];
        let mut ok = true;
        for (slot, c) in v.iter_mut().zip(quad.iter().take(4 - pad.min(2))) {
            match base64_char(*c) {
                Some(x) => *slot = x,
                None => ok = false,
            }
        }
        if !ok || pad > 2 {
            break;
        }
        let n = (u32::from(v[0]) << 18) | (u32::from(v[1]) << 12) | (u32::from(v[2]) << 6) | u32::from(v[3]);
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
        if pad > 0 {
            break;
        }
    }
    out
}

fn push_unique(out: &mut Vec<FoundKey>, found: FoundKey) {
    if !out.iter().any(|f| f.key == found.key) {
        out.push(found);
    }
}

impl Scanner {
    pub fn new() -> Scanner {
        Scanner {
            pem_wide: Finder::new(PEM_WIDE_BEGIN),
            b64: Finder::new(B64_RSA2048),
            modulus: Finder::new(DER_MODULUS_2048),
            rsa1: Finder::new(RSA1_2048),
            bhd5: Finder::new(b"BHD5"),
            exponent: Finder::new(EXPONENT_WORD),
        }
    }

    /// Every RSA public key in `buf`, in any of the [`KeyForm`]s. The same key in several places is reported once (the first).
    pub fn keys(&self, buf: &[u8]) -> KeyScan {
        let mut scan = KeyScan::default();

        // PEM text: look at each marker on its own (a window, so a bad block cannot hide a good one after it)
        for at in memchr::memmem::find_iter(buf, b"-----BEGIN ") {
            let window = buf.get(at..buf.len().min(at + MAX_PEM_BLOCK)).unwrap_or_default();
            let pem = scan_pem(window);
            scan.pem_blocks += pem.blocks.min(1);
            scan.pem_rejected += usize::from(pem.blocks > 0 && pem.keys.is_empty());
            for key in pem.keys.into_iter().take(1) {
                push_unique(&mut scan.keys, FoundKey { form: KeyForm::Pem, offset: at, key });
            }
        }
        // the same text as UTF-16
        for at in self.pem_wide.find_iter(buf) {
            let window = buf.get(at..buf.len().min(at + 2 * MAX_PEM_BLOCK)).unwrap_or_default();
            let ascii: Vec<u8> = window.chunks_exact(2).take_while(|p| p[1] == 0).map(|p| p[0]).collect();
            let pem = scan_pem(&ascii);
            scan.pem_blocks += pem.blocks.min(1);
            scan.pem_rejected += usize::from(pem.blocks > 0 && pem.keys.is_empty());
            for key in pem.keys.into_iter().take(1) {
                push_unique(&mut scan.keys, FoundKey { form: KeyForm::PemWide, offset: at, key });
            }
        }
        // base64 without the PEM lines
        for at in self.b64.find_iter(buf) {
            let run = buf.get(at..).unwrap_or_default();
            let mut text = Vec::with_capacity(512);
            for &c in run.iter().take(2000) {
                match c {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'=' => text.push(c),
                    b'\r' | b'\n' | b' ' | b'\t' => {}
                    _ => break,
                }
                if text.len() >= 1400 {
                    break;
                }
            }
            let der = base64_prefix(&text);
            if let Some(total) = seq_total(&der, 0) {
                if let Some(key) = der.get(..total).and_then(|d| RsaPublicKey::from_pkcs1_der(d).ok()) {
                    push_unique(&mut scan.keys, FoundKey { form: KeyForm::Base64, offset: at, key });
                }
            }
        }
        // DER: the modulus INTEGER of a 2048-bit key, with the SEQUENCE (and maybe the SubjectPublicKeyInfo) around it
        for at in self.modulus.find_iter(buf) {
            let Some(seq) = at.checked_sub(4) else { continue };
            // SubjectPublicKeyInfo: 30 82 hh ll | algorithm (15) | 03 82 hh ll 00 | RSAPublicKey
            if let Some(outer) = seq.checked_sub(24) {
                let alg_ok = get(buf, outer + 4, DER_RSA_ALGORITHM.len()) == Some(DER_RSA_ALGORITHM);
                if alg_ok {
                    if let Some(total) = seq_total(buf, outer) {
                        if let Some(key) = get(buf, outer, total).and_then(|d| RsaPublicKey::from_spki_der(d).ok()) {
                            push_unique(&mut scan.keys, FoundKey { form: KeyForm::SpkiDer, offset: outer, key });
                            continue;
                        }
                    }
                }
            }
            if let Some(total) = seq_total(buf, seq) {
                if let Some(key) = get(buf, seq, total).and_then(|d| RsaPublicKey::from_pkcs1_der(d).ok()) {
                    push_unique(&mut scan.keys, FoundKey { form: KeyForm::Pkcs1Der, offset: seq, key });
                }
            }
        }
        // Windows key blobs
        for at in self.rsa1.find_iter(buf) {
            let w = |i: usize| u32_le(buf, at + i);
            let (Some(w8), Some(w12)) = (w(8), w(12)) else { continue };
            if (1..=8).contains(&w8) && w12 == BLOCK as u32 && w(16) == Some(0) && w(20) == Some(0) {
                // CNG: header of six words, then exponent and modulus, both big-endian
                let exp = get(buf, at + 24, w8 as usize);
                let modulus = get(buf, at + 24 + w8 as usize, BLOCK);
                if let (Some(e), Some(n)) = (exp, modulus) {
                    if let Ok(key) = RsaPublicKey::from_be_bytes(n, e) {
                        push_unique(&mut scan.keys, FoundKey { form: KeyForm::CngBlob, offset: at, key });
                    }
                }
            } else if w8 >= 3 && w8 & 1 == 1 {
                // CryptoAPI: magic, bit length, exponent, then the modulus little-endian
                if let Some(n_le) = get(buf, at + 12, BLOCK) {
                    let n: Vec<u8> = n_le.iter().rev().copied().collect();
                    if let Ok(key) = RsaPublicKey::from_be_bytes(&n, &w8.to_be_bytes()) {
                        push_unique(&mut scan.keys, FoundKey { form: KeyForm::CapiBlob, offset: at, key });
                    }
                }
            }
        }
        scan
    }

    /// Structures that point at a 2048-bit number. `base` is the address `buf` has in memory (only its alignment
    /// matters: structures sit on 8-byte boundaries). At most 100 000 are reported.
    pub fn bignums(&self, buf: &[u8], base: usize) -> Vec<BnHit> {
        let mut hits = Vec::new();
        let first = (8 - base % 8) % 8;
        let user_ptr = |p: u64| p & 7 == 0 && (0x1_0000..0x7FFF_FFFF_FFFF).contains(&p);
        let word = |i: usize| buf.get(i..i + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
        let mut i = first;
        while i + 24 <= buf.len() && hits.len() < MAX_BN_HITS {
            let Some(x) = word(i) else { break };
            // x is the second word of an OpenSSL BIGNUM / the first word of a new mbed TLS mpi / ... see below
            let (lo, hi) = (x as u32, (x >> 32) as u32);
            // OpenSSL: d | top, dmax | neg, flags   (x is at +8)
            if (lo == 32 || lo == 64) && (lo..=lo + 34).contains(&hi) && i >= 8 {
                if let (Some(ptr), Some(neg), Some(flags)) = (word(i - 8), u32_le(buf, i + 8), u32_le(buf, i + 12)) {
                    if neg == 0 && flags <= 7 && user_ptr(ptr) {
                        hits.push(BnHit { offset: i - 8, ptr: ptr as usize, kind: BnKind::OpenSsl });
                    }
                }
            }
            // mbed TLS 2.x: s (int) | n (size_t, at +8) | p (at +16)
            if (x == 32 || x == 64) && i >= 8 && u32_le(buf, i - 8) == Some(1) {
                if let Some(ptr) = word(i + 8).filter(|p| user_ptr(*p)) {
                    hits.push(BnHit { offset: i - 8, ptr: ptr as usize, kind: BnKind::MbedOld });
                }
            }
            // mbed TLS 3.x: s (short) n (unsigned short) pad | p (at +8)
            if x == 0x0020_0001 || x == 0x0040_0001 {
                if let Some(ptr) = word(i + 8).filter(|p| user_ptr(*p)) {
                    hits.push(BnHit { offset: i, ptr: ptr as usize, kind: BnKind::MbedNew });
                }
            }
            i += 8;
        }
        hits
    }

    /// Numbers of 256 bytes lying right before or right after the exponent 65537 (a key kept as a plain structure, or a
    /// big-number library's exponent next to its modulus), as candidate keys: the number as little-endian limbs or as
    /// big-endian bytes, odd, with the top bit set and the look of a random number. A candidate is only a candidate:
    /// whether it is a key of the archives is for [`crate::rsa::first_block_starts_with`] to say. At most 64 per call.
    pub fn moduli_near_exponent(&self, buf: &[u8]) -> Vec<RsaPublicKey> {
        let mut out: Vec<RsaPublicKey> = Vec::new();
        for at in self.exponent.find_iter(buf) {
            let before = at.checked_sub(BLOCK).and_then(|from| get(buf, from, BLOCK));
            let after = get(buf, at + EXPONENT_WORD.len(), BLOCK);
            for window in [before, after].into_iter().flatten() {
                let mut seen = [false; 256];
                for b in window {
                    seen[*b as usize] = true;
                }
                if seen.iter().filter(|s| **s).count() < MIN_DISTINCT_BYTES {
                    continue;
                }
                let reversed: Vec<u8> = window.iter().rev().copied().collect();
                for number in [window, &reversed[..]] {
                    if let Some(key) = modulus_key(number) {
                        if !out.contains(&key) {
                            out.push(key);
                        }
                    }
                }
            }
            if out.len() >= 64 {
                break;
            }
        }
        out
    }

    /// Places that start like a decrypted `BHD5` header. The header fields are checked for sense; whether it is a whole,
    /// sound table is for [`check_image`] to say (the caller reads `declared` bytes from the place first).
    pub fn headers(&self, buf: &[u8]) -> Vec<HeaderHit> {
        let mut out = Vec::new();
        for at in self.bhd5.find_iter(buf) {
            let (Some(&endian), Some(&z1), Some(&z2)) = (buf.get(at + 4), buf.get(at + 6), buf.get(at + 7)) else { continue };
            if endian != 0xFF || z1 != 0 || z2 != 0 || i32_le(buf, at + 8) != Some(1) {
                continue;
            }
            let (Some(declared), Some(buckets), Some(table), Some(salt)) = (i32_le(buf, at + 0x0C), i32_le(buf, at + 0x10), i32_le(buf, at + 0x14), i32_le(buf, at + 0x18)) else { continue };
            let (Ok(declared), Ok(buckets), Ok(table), Ok(salt)) = (usize::try_from(declared), usize::try_from(buckets), usize::try_from(table), usize::try_from(salt)) else { continue };
            let sane = (0x1C..=MAX_IMAGE).contains(&declared)
                && (1..=1 << 22).contains(&buckets)
                && salt <= 4096
                && table >= 0x1C + salt
                && table.checked_add(buckets * 8).is_some_and(|end| end <= declared);
            if sane {
                out.push(HeaderHit { offset: at, declared, buckets, zero_before: at.checked_sub(1).and_then(|p| buf.get(p)) == Some(&0) });
            }
        }
        out
    }

    /// Where `hash` (little-endian) is followed by an `i32` size in `1..=max_size` and an `i64` offset in `0..=max_offset`
    /// that is a multiple of 16 (the shape of a file header in the table of contents). At most `limit` hits.
    pub fn entry_hits(&self, buf: &[u8], hash: u32, max_size: u32, max_offset: u64, limit: usize) -> Vec<EntryHit> {
        let mut out = Vec::new();
        for at in memchr::memmem::find_iter(buf, &hash.to_le_bytes()) {
            let (Some(size), Some(offset)) = (i32_le(buf, at + 4), i64_le(buf, at + 8)) else { continue };
            let (Ok(size), Ok(offset)) = (u32::try_from(size), u64::try_from(offset)) else { continue };
            // the files of an archive start on multiples of 16 bytes
            if (1..=max_size).contains(&size) && offset <= max_offset && offset.is_multiple_of(16) {
                out.push(EntryHit { offset: at, padded_size: size, file_offset: offset });
                if out.len() >= limit {
                    break;
                }
            }
        }
        out
    }
}

/// The key of a candidate modulus read from memory: 256 bytes, little-endian (the way big-number structures store them),
/// odd and with the top bit set, with the usual exponent 65537.
pub fn modulus_key(limbs_le: &[u8]) -> Option<RsaPublicKey> {
    if limbs_le.len() != BLOCK || limbs_le.first().is_none_or(|b| b & 1 == 0) || limbs_le.last().is_none_or(|b| b & 0x80 == 0) {
        return None;
    }
    RsaPublicKey::new(BigUint::from_bytes_le(limbs_le), BigUint::from(65537u32)).ok()
}

/// Does a plain header of `declared` bytes belong to a `.bhd` file of `bhd_len` bytes? The file is made of 256-byte blocks
/// that hold 255 plain bytes each, the last one padded; a header that declares anything between "just fills the last
/// block" and "the size of the file" is taken to fit (the difference is under one percent, far less than the difference
/// between two archives).
pub fn pairs_with(declared: usize, bhd_len: u64) -> bool {
    let declared = declared as u64;
    let blocks = bhd_len / BLOCK as u64 + u64::from(!bhd_len.is_multiple_of(BLOCK as u64));
    blocks > 0 && declared > (blocks - 1) * 255 && declared <= blocks * BLOCK as u64
}

/// A header the game wrote with a zero byte in front of every 255 plain bytes (256-byte slots): the plain bytes only.
pub fn compact_strided(raw: &[u8]) -> Vec<u8> {
    raw.chunks(BLOCK).flat_map(|slot| slot.iter().skip(1).copied()).collect()
}

/// Proof that `image` is a decrypted `BHD5` header: it parses completely (every bucket, file header and record is inside
/// it). With `bdt_len`, the number of files that reach past the end of the `.bdt` is counted too.
pub fn check_image(image: &[u8], bdt_len: Option<u64>) -> Result<ImageInfo, String> {
    let parsed = Bhd5::parse(image.to_vec()).map_err(|e| e.to_string())?;
    let declared = parsed.len();
    let mut max_end = 0u64;
    let mut outside = 0usize;
    for e in parsed.entries() {
        let end = e.offset.saturating_add(u64::from(e.padded_size));
        max_end = max_end.max(end);
        if bdt_len.is_some_and(|len| end > len) {
            outside += 1;
        }
    }
    Ok(ImageInfo { declared, buckets: parsed.bucket_count(), entries: parsed.entries().len(), max_end, outside })
}

/// The whole of `reader`, scanned for keys in pieces of `chunk` bytes (with an overlap so a key cut in two is found).
/// For the program file: the PEM keys are found by [`crate::keys::scan_reader`]; this finds the other shapes.
pub fn scan_reader_keys<R: std::io::Read>(mut r: R, chunk: usize) -> std::io::Result<Vec<FoundKey>> {
    const OVERLAP: usize = 4096;
    let scanner = Scanner::new();
    let chunk = chunk.max(2 * OVERLAP);
    let mut buf = vec![0u8; chunk];
    let mut filled = 0usize;
    let mut base = 0usize;
    let mut out: Vec<FoundKey> = Vec::new();
    loop {
        let n = match r.read(buf.get_mut(filled..).unwrap_or_default()) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        filled += n;
        if n == 0 || filled == buf.len() {
            let data = buf.get(..filled).unwrap_or_default();
            for mut f in scanner.keys(data).keys {
                f.offset += base;
                push_unique(&mut out, f);
            }
            if n == 0 {
                break;
            }
            let keep = OVERLAP.min(filled);
            buf.copy_within(filled - keep..filled, 0);
            base += filled - keep;
            filled = keep;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::base64_encode;
    use crate::testing::archive::{ArchiveBuilder, Bhd5Spec, FileSpec};
    use crate::testing::keys::test_key;

    fn noise(seed: u64, len: usize) -> Vec<u8> {
        crate::testing::install::noise(seed, len)
    }

    fn with(prefix: usize, thing: &[u8], suffix: usize) -> Vec<u8> {
        let mut v = noise(1, prefix);
        v.extend_from_slice(thing);
        v.extend(noise(2, suffix));
        v
    }

    fn key0() -> RsaPublicKey {
        test_key(0).public
    }

    fn der(key: &RsaPublicKey) -> Vec<u8> {
        key.to_pkcs1_der()
    }

    fn spki(key: &RsaPublicKey) -> Vec<u8> {
        let inner = der(key);
        let mut bits = vec![0x03, 0x82, ((inner.len() + 1) >> 8) as u8, ((inner.len() + 1) & 0xFF) as u8, 0x00];
        bits.extend(&inner);
        let mut body = DER_RSA_ALGORITHM.to_vec();
        body.extend(bits);
        let mut out = vec![0x30, 0x82, (body.len() >> 8) as u8, (body.len() & 0xFF) as u8];
        out.extend(body);
        out
    }

    fn forms(buf: &[u8]) -> Vec<KeyForm> {
        Scanner::new().keys(buf).keys.iter().map(|f| f.form).collect()
    }

    #[test]
    fn finds_a_key_in_each_of_its_shapes() {
        let key = key0();
        let pem = key.to_pem();
        assert_eq!(forms(&with(100, pem.as_bytes(), 50)), vec![KeyForm::Pem]);

        let wide: Vec<u8> = pem.bytes().flat_map(|b| [b, 0]).collect();
        assert_eq!(forms(&with(100, &wide, 50)), vec![KeyForm::PemWide]);

        let b64 = base64_encode(&der(&key));
        assert_eq!(forms(&with(7, b64.as_bytes(), 9)), vec![KeyForm::Base64]);
        let lines: String = b64.as_bytes().chunks(64).map(|c| format!("{}\r\n", String::from_utf8_lossy(c))).collect();
        assert_eq!(forms(&with(7, lines.as_bytes(), 9)), vec![KeyForm::Base64], "with line breaks");

        assert_eq!(forms(&with(33, &der(&key), 20)), vec![KeyForm::Pkcs1Der]);
        assert_eq!(forms(&with(33, &spki(&key), 20)), vec![KeyForm::SpkiDer]);

        // CNG: RSA1, 2048, cbPublicExp 3, cbModulus 256, 0, 0, exponent, modulus (big-endian)
        let mut cng = b"RSA1".to_vec();
        for w in [2048u32, 3, 256, 0, 0] {
            cng.extend(w.to_le_bytes());
        }
        cng.extend([1, 0, 1]);
        cng.extend(key.n.to_bytes_be());
        assert_eq!(forms(&with(5, &cng, 5)), vec![KeyForm::CngBlob]);

        // CryptoAPI: PUBLICKEYBLOB header, RSA1, 2048, exponent 65537, modulus (little-endian)
        let mut capi = vec![0x06, 0x02, 0x00, 0x00, 0x00, 0xA4, 0x00, 0x00];
        capi.extend(b"RSA1");
        capi.extend(2048u32.to_le_bytes());
        capi.extend(65537u32.to_le_bytes());
        capi.extend(key.n.to_bytes_le());
        assert_eq!(forms(&with(5, &capi, 5)), vec![KeyForm::CapiBlob]);

        // every shape gives back the same key
        for buf in [pem.as_bytes().to_vec(), der(&key), spki(&key), cng, capi] {
            let found = Scanner::new().keys(&buf).keys;
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].key, key);
        }
    }

    #[test]
    fn two_keys_and_the_same_key_twice() {
        let (a, b) = (test_key(0).public, test_key(1).public);
        let mut buf = a.to_pem().into_bytes();
        buf.extend(noise(3, 40));
        buf.extend(der(&b));
        buf.extend(noise(4, 40));
        buf.extend(der(&a));
        let found = Scanner::new().keys(&buf).keys;
        assert_eq!(found.iter().map(|f| f.key.clone()).collect::<Vec<_>>(), vec![a, b], "the repeated key is reported once");
        assert_eq!(found.iter().map(|f| f.form).collect::<Vec<_>>(), vec![KeyForm::Pem, KeyForm::Pkcs1Der]);
    }

    #[test]
    fn damaged_or_unrelated_bytes_give_no_key_and_no_panic() {
        let key = key0();
        let mut broken = der(&key);
        broken[100] ^= 0x55; // still a number, but the structure after it is fine: this stays a (different) key or none
        let _ = Scanner::new().keys(&broken);
        let mut truncated = der(&key);
        truncated.truncate(200);
        assert!(Scanner::new().keys(&truncated).keys.is_empty());
        let mut even = der(&key);
        let last_modulus_byte = 4 + 4 + 1 + 255; // sequence header, integer header, leading zero, 255 more bytes
        even[last_modulus_byte] &= 0xFE;
        assert!(Scanner::new().keys(&even).keys.is_empty(), "an even modulus is no key");
        for len in [0usize, 1, 3, 11, 12, 13, 31, 100] {
            for seed in 0..20 {
                let junk = noise(seed, len);
                let _ = Scanner::new().keys(&junk);
                let _ = Scanner::new().bignums(&junk, 0);
                let _ = Scanner::new().headers(&junk);
            }
        }
        assert!(Scanner::new().keys(&noise(9, 50_000)).keys.is_empty());
        // text that only mentions a key
        assert!(Scanner::new().keys(b"-----BEGIN RSA PUBLIC KEY----- and nothing else").keys.is_empty());
        let counted = Scanner::new().keys(b"-----BEGIN PUBLIC KEY-----\nQUJD\n-----END PUBLIC KEY-----\n");
        assert!(counted.keys.is_empty());
        assert_eq!((counted.pem_blocks, counted.pem_rejected), (1, 1));
        // base64 that begins like a key but ends early
        assert!(Scanner::new().keys(b"MIIBCgKCAQEAAAAAAAAAAAAA").keys.is_empty());
    }

    #[test]
    fn a_key_cut_by_a_chunk_boundary_is_found_by_the_reader_scan() {
        let key = key0();
        for chunk in [8192usize, 9000, 12_000, 1 << 20] {
            // the key starts 100 bytes before the end of the first chunk, so the first chunk holds only part of it
            let start = chunk.max(8192) - 100;
            let mut data = noise(5, start);
            data.extend(der(&key));
            data.extend(noise(6, 9000));
            let found = scan_reader_keys(std::io::Cursor::new(&data), chunk).unwrap();
            assert_eq!(found.len(), 1, "chunk {chunk}");
            assert_eq!(found[0].key, key);
            assert_eq!(found[0].offset, start, "offsets are positions in the file");
        }
        assert!(scan_reader_keys(std::io::Cursor::new(Vec::<u8>::new()), 8192).unwrap().is_empty());
    }

    fn put_u64(buf: &mut [u8], at: usize, v: u64) {
        buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }

    fn put_u32(buf: &mut [u8], at: usize, v: u32) {
        buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn big_number_structures_that_point_at_a_2048_bit_number_are_found() {
        let ptr = 0x0000_01F4_1234_5000u64;
        // OpenSSL BIGNUM at offset 64 of a buffer whose base address is 8-aligned
        let mut buf = noise(7, 512);
        buf[64..64 + 24].fill(0);
        put_u64(&mut buf, 64, ptr);
        put_u32(&mut buf, 72, 32);
        put_u32(&mut buf, 76, 33);
        put_u32(&mut buf, 80, 0);
        put_u32(&mut buf, 84, 1);
        let hits = Scanner::new().bignums(&buf, 0x7000);
        assert!(hits.contains(&BnHit { offset: 64, ptr: ptr as usize, kind: BnKind::OpenSsl }), "{hits:?}");
        // a base address that is not 8-aligned shifts the grid: the same bytes are then not on a boundary
        assert!(!Scanner::new().bignums(&buf, 0x7001).iter().any(|h| h.offset == 64));
        assert!(Scanner::new().bignums(&buf, 0x7000 - 8).iter().any(|h| h.offset == 64), "any multiple of 8 is fine");

        // a negative number, a wild flag word or a pointer that cannot be one are not
        for (neg, flags, p) in [(1u32, 1u32, ptr), (0, 99, ptr), (0, 1, 7), (0, 1, 0xFFFF_FFFF_FFFF_FFF0)] {
            let mut b = vec![0u8; 128];
            put_u64(&mut b, 8, p);
            put_u32(&mut b, 16, 32);
            put_u32(&mut b, 20, 32);
            put_u32(&mut b, 24, neg);
            put_u32(&mut b, 28, flags);
            assert!(!Scanner::new().bignums(&b, 0).iter().any(|h| h.kind == BnKind::OpenSsl), "neg {neg} flags {flags} ptr {p:#x}");
        }

        // mbed TLS 2.x: int s = 1 | size_t n = 32 | pointer
        let mut old = vec![0u8; 128];
        put_u32(&mut old, 16, 1);
        put_u64(&mut old, 24, 32);
        put_u64(&mut old, 32, ptr);
        assert!(Scanner::new().bignums(&old, 0).contains(&BnHit { offset: 16, ptr: ptr as usize, kind: BnKind::MbedOld }));
        // mbed TLS 3.x: short s = 1, unsigned short n = 32, pointer
        let mut new = vec![0u8; 128];
        new[16..18].copy_from_slice(&1u16.to_le_bytes());
        new[18..20].copy_from_slice(&32u16.to_le_bytes());
        put_u64(&mut new, 24, ptr);
        assert!(Scanner::new().bignums(&new, 0).contains(&BnHit { offset: 16, ptr: ptr as usize, kind: BnKind::MbedNew }));
        // nothing but zeros and nothing but noise
        assert!(Scanner::new().bignums(&[0u8; 4096], 0).is_empty());
    }

    #[test]
    fn a_modulus_next_to_the_exponent_is_a_candidate_in_either_byte_order() {
        let key = key0();
        let le = key.n.to_bytes_le();
        let be = key.n.to_bytes_be();
        let exp = 65537u64.to_le_bytes();
        // little-endian limbs, then the exponent (a plain structure)
        let mut a = noise(1, 40);
        a.extend(&le);
        a.extend(exp);
        a.extend(noise(2, 40));
        // the exponent, then the modulus big-endian
        let mut b = noise(3, 40);
        b.extend(exp);
        b.extend(&be);
        b.extend(noise(4, 40));
        for (name, buf) in [("limbs then exponent", a), ("exponent then big-endian", b)] {
            let found = Scanner::new().moduli_near_exponent(&buf);
            assert!(found.contains(&key), "{name}: {found:?}");
        }
        // padding, text and short patterns are not candidates
        let mut dull = vec![0xAAu8; 256];
        dull[0] = 0xAB;
        dull[255] = 0xAB;
        dull.extend(exp);
        assert!(Scanner::new().moduli_near_exponent(&dull).is_empty());
        let mut neither = le.clone();
        neither[0] = 0x10; // even as little-endian limbs, and no top bit as big-endian bytes
        neither.extend(exp);
        assert!(Scanner::new().moduli_near_exponent(&neither).is_empty(), "no reading of this number is an odd 2048-bit modulus");
        assert!(Scanner::new().moduli_near_exponent(&exp).is_empty());
    }

    #[test]
    fn a_modulus_read_from_memory_needs_the_marks_of_one() {
        let key = key0();
        let le = key.n.to_bytes_le();
        assert_eq!(modulus_key(&le), Some(key.clone()));
        let mut even = le.clone();
        even[0] &= 0xFE;
        assert_eq!(modulus_key(&even), None);
        let mut small = le.clone();
        small[255] &= 0x7F;
        assert_eq!(modulus_key(&small), None, "the top bit of a 2048-bit modulus is set");
        assert_eq!(modulus_key(&le[..255]), None);
        assert_eq!(modulus_key(&[]), None);
    }

    /// A plain header and the `.bhd` bytes it is encrypted to.
    fn header_and_file(files: usize) -> (Vec<u8>, usize) {
        let specs: Vec<FileSpec> = (0..files).map(|i| FileSpec::plain(1000 + i as u32 * 7, 64, 16 + i as u64 * 64).unpadded(60)).collect();
        let plain = Bhd5Spec { salt: "FRPGHDRSALT".to_string(), buckets: 5, files: specs }.build();
        let encrypted = test_key(0).encrypt_header(&plain);
        (plain, encrypted.len())
    }

    #[test]
    fn a_decrypted_header_in_memory_is_found_checked_and_paired() {
        let (plain, file_len) = header_and_file(40);
        let bdt_len = 16 + 40 * 64;
        let buf = with(1000, &plain, 3000);
        let heads = Scanner::new().headers(&buf);
        assert_eq!(heads.len(), 1);
        let h = heads[0];
        assert_eq!((h.offset, h.declared, h.buckets), (1000, plain.len(), 5));
        let image = &buf[h.offset..h.offset + h.declared];
        let info = check_image(image, Some(bdt_len as u64)).unwrap();
        assert_eq!((info.declared, info.buckets, info.entries, info.outside), (plain.len(), 5, 40, 0));
        assert_eq!(info.max_end, bdt_len as u64);
        assert_eq!(check_image(image, Some(bdt_len as u64 - 1)).unwrap().outside, 1, "the last file reaches past a shorter .bdt");
        assert!(pairs_with(h.declared, file_len as u64), "{} {file_len}", h.declared);
        assert!(!pairs_with(h.declared, file_len as u64 + 256 * 3));
        assert!(!pairs_with(h.declared, 256));
        assert!(!pairs_with(h.declared, 0));
        // a header that was cut is not an image
        assert!(check_image(&image[..image.len() - 1], None).is_err());
        assert!(check_image(&[0u8; 100], None).is_err());
    }

    #[test]
    fn pairing_follows_the_block_arithmetic_for_any_size() {
        for plain_len in [0x1Cusize, 100, 254, 255, 256, 509, 510, 511, 5000, 1_300_000] {
            let blocks = plain_len.div_ceil(255);
            assert!(pairs_with(plain_len, (blocks * 256) as u64), "{plain_len}");
            // an archive that is 2 % bigger or smaller is another archive
            assert!(!pairs_with(plain_len, (blocks * 256 + blocks * 256 / 50 + 512) as u64), "{plain_len} against a bigger file");
            if blocks > 4 {
                assert!(!pairs_with(plain_len, (blocks * 256 - blocks * 256 / 50 - 512) as u64), "{plain_len} against a smaller file");
            }
            assert!(!pairs_with(plain_len, 0));
        }
        // a header that declares the size of the whole file is taken to fit as well
        assert!(pairs_with(20 * 256, 20 * 256));
        assert!(!pairs_with(20 * 256 + 1, 20 * 256));
    }

    #[test]
    fn a_header_written_into_256_byte_slots_can_be_compacted() {
        let (plain, _) = header_and_file(30);
        let mut slots = Vec::new();
        for chunk in plain.chunks(255) {
            slots.push(0u8);
            slots.extend_from_slice(chunk);
            slots.resize(slots.len().div_ceil(256) * 256, 0);
        }
        // "BHD5" sits one byte into the first slot
        let heads = Scanner::new().headers(&with(10, &slots, 0));
        assert_eq!(heads.len(), 1);
        assert!(heads[0].zero_before);
        let compact = compact_strided(&slots);
        assert_eq!(&compact[..plain.len()], &plain[..]);
        assert!(check_image(&compact, None).is_ok());
    }

    #[test]
    fn text_that_only_says_bhd5_is_not_a_header() {
        for junk in [&b"BHD5"[..], b"BHD5 archive header", &[b'B', b'H', b'D', b'5', 0xFF, 0, 0, 0, 1, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x7F, 1, 0, 0, 0, 0x1C, 0, 0, 0, 0, 0, 0, 0][..]] {
            assert!(Scanner::new().headers(&with(30, junk, 30)).is_empty());
        }
        // sane numbers but a bucket table that does not fit
        let (mut plain, _) = header_and_file(3);
        plain[0x10..0x14].copy_from_slice(&1_000_000i32.to_le_bytes());
        assert!(Scanner::new().headers(&plain).is_empty());
    }

    #[test]
    fn entry_hits_show_where_a_known_hash_is_followed_by_a_size_and_an_offset() {
        let (plain, _) = header_and_file(10);
        let hash = 1000 + 3 * 7u32;
        let buf = with(64, &plain, 64);
        let hits = Scanner::new().entry_hits(&buf, hash, 1 << 30, 1 << 34, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].padded_size, hits[0].file_offset), (64, 16 + 3 * 64));
        assert!(Scanner::new().entry_hits(&buf, hash, 10, 1 << 34, 10).is_empty(), "too big for the limit");
        assert!(Scanner::new().entry_hits(&buf, hash, 1 << 30, 100, 10).is_empty(), "offset beyond the limit");
        assert!(Scanner::new().entry_hits(&buf, 0xDEAD_BEEF, 1 << 30, 1 << 34, 10).is_empty());
    }

    #[test]
    fn a_whole_archive_built_by_the_test_builder_has_a_header_that_checks_out() {
        let mut b = ArchiveBuilder::new(7);
        b.add("/msg/ENGLISH/item.msgbnd.dcx", &[1u8; 200]).add("/regulation.bin", &[2u8; 90]);
        let (bhd, bdt) = b.build(&test_key(0));
        let plain = crate::rsa::decrypt_header(&test_key(0).public, &bhd).unwrap();
        let heads = Scanner::new().headers(&plain);
        assert_eq!(heads.len(), 1);
        let info = check_image(&plain[..heads[0].declared], Some(bdt.len() as u64)).unwrap();
        assert_eq!((info.entries, info.outside), (2, 0));
        assert!(pairs_with(heads[0].declared, bhd.len() as u64));
    }
}
