//! RSA public keys: finding them as PEM text inside any bytes (the player's own `DarkSoulsIII.exe` holds the archive keys
//! as plain text), reading them (DER `RSAPublicKey` and `SubjectPublicKeyInfo`, parsed minimally and strictly) and
//! fingerprints for reports. Only public keys are handled; reports show the fingerprint, never the key.
use num_bigint::BigUint;
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::Read;
use std::path::Path;

/// Moduli outside this range are not taken for keys.
pub const MIN_MODULUS_BITS: u64 = 512;
pub const MAX_MODULUS_BITS: u64 = 8192;
/// A PEM block (both marker lines and the text between them) longer than this is not a key.
pub const MAX_PEM_BLOCK: usize = 8192;
/// A key file bigger than this is not read.
pub const MAX_KEY_FILE: u64 = 4 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// The base64 text is damaged.
    Base64(&'static str),
    /// The DER bytes are not a plain RSA public key.
    Der(&'static str),
    /// The numbers cannot be an RSA public key.
    Key(&'static str),
    /// The key file could not be read.
    Io(String),
    /// The file holds no RSA public key.
    NoKeys,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::Base64(why) => write!(f, "damaged base64 text ({why})"),
            KeyError::Der(why) => write!(f, "not an RSA public key in DER form ({why})"),
            KeyError::Key(why) => write!(f, "not usable as an RSA public key ({why})"),
            KeyError::Io(why) => write!(f, "{why}"),
            KeyError::NoKeys => write!(f, "no RSA public key (a \"-----BEGIN RSA PUBLIC KEY-----\" block) found"),
        }
    }
}

impl std::error::Error for KeyError {}

/// An RSA public key: modulus and exponent. Build one with [`RsaPublicKey::new`] (which checks the numbers) or by reading
/// PEM/DER; `Debug` shows only the size and the fingerprint.
#[derive(Clone, PartialEq, Eq)]
pub struct RsaPublicKey {
    /// The modulus.
    pub n: BigUint,
    /// The public exponent.
    pub e: BigUint,
}

impl fmt::Debug for RsaPublicKey {
    /// Only the size and the fingerprint, so that a key never ends up in a log by accident.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RsaPublicKey({} bit, {})", self.bits(), self.fingerprint())
    }
}

impl RsaPublicKey {
    /// A key from its numbers: the modulus must be odd and have 512..=8192 bits, the exponent odd, at least 3 and below the
    /// modulus.
    pub fn new(n: BigUint, e: BigUint) -> Result<RsaPublicKey, KeyError> {
        let bits = n.bits();
        if !(MIN_MODULUS_BITS..=MAX_MODULUS_BITS).contains(&bits) {
            return Err(KeyError::Key("the modulus has an unusual size"));
        }
        if !n.bit(0) {
            return Err(KeyError::Key("the modulus is even"));
        }
        if !e.bit(0) || e < BigUint::from(3u8) || e >= n {
            return Err(KeyError::Key("the exponent is not usable"));
        }
        Ok(RsaPublicKey { n, e })
    }

    /// A key from big-endian modulus and exponent bytes.
    pub fn from_be_bytes(n: &[u8], e: &[u8]) -> Result<RsaPublicKey, KeyError> {
        RsaPublicKey::new(BigUint::from_bytes_be(n), BigUint::from_bytes_be(e))
    }

    /// Size of the modulus in bits.
    pub fn bits(&self) -> u64 {
        self.n.bits()
    }

    /// Size of the modulus in bytes: the length of one encrypted block (256 for a 2048-bit key).
    pub fn modulus_len(&self) -> usize {
        self.bits().div_ceil(8) as usize
    }

    /// The first 8 hex characters of the SHA-256 of the big-endian modulus bytes. This is what reports show.
    pub fn fingerprint(&self) -> String {
        let digest = Sha256::digest(self.n.to_bytes_be());
        crate::util::hex(&digest[..4])
    }

    /// `RSAPublicKey ::= SEQUENCE { modulus INTEGER, publicExponent INTEGER }`.
    pub fn from_pkcs1_der(der: &[u8]) -> Result<RsaPublicKey, KeyError> {
        let mut outer = Der::new(der);
        let seq = outer.read(0x30)?;
        outer.finish()?;
        let mut inner = Der::new(seq);
        let n = inner.read_uint()?;
        let e = inner.read_uint()?;
        inner.finish()?;
        RsaPublicKey::new(n, e)
    }

    /// `SubjectPublicKeyInfo` holding an `rsaEncryption` key.
    pub fn from_spki_der(der: &[u8]) -> Result<RsaPublicKey, KeyError> {
        const RSA_ENCRYPTION_OID: [u8; 9] = [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
        let mut outer = Der::new(der);
        let spki = outer.read(0x30)?;
        outer.finish()?;
        let mut inner = Der::new(spki);
        let algorithm = inner.read(0x30)?;
        let bits = inner.read(0x03)?;
        inner.finish()?;
        let mut alg = Der::new(algorithm);
        if alg.read(0x06)? != RSA_ENCRYPTION_OID {
            return Err(KeyError::Der("the key is not an rsaEncryption key"));
        }
        if !alg.read(0x05)?.is_empty() {
            return Err(KeyError::Der("the algorithm parameters are not NULL"));
        }
        alg.finish()?;
        match bits.split_first() {
            Some((0, key)) => RsaPublicKey::from_pkcs1_der(key),
            _ => Err(KeyError::Der("the key bit string has unused bits")),
        }
    }

    /// Either form: a `SubjectPublicKeyInfo` starts with a nested SEQUENCE, a plain `RSAPublicKey` with an INTEGER.
    pub fn from_der(der: &[u8]) -> Result<RsaPublicKey, KeyError> {
        match RsaPublicKey::from_pkcs1_der(der) {
            Ok(k) => Ok(k),
            Err(first) => RsaPublicKey::from_spki_der(der).map_err(|_| first),
        }
    }

    /// The key as DER `RSAPublicKey`.
    pub fn to_pkcs1_der(&self) -> Vec<u8> {
        fn uint(v: &BigUint) -> Vec<u8> {
            let mut bytes = v.to_bytes_be();
            if bytes.first().is_some_and(|b| b & 0x80 != 0) {
                bytes.insert(0, 0);
            }
            let mut out = vec![0x02];
            out.extend(der_len(bytes.len()));
            out.extend(bytes);
            out
        }
        let mut body = uint(&self.n);
        body.extend(uint(&self.e));
        let mut out = vec![0x30];
        out.extend(der_len(body.len()));
        out.extend(body);
        out
    }

    /// The key as a `-----BEGIN RSA PUBLIC KEY-----` block with 64-character lines and `\n` line ends.
    pub fn to_pem(&self) -> String {
        let b64 = base64_encode(&self.to_pkcs1_der());
        let mut out = String::from("-----BEGIN RSA PUBLIC KEY-----\n");
        for line in b64.as_bytes().chunks(64) {
            out.push_str(&String::from_utf8_lossy(line));
            out.push('\n');
        }
        out.push_str("-----END RSA PUBLIC KEY-----\n");
        out
    }
}

fn der_len(len: usize) -> Vec<u8> {
    if len < 0x80 {
        vec![len as u8]
    } else {
        let bytes = (len as u64).to_be_bytes();
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        let mut out = vec![0x80 | (8 - skip) as u8];
        out.extend(&bytes[skip..]);
        out
    }
}

/// A strict reader of definite-length DER elements.
struct Der<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Der<'a> {
    fn new(b: &'a [u8]) -> Der<'a> {
        Der { b, pos: 0 }
    }

    /// The content of the next element, which must have the given tag.
    fn read(&mut self, tag: u8) -> Result<&'a [u8], KeyError> {
        let ends_early = KeyError::Der("the data ends early");
        let rest = self.b.get(self.pos..).ok_or_else(|| ends_early.clone())?;
        let (&t, rest) = rest.split_first().ok_or_else(|| ends_early.clone())?;
        if t != tag {
            return Err(KeyError::Der("an unexpected element"));
        }
        let (&first, rest) = rest.split_first().ok_or_else(|| ends_early.clone())?;
        let (len, rest) = if first < 0x80 {
            (first as usize, rest)
        } else if first == 0x80 {
            return Err(KeyError::Der("an indefinite length"));
        } else {
            let count = (first & 0x7F) as usize;
            let (len_bytes, rest) = rest.split_at_checked(count).filter(|_| count <= 4).ok_or(KeyError::Der("an unusable length"))?;
            if len_bytes.first() == Some(&0) {
                return Err(KeyError::Der("a length that is not minimal"));
            }
            let len = len_bytes.iter().fold(0usize, |acc, b| (acc << 8) | *b as usize);
            if len < 0x80 {
                return Err(KeyError::Der("a length that is not minimal"));
            }
            (len, rest)
        };
        let content = rest.get(..len).ok_or(ends_early)?;
        self.pos = self.b.len() - rest.len() + len;
        Ok(content)
    }

    /// The next element must be a non-negative INTEGER in its shortest form.
    fn read_uint(&mut self) -> Result<BigUint, KeyError> {
        let content = self.read(0x02)?;
        match content {
            [] => Err(KeyError::Der("an empty number")),
            [first, ..] if first & 0x80 != 0 => Err(KeyError::Der("a negative number")),
            [0, second, ..] if second & 0x80 == 0 => Err(KeyError::Der("a number that is not minimal")),
            _ => Ok(BigUint::from_bytes_be(content)),
        }
    }

    /// Nothing may follow.
    fn finish(&self) -> Result<(), KeyError> {
        if self.pos == self.b.len() {
            Ok(())
        } else {
            Err(KeyError::Der("extra bytes at the end"))
        }
    }
}

fn base64_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Strict base64 (standard alphabet, `=` padding, no stray bits). White space (space, tab, CR, LF) is ignored.
pub fn base64_decode(text: &[u8]) -> Result<Vec<u8>, KeyError> {
    let clean: Vec<u8> = text.iter().copied().filter(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n')).collect();
    if clean.is_empty() {
        return Err(KeyError::Base64("nothing between the markers"));
    }
    if !clean.len().is_multiple_of(4) {
        return Err(KeyError::Base64("the length is not a multiple of 4"));
    }
    let quads = clean.len() / 4;
    let mut out = Vec::with_capacity(quads * 3);
    for (i, quad) in clean.chunks_exact(4).enumerate() {
        let pad = quad.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 || (pad > 0 && i + 1 != quads) {
            return Err(KeyError::Base64("misplaced padding"));
        }
        let mut v = [0u8; 4];
        for (slot, c) in v.iter_mut().zip(quad.iter().take(4 - pad)) {
            *slot = base64_value(*c).ok_or(KeyError::Base64("a character that is not base64"))?;
        }
        let n = (u32::from(v[0]) << 18) | (u32::from(v[1]) << 12) | (u32::from(v[2]) << 6) | u32::from(v[3]);
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
        if (pad == 2 && v[1] & 0x0F != 0) || (pad == 1 && v[2] & 0x03 != 0) {
            return Err(KeyError::Base64("stray bits at the end"));
        }
    }
    Ok(out)
}

/// Standard base64 with padding and no line breaks.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let letter = |n: u32| ALPHABET.get(n as usize & 63).map_or('?', |c| *c as char);
    for chunk in bytes.chunks(3) {
        let byte = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        out.push(letter(n >> 18));
        out.push(letter(n >> 12));
        out.push(if chunk.len() > 1 { letter(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { letter(n) } else { '=' });
    }
    out
}

const MARKER: &[u8] = b"-----BEGIN ";

/// Where the next `-----BEGIN ` is at or after `from`.
fn find_marker(buf: &[u8], from: usize) -> Option<usize> {
    let mut at = from;
    while let Some(p) = buf.get(at..)?.iter().position(|b| *b == b'-') {
        let candidate = at + p;
        if buf.get(candidate..).is_some_and(|rest| rest.starts_with(MARKER)) {
            return Some(candidate);
        }
        at = candidate + 1;
    }
    None
}

/// What was found by a PEM scan.
#[derive(Debug, Clone, Default)]
pub struct PemScan {
    /// The distinct keys, in the order they first appear.
    pub keys: Vec<RsaPublicKey>,
    /// `RSA PUBLIC KEY` / `PUBLIC KEY` blocks that were looked at.
    pub blocks: usize,
    /// Of those, the ones that did not hold a usable key (damaged text, a different kind of key, ...).
    pub rejected: usize,
}

/// Finds `-----BEGIN RSA PUBLIC KEY-----` and `-----BEGIN PUBLIC KEY-----` blocks in any bytes, in pieces: feed the data
/// in chunks of any size and call [`PemScanner::finish`]; a block that straddles two chunks is found all the same.
#[derive(Default)]
pub struct PemScanner {
    buf: Vec<u8>,
    scan: PemScan,
}

enum Block {
    /// Not enough data yet to decide.
    Incomplete,
    /// Go on from this offset.
    Next(usize),
}

impl PemScanner {
    pub fn new() -> PemScanner {
        PemScanner::default()
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
        self.run(false);
    }

    pub fn finish(mut self) -> PemScan {
        self.run(true);
        self.scan
    }

    fn run(&mut self, last: bool) {
        let mut pos = 0;
        loop {
            let Some(at) = find_marker(&self.buf, pos) else {
                // a marker may begin in the last bytes and end in the next chunk
                pos = pos.max(self.buf.len().saturating_sub(MARKER.len() - 1));
                break;
            };
            match self.block_at(at, last) {
                Block::Incomplete => {
                    pos = at;
                    break;
                }
                Block::Next(next) => pos = next,
            }
        }
        let keep_from = pos.min(self.buf.len());
        self.buf.drain(..keep_from);
    }

    fn block_at(&mut self, at: usize, last: bool) -> Block {
        let label_start = at + MARKER.len();
        let Some(window) = self.buf.get(label_start..self.buf.len().min(label_start + 32)) else { return Block::Next(label_start) };
        let Some(label_len) = window.windows(5).position(|w| w == b"-----") else {
            return if !last && window.len() < 32 { Block::Incomplete } else { Block::Next(label_start) };
        };
        let label = window.get(..label_len).unwrap_or_default().to_vec();
        let spki = match label.as_slice() {
            b"RSA PUBLIC KEY" => false,
            b"PUBLIC KEY" => true,
            _ => return Block::Next(label_start), // a certificate, a private key, ...: not ours
        };
        let body_start = label_start + label_len + 5;
        let mut end_marker = b"-----END ".to_vec();
        end_marker.extend(&label);
        end_marker.extend(b"-----");
        let limit = self.buf.len().min(at + MAX_PEM_BLOCK);
        let found = self.buf.get(body_start..limit).and_then(|body| body.windows(end_marker.len()).position(|w| w == end_marker.as_slice()));
        let Some(rel) = found else {
            if !last && self.buf.len() < at + MAX_PEM_BLOCK {
                return Block::Incomplete;
            }
            self.scan.blocks += 1;
            self.scan.rejected += 1;
            return Block::Next(label_start);
        };
        let end = body_start + rel;
        self.scan.blocks += 1;
        let body = self.buf.get(body_start..end).unwrap_or_default();
        let parsed = base64_decode(body).and_then(|der| if spki { RsaPublicKey::from_spki_der(&der) } else { RsaPublicKey::from_pkcs1_der(&der) });
        match parsed {
            Ok(key) => {
                if !self.scan.keys.contains(&key) {
                    self.scan.keys.push(key);
                }
                Block::Next(end + end_marker.len())
            }
            Err(_) => {
                self.scan.rejected += 1;
                Block::Next(label_start) // a block nested inside the damaged one may still be good
            }
        }
    }
}

/// All the keys that appear as PEM text in `data`.
pub fn scan_pem(data: &[u8]) -> PemScan {
    let mut scanner = PemScanner::new();
    scanner.feed(data);
    scanner.finish()
}

/// Shorthand for `scan_pem(data).keys`.
pub fn find_pem_keys(data: &[u8]) -> Vec<RsaPublicKey> {
    scan_pem(data).keys
}

/// The keys in a PEM file the player gave (`--keys`) or the game's key cache. Fails if the file holds no key.
pub fn load_pem_file(path: &Path) -> Result<Vec<RsaPublicKey>, KeyError> {
    let meta = std::fs::metadata(path).map_err(|e| KeyError::Io(format!("cannot read the key file: {}", io_reason(&e))))?;
    if !meta.is_file() || meta.len() > MAX_KEY_FILE {
        return Err(KeyError::Io("the key file is not a plain file of a sensible size".to_string()));
    }
    let bytes = std::fs::read(path).map_err(|e| KeyError::Io(format!("cannot read the key file: {}", io_reason(&e))))?;
    let keys = find_pem_keys(&bytes);
    if keys.is_empty() {
        Err(KeyError::NoKeys)
    } else {
        Ok(keys)
    }
}

/// An I/O error without the file name in it (reports must not carry the player's folder names).
pub(crate) fn io_reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "no such file".to_string(),
        std::io::ErrorKind::PermissionDenied => "access denied".to_string(),
        _ => e.to_string(),
    }
}

/// What reading a program file once gives: its size, its SHA-256 and the keys that are in it as text.
#[derive(Debug, Clone)]
pub struct ReaderScan {
    pub size: u64,
    pub sha256: [u8; 32],
    pub pem: PemScan,
}

/// Reads `r` once, in chunks of 4 MiB (the program file is 50-100 MB), for its size, SHA-256 and PEM keys.
pub fn scan_reader<R: Read>(r: R) -> std::io::Result<ReaderScan> {
    scan_reader_chunked(r, 4 << 20)
}

/// [`scan_reader`] with a chosen chunk size (tests use tiny ones to cut keys in two).
pub fn scan_reader_chunked<R: Read>(mut r: R, chunk: usize) -> std::io::Result<ReaderScan> {
    let mut sha = Sha256::new();
    let mut scanner = PemScanner::new();
    let mut buf = vec![0u8; chunk.max(1)];
    let mut size = 0u64;
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let Some(data) = buf.get(..n) else { return Err(std::io::Error::other("a read returned more bytes than the buffer holds")) };
        sha.update(data);
        scanner.feed(data);
        size += n as u64;
    }
    Ok(ReaderScan { size, sha256: sha.finalize().into(), pem: scanner.finish() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway 2048-bit key made with `openssl genrsa` for these tests (fingerprint 87febfc8).
    pub(crate) const PKCS1_PEM: &str = "-----BEGIN RSA PUBLIC KEY-----
MIIBCgKCAQEAxW4X4ixn5jg51MeQ9EFq8UMCNZjDWRrBdQK0DoutoIwcZIX4tqG9
crH5M2TKQSiHNtwhUGCERNHv+dlF30vub6JHR9H+FR2oj5R+7UoMHv4stvIIuLM/
O5riq7+nAwTiRYcXDIeLLrfkVsPpU4PkAqMJEbFqTf4dzd85fXLMOlGsqDuqRJCN
UFxF3iPIKuzByNMBWV7fYbzv39n7++JpXT89JWH4y1XdS067w7n1XUFusdaS2Rnu
rhqOu0r8hui9/QEOdSxZDny04q9AaIKmSb78PJq5UCKAzuyoCh8vYvq+qC1KUpxY
Pxdlaw7cnREQh08IajwXQmOfZlxfqzX/uQIDAQAB
-----END RSA PUBLIC KEY-----
";
    pub(crate) const SPKI_PEM: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAxW4X4ixn5jg51MeQ9EFq
8UMCNZjDWRrBdQK0DoutoIwcZIX4tqG9crH5M2TKQSiHNtwhUGCERNHv+dlF30vu
b6JHR9H+FR2oj5R+7UoMHv4stvIIuLM/O5riq7+nAwTiRYcXDIeLLrfkVsPpU4Pk
AqMJEbFqTf4dzd85fXLMOlGsqDuqRJCNUFxF3iPIKuzByNMBWV7fYbzv39n7++Jp
XT89JWH4y1XdS067w7n1XUFusdaS2RnurhqOu0r8hui9/QEOdSxZDny04q9AaIKm
Sb78PJq5UCKAzuyoCh8vYvq+qC1KUpxYPxdlaw7cnREQh08IajwXQmOfZlxfqzX/
uQIDAQAB
-----END PUBLIC KEY-----
";
    const N1_PREFIX: &str = "c56e17e22c67e63839d4c790f4416af1";

    fn der_of(pem: &str) -> Vec<u8> {
        let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
        base64_decode(body.as_bytes()).unwrap()
    }

    #[test]
    fn base64_round_trips_and_is_strict() {
        for len in 0..40usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 7 + 3) as u8).collect();
            let text = base64_encode(&data);
            if len > 0 {
                assert_eq!(base64_decode(text.as_bytes()).unwrap(), data, "length {len}");
            } else {
                assert!(base64_decode(text.as_bytes()).is_err(), "nothing to decode");
            }
        }
        assert_eq!(base64_decode(b"TWFu").unwrap(), b"Man");
        assert_eq!(base64_decode(b"TWE=").unwrap(), b"Ma");
        assert_eq!(base64_decode(b"TQ==").unwrap(), b"M");
        assert_eq!(base64_decode(b"TW\r\nFu \t").unwrap(), b"Man", "white space is ignored");
        for bad in [&b"TWF"[..], b"TWFu=", b"TW=u", b"=WFu", b"TWE=TWFu", b"TWF*", b"TR==", b"TWF=", b"T===", b"====", b"TW\0u"] {
            assert!(base64_decode(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn reads_both_kinds_of_key_and_they_agree() {
        let a = RsaPublicKey::from_pkcs1_der(&der_of(PKCS1_PEM)).unwrap();
        let b = RsaPublicKey::from_spki_der(&der_of(SPKI_PEM)).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.bits(), 2048);
        assert_eq!(a.modulus_len(), 256);
        assert_eq!(a.e, BigUint::from(65537u32));
        assert!(crate::util::hex(&a.n.to_bytes_be()).starts_with(N1_PREFIX));
        assert_eq!(a.fingerprint(), "87febfc8", "sha256 of the modulus bytes, first 4 bytes (checked with python)");
        assert_eq!(RsaPublicKey::from_der(&der_of(PKCS1_PEM)).unwrap(), a);
        assert_eq!(RsaPublicKey::from_der(&der_of(SPKI_PEM)).unwrap(), a);
        assert_eq!(a.to_pkcs1_der(), der_of(PKCS1_PEM), "writing it back gives the same bytes");
        assert_eq!(a.to_pem(), PKCS1_PEM);
        assert_eq!(format!("{a:?}"), "RsaPublicKey(2048 bit, 87febfc8)", "Debug shows no key material");
    }

    #[test]
    fn der_is_parsed_strictly() {
        let good = der_of(PKCS1_PEM);
        // truncated at every length
        for cut in 0..good.len() {
            assert!(RsaPublicKey::from_pkcs1_der(&good[..cut]).is_err(), "cut at {cut}");
        }
        // trailing bytes
        let mut long = good.clone();
        long.push(0);
        assert!(RsaPublicKey::from_pkcs1_der(&long).is_err());
        // a wrong tag, an indefinite length, a non-minimal length, a negative modulus
        let mut x = good.clone();
        x[0] = 0x31;
        assert!(RsaPublicKey::from_pkcs1_der(&x).is_err());
        let mut x = good.clone();
        x[1] = 0x80;
        assert!(RsaPublicKey::from_pkcs1_der(&x).is_err());
        let mut x = good.clone();
        x.splice(1..4, [0x83, 0x00, 0x01, 0x0A]);
        assert!(RsaPublicKey::from_pkcs1_der(&x).is_err(), "length with a leading zero byte");
        let mut x = good.clone();
        x[4 + 4] = 0xC5; // the modulus: drop the leading 00 so the top bit is set
        assert!(RsaPublicKey::from_pkcs1_der(&x).is_err());
        // SPKI with another algorithm
        let mut s = der_of(SPKI_PEM);
        s[16] ^= 1; // the last byte of the algorithm identifier
        assert!(RsaPublicKey::from_spki_der(&s).is_err());
        let mut s = der_of(SPKI_PEM);
        s[17] ^= 1; // the tag of the NULL parameters
        assert!(RsaPublicKey::from_spki_der(&s).is_err());
        assert!(RsaPublicKey::from_pkcs1_der(&der_of(SPKI_PEM)).is_err());
        assert!(RsaPublicKey::from_spki_der(&good).is_err());
        assert!(RsaPublicKey::from_der(&[]).is_err());
        assert!(RsaPublicKey::from_der(&[0x30, 0x00]).is_err());
    }

    #[test]
    fn unusable_numbers_are_refused() {
        let big = BigUint::from_bytes_be(&[0xC5; 256]);
        assert!(RsaPublicKey::new(big.clone(), BigUint::from(65537u32)).is_ok());
        assert!(RsaPublicKey::new(big.clone(), BigUint::from(1u8)).is_err(), "exponent too small");
        assert!(RsaPublicKey::new(big.clone(), BigUint::from(65536u32)).is_err(), "even exponent");
        assert!(RsaPublicKey::new(big.clone() + 1u8, BigUint::from(65537u32)).is_err(), "even modulus");
        assert!(RsaPublicKey::new(BigUint::from(35u8), BigUint::from(5u8)).is_err(), "tiny modulus");
        assert!(RsaPublicKey::new(BigUint::from_bytes_be(&[0xFF; 2000]), BigUint::from(3u8)).is_err(), "huge modulus");
        assert!(RsaPublicKey::new(big.clone(), big.clone()).is_err(), "exponent not below the modulus");
        assert!(RsaPublicKey::from_be_bytes(&[0xC5; 256], &[1, 0, 1]).is_ok());
    }

    #[test]
    fn finds_keys_among_junk() {
        let mut data: Vec<u8> = (0..5000u32).map(|i| (i * 31 % 251) as u8).collect();
        data.extend_from_slice(b"\0\0some text -----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n");
        data.extend_from_slice(PKCS1_PEM.as_bytes());
        data.extend_from_slice(&[0xFF; 100]);
        data.extend_from_slice(SPKI_PEM.replace('\n', "\r\n").as_bytes()); // the same key again, other form and line ends
        data.extend_from_slice(b"-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----");
        let scan = scan_pem(&data);
        assert_eq!(scan.keys.len(), 1, "the same key twice counts once");
        assert_eq!((scan.blocks, scan.rejected), (2, 0));
        assert_eq!(scan.keys[0].fingerprint(), "87febfc8");
        assert!(find_pem_keys(b"nothing here").is_empty());
        assert!(find_pem_keys(b"").is_empty());
    }

    #[test]
    fn damaged_blocks_are_counted_and_do_not_hide_good_ones() {
        let mut data = Vec::new();
        data.extend_from_slice(b"-----BEGIN RSA PUBLIC KEY-----\nMIIBCgKCAQEAxW4X4ixn5jg5\n-----END RSA PUBLIC KEY-----\n");
        data.extend_from_slice(b"-----BEGIN RSA PUBLIC KEY-----\n!!!!\n-----END RSA PUBLIC KEY-----\n");
        data.extend_from_slice(b"-----BEGIN RSA PUBLIC KEY-----\nMIIBCgKC"); // never ended ...
        data.extend_from_slice(PKCS1_PEM.as_bytes()); // ... but a good block follows inside
        data.extend_from_slice(b"-----BEGIN RSA PUBLIC KEY-----\nMIIBCgKCAQEAxW4X");
        let scan = scan_pem(&data);
        assert_eq!(scan.keys.len(), 1);
        assert_eq!(scan.rejected, 4, "{scan:?}");
    }

    #[test]
    fn a_key_cut_by_a_chunk_boundary_is_still_found() {
        let mut data: Vec<u8> = vec![0xAB; 300];
        data.extend_from_slice(PKCS1_PEM.as_bytes());
        data.extend_from_slice(&[0xCD; 77]);
        data.extend_from_slice(SPKI_PEM.as_bytes());
        data.extend_from_slice(&[0x11; 40]);
        let whole = scan_pem(&data);
        assert_eq!(whole.keys.len(), 1);
        let sha_whole = Sha256::digest(&data);
        for chunk in [1usize, 2, 3, 5, 7, 11, 13, 64, 100, 331, 700, 4096, 1 << 20] {
            let scan = scan_reader_chunked(&data[..], chunk).unwrap();
            assert_eq!(scan.pem.keys, whole.keys, "chunk {chunk}");
            assert_eq!((scan.pem.blocks, scan.pem.rejected), (whole.blocks, whole.rejected), "chunk {chunk}");
            assert_eq!(scan.size, data.len() as u64);
            assert_eq!(scan.sha256[..], sha_whole[..], "chunk {chunk}");
        }
        // the default chunk size and an empty input
        assert_eq!(scan_reader(&data[..]).unwrap().pem.keys.len(), 1);
        let empty = scan_reader(&b""[..]).unwrap();
        assert_eq!((empty.size, empty.pem.keys.len()), (0, 0));
        assert_eq!(crate::util::hex(&empty.sha256), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn key_files() {
        let dir = std::env::temp_dir().join(format!("ashen-ds3data-keys-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("keys.pem");
        std::fs::write(&ok, format!("# my keys\n{PKCS1_PEM}\n")).unwrap();
        assert_eq!(load_pem_file(&ok).unwrap().len(), 1);
        let none = dir.join("none.pem");
        std::fs::write(&none, "nothing useful in here").unwrap();
        assert_eq!(load_pem_file(&none), Err(KeyError::NoKeys));
        let err = load_pem_file(&dir.join("missing.pem")).unwrap_err();
        assert!(matches!(&err, KeyError::Io(why) if why.contains("no such file")), "{err}");
        assert!(!err.to_string().contains(dir.to_string_lossy().as_ref()), "no folder names in messages");
        assert!(matches!(load_pem_file(&dir), Err(KeyError::Io(_))), "a folder is not a key file");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
