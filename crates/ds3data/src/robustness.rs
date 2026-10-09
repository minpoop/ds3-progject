//! Damaged and hostile input for every parser: random garbage, valid files with a few bytes changed, truncated and
//! extended files. Nothing may panic, run away or allocate from a number in the file; whatever comes back as `Ok` must be
//! usable without panicking either. The generator is a fixed xorshift, so a failure can be repeated.
use crate::archive::Archive;
use crate::bhd5::Bhd5;
use crate::bnd4::{replace_file, Bnd4};
use crate::dcx;
use crate::fmg::FmgFile;
use crate::hash::path_hash;
use crate::install::Ds3Install;
use crate::keys::{base64_decode, scan_pem, scan_reader_chunked, PemScanner, RsaPublicKey};
use crate::msgpatch::{patch_item_msgbnd, ItemEdit};
use crate::testing::archive::{ArchiveBuilder, Bhd5Spec, FileSpec};
use crate::testing::items::{item_bnd4, item_dcx, item_tables};
use crate::testing::keys::test_key;
use std::time::{Duration, Instant};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// `data` with a few random changes: a byte set, a bit flipped, a wild number written over four bytes, a cut, an insertion,
/// a deletion or a run filled with one value.
fn mutate(rng: &mut Rng, data: &[u8]) -> Vec<u8> {
    let mut x = data.to_vec();
    for _ in 0..1 + rng.below(4) {
        if x.is_empty() {
            break;
        }
        let at = rng.below(x.len());
        match rng.below(7) {
            0 => x[at] = rng.next() as u8,
            1 => x[at] ^= 1 << rng.below(8),
            2 => {
                let wild = [0u32, 1, 2, 0x10, 0x100, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF, x.len() as u32, 0x40][rng.below(10)].to_le_bytes();
                for (k, w) in wild.iter().enumerate() {
                    if let Some(slot) = x.get_mut(at + k) {
                        *slot = *w;
                    }
                }
            }
            3 => x.truncate(at),
            4 => x.insert(at, rng.next() as u8),
            5 => {
                x.remove(at);
            }
            _ => {
                let end = (at + rng.below(16)).min(x.len());
                let fill = rng.next() as u8;
                x[at..end].fill(fill);
            }
        }
    }
    x
}

/// Runs `f`, which must not take long.
fn quick<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = f();
    assert!(started.elapsed() < Duration::from_secs(5), "{what} took {:?}", started.elapsed());
    out
}

fn edits() -> Vec<ItemEdit<'static>> {
    vec![ItemEdit { id: 2_000_000, expect_name: "Shortsword", new_name: "Chainsword", short_text: Some("s"), long_text: Some("l\nl") }]
}

/// Everything that can be done with bytes that claim to be one of the formats.
fn poke_everything(bytes: &[u8]) {
    let _ = scan_pem(bytes);
    let _ = base64_decode(bytes);
    let _ = RsaPublicKey::from_der(bytes);
    if let Ok(h) = Bhd5::parse(bytes.to_vec()) {
        for e in h.entries().iter().take(50) {
            let _ = h.aes_record(e);
            let _ = h.sha_record(e);
        }
    }
    if let Ok((inner, info)) = dcx::decode_limited(bytes, 1 << 24) {
        let _ = dcx::encode(&inner, &info);
    }
    let _ = dcx::decode_if_dcx(bytes);
    if let Ok(b) = Bnd4::parse(bytes) {
        for f in b.files.iter().take(8) {
            let _ = b.file_bytes(bytes, f.index);
            let _ = replace_file(bytes, &b, f.index, b"new data for the file");
        }
    }
    if let Ok(t) = FmgFile::parse(bytes) {
        let _ = FmgFile::parse(&t.to_bytes());
    }
    let _ = patch_item_msgbnd(bytes, &edits());
}

#[test]
fn random_garbage_never_panics_any_parser() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    // the magic numbers of each format in front of garbage get the parsers past their first check
    let prefixes: [&[u8]; 8] = [b"", b"BHD5\xFF", b"DCX\0", b"BND4", &[0, 0, 2, 0], b"-----BEGIN RSA PUBLIC KEY-----\n", b"\x30\x82", b"DCX\0\0\x01\0\0\0\0\0\x18\0\0\0\x24\0\0\0\x44\0\0\0\x4C"];
    for round in 0..600 {
        let len = [0usize, 1, 3, 4, 7, 16, 40, 64, 100, 255, 256, 257, 512, 1000, 2048][round % 15];
        let mut data = prefixes[round % prefixes.len()].to_vec();
        data.extend(rng.bytes(len));
        quick("garbage", || poke_everything(&data));
    }
}

#[test]
fn valid_files_with_random_damage_never_panic() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let bhd = Bhd5Spec {
        salt: "salt".to_string(),
        buckets: 5,
        files: vec![
            FileSpec::plain(1, 64, 0x10),
            FileSpec::plain(2, 80, 0x60).unpadded(75).aes([7; 16], &[(0, 32), (48, 64), (-1, -1)]).sha([5; 32], &[(0, 80)]),
            FileSpec::plain(3, 16, 0xB0),
        ],
    }
    .build();
    let samples: Vec<(&str, Vec<u8>)> = vec![
        ("BHD5", bhd),
        ("DCX", item_dcx()),
        ("BND4", item_bnd4()),
        ("FMG", item_tables().remove(1).fmg.to_bytes()),
        ("PEM", test_key(0).pem().into_bytes()),
        ("DER", test_key(0).public.to_pkcs1_der()),
    ];
    for (name, good) in &samples {
        poke_everything(good);
        // every truncation
        for cut in (0..good.len()).step_by((good.len() / 300).max(1)) {
            quick(name, || poke_everything(&good[..cut]));
        }
        // random changes, and changes on top of changes
        for _ in 0..400 {
            let once = mutate(&mut rng, good);
            quick(name, || poke_everything(&once));
            let twice = mutate(&mut rng, &once);
            quick(name, || poke_everything(&twice));
        }
        // appended junk
        let mut longer = good.clone();
        longer.extend(rng.bytes(333));
        quick(name, || poke_everything(&longer));
    }
}

#[test]
fn key_text_cut_into_chunks_and_damaged_is_never_a_problem() {
    let mut rng = Rng(77);
    let mut doc = rng.bytes(700);
    doc.extend(test_key(0).pem().into_bytes());
    doc.extend(rng.bytes(300));
    doc.extend(test_key(1).pem().into_bytes());
    for round in 0..300 {
        let damaged = if round == 0 { doc.clone() } else { mutate(&mut rng, &doc) };
        let whole = scan_pem(&damaged);
        let chunk = 1 + rng.below(97);
        let streamed = scan_reader_chunked(&damaged[..], chunk).unwrap();
        assert_eq!(streamed.pem.keys, whole.keys, "chunk {chunk}");
        assert_eq!((streamed.pem.blocks, streamed.pem.rejected), (whole.blocks, whole.rejected), "chunk {chunk}");
        let mut scanner = PemScanner::new();
        for piece in damaged.chunks(chunk) {
            scanner.feed(piece);
        }
        assert_eq!(scanner.finish().keys, whole.keys);
    }
}

#[test]
fn archives_with_damaged_headers_or_data_never_panic() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    let key = test_key(0);
    let mut rng = Rng(424_242);
    // damaged plain headers, encrypted with the real key so that the parser (not the key check) sees them
    let mut b = ArchiveBuilder::new(5);
    b.add("/a.bin", &[1u8; 70]);
    b.add_encrypted("/b.bin", &[2u8; 90], [3; 16], &[(0, 48), (-1, -1), (64, 80)]);
    b.add("/c.bin", &[3u8; 10]);
    let (bhd_cipher, bdt) = b.build(&key);
    let plain = crate::rsa::decrypt_header(&key.public, &bhd_cipher).unwrap();
    std::fs::write(dir.join("A.bdt"), &bdt).unwrap();
    for round in 0..40 {
        let damaged = if round == 0 { plain.clone() } else { mutate(&mut rng, &plain) };
        std::fs::write(dir.join("A.bhd"), key.encrypt_header(&damaged)).unwrap();
        if let Ok(a) = quick("open", || Archive::open(&dir.join("A.bhd"), &dir.join("A.bdt"), std::slice::from_ref(&key.public))) {
            for e in a.entries().iter().take(100) {
                let _ = quick("read", || a.read_limited(e, 1 << 20));
            }
            let _ = a.find(path_hash("/a.bin")).count();
        }
    }
    // damaged ciphertext: the key check or the parser refuses it
    for _ in 0..200 {
        let damaged = mutate(&mut rng, &bhd_cipher);
        std::fs::write(dir.join("B.bhd"), &damaged).unwrap();
        std::fs::write(dir.join("B.bdt"), &bdt).unwrap();
        let _ = quick("open", || Archive::open(&dir.join("B.bhd"), &dir.join("B.bdt"), std::slice::from_ref(&key.public)));
    }
    // damaged data file under a good header
    std::fs::write(dir.join("C.bhd"), &bhd_cipher).unwrap();
    for _ in 0..100 {
        let damaged = mutate(&mut rng, &bdt);
        std::fs::write(dir.join("C.bdt"), &damaged).unwrap();
        if let Ok(a) = Archive::open(&dir.join("C.bhd"), &dir.join("C.bdt"), std::slice::from_ref(&key.public)) {
            for e in a.entries() {
                let _ = a.read(e);
            }
        }
    }
}

#[test]
fn an_install_made_of_garbage_files_opens_with_every_archive_refused() {
    let t = tempfile::tempdir().unwrap();
    let game = t.path().join("Game");
    std::fs::create_dir_all(&game).unwrap();
    let mut rng = Rng(5);
    let mut exe = rng.bytes(5000);
    exe.extend(test_key(0).pem().into_bytes());
    std::fs::write(game.join("DarkSoulsIII.exe"), exe).unwrap();
    for (i, size) in [0usize, 1, 100, 255, 256, 257, 511, 512, 513, 4096].iter().enumerate() {
        std::fs::write(game.join(format!("Data{i}.bhd")), rng.bytes(*size)).unwrap();
        std::fs::write(game.join(format!("Data{i}.bdt")), rng.bytes(i * 37)).unwrap();
    }
    // a .bhd of a folder's name, and non-ASCII names
    std::fs::create_dir_all(game.join("Dir.bhd")).unwrap();
    std::fs::write(game.join("\u{30c0}\u{30fc}\u{30af}.bhd"), rng.bytes(300)).unwrap();
    let install = quick("open", || Ds3Install::open(t.path(), &[])).unwrap();
    assert_eq!(install.exe().keys.len(), 1);
    assert!(install.archives().len() >= 10);
    assert!(install.archives().iter().all(|a| a.archive.is_err()), "garbage opens nothing");
    assert!(install.lookup("/msg/engUS/item.msgbnd.dcx").is_empty());
    assert!(install.read("/regulation.bin").is_err());
}
