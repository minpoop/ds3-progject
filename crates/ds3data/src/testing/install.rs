//! A whole synthetic Dark Souls III install: a program file with the test keys among junk, three archives
//! (`Data0` and `DLC1` with test key 0, `Data1` with test key 1) holding fake `item.msgbnd.dcx`, `menu.msgbnd.dcx`,
//! `regulation.bin` and weapon model containers. No game data is in it: every byte is made up.
use super::archive::ArchiveBuilder;
use super::bnd4::Bnd4Spec;
use super::items::{self, bnd4_of, game_path, item_tables};
use super::keys::{test_key, TestKey};
use crate::dcx::{self, DcxInfo};
use crate::fmg::FmgFile;
use std::path::{Path, PathBuf};

/// What the fake install holds for the item text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemMsg {
    /// `/msg/ENGLISH/item.msgbnd.dcx` as the game has it.
    Good,
    /// ... but "Shortsword" at id 2000000 is called "Broadsword" (a game update that renamed things).
    WrongName,
    /// There is no item text at all.
    Missing,
    /// Only `/msg/FRENCH/item.msgbnd.dcx` exists.
    FrenchOnly,
    /// The DCX file is damaged (its checksum fails).
    DamagedDcx,
    /// The BND4 inside is laid out in a way that cannot be patched.
    UnpatchableLayout,
}

#[derive(Clone, Debug)]
pub struct FakeOptions {
    /// Which test keys (0 or 1) appear as PEM text in the program file.
    pub exe_keys: Vec<usize>,
    pub item_msg: ItemMsg,
    /// Put a junk file with the same path hash as the item text in front of it in the same archive.
    pub collide: bool,
    /// Add a `Data9.bhd` without a `.bdt` and a `Data8.bhd`/`.bdt` that no key opens.
    pub broken_archives: bool,
}

impl Default for FakeOptions {
    fn default() -> Self {
        FakeOptions { exe_keys: vec![0, 1], item_msg: ItemMsg::Good, collide: true, broken_archives: false }
    }
}

/// Where a fake install is.
#[derive(Clone, Debug)]
pub struct FakeDs3 {
    /// The folder a Steam library would hold (`.../common/DARK SOULS III`).
    pub root: PathBuf,
    /// `root/Game`.
    pub game: PathBuf,
    pub exe: PathBuf,
}

pub const ITEM_PATH: &str = "/msg/ENGLISH/item.msgbnd.dcx";
pub const MODEL_PATHS: [&str; 5] = [
    "/parts/wp_a_0200.partsbnd.dcx",
    "/parts/wp_a_0200_l.partsbnd.dcx",
    "/parts/wp_a_1404.partsbnd.dcx",
    "/parts/wp_a_1409.partsbnd.dcx",
    "/parts/wp_a_1419.partsbnd.dcx",
];

/// Deterministic junk.
pub fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

/// Encrypted ranges for a stored file of `padded` bytes (a multiple of 16): the start, a hole, the rest.
pub fn ranges_for(padded: usize) -> Vec<(i64, i64)> {
    let first = padded.min(64);
    let mut ranges = vec![(0, first as i64), (-1, -1)];
    if padded > 80 {
        ranges.push((80, padded as i64));
    }
    ranges
}

fn add_encrypted(b: &mut ArchiveBuilder, path: &str, data: &[u8], key: u8) {
    let padded = data.len().div_ceil(16) * 16;
    b.add_encrypted(path, data, [key; 16], &ranges_for(padded));
}

/// A small weapon container (the kind `*.partsbnd.dcx` is), DCX-compressed.
pub fn model_dcx(model: &str) -> Vec<u8> {
    let mut spec = Bnd4Spec::new(0x74);
    let base = format!("N:\\FDP\\data\\INTERROOT_win64\\parts\\weapon\\{model}");
    spec = spec.file(100, &format!("{base}\\{model}.flver"), &noise(model.len() as u64 * 7919, 700));
    spec = spec.file(101, &format!("{base}\\{model}.tpf"), &noise(model.len() as u64 * 104729, 333));
    spec = spec.file(102, &format!("{base}\\{model}.hkx"), &noise(model.len() as u64 * 1299709, 120));
    dcx::encode(&spec.build(), &DcxInfo::ds3_default()).expect("encode")
}

fn small_msg_dcx(table_name: &str, text: &str) -> Vec<u8> {
    let mut fmg = FmgFile::new();
    fmg.set(1, text);
    let bnd = Bnd4Spec::new(0x74).file(1, &game_path(table_name), &fmg.to_bytes()).build();
    dcx::encode(&bnd, &DcxInfo::ds3_default()).expect("encode")
}

/// The DCX bytes of the item text for an option.
pub fn item_msg_dcx(kind: ItemMsg) -> Vec<u8> {
    match kind {
        ItemMsg::Good | ItemMsg::FrenchOnly => items::item_dcx(),
        ItemMsg::Missing => Vec::new(),
        ItemMsg::WrongName => {
            let mut tables = item_tables();
            tables[1].fmg.set(2_000_000, "Broadsword");
            dcx::encode(&bnd4_of(&tables, &[(99, "Readme.txt", b"not a table".to_vec())]), &DcxInfo::ds3_default()).expect("encode")
        }
        ItemMsg::DamagedDcx => {
            let mut bytes = items::item_dcx();
            let n = bytes.len();
            bytes[n / 2] ^= 0x40;
            bytes
        }
        ItemMsg::UnpatchableLayout => {
            let mut spec = Bnd4Spec::new(0x74);
            spec.alignment = 24;
            for t in item_tables() {
                spec = spec.file(t.id, &t.name, &t.fmg.to_bytes());
            }
            dcx::encode(&spec.build(), &DcxInfo::ds3_default()).expect("encode")
        }
    }
}

fn exe_bytes(keys: &[TestKey]) -> Vec<u8> {
    let mut exe = noise(0xD53, 150_000);
    exe[..2].copy_from_slice(b"MZ");
    // a decoy and a damaged block among the real ones
    let mut blocks: Vec<Vec<u8>> = vec![b"-----BEGIN CERTIFICATE-----\nMIIBkTCB+wIJAKH\n-----END CERTIFICATE-----\n".to_vec()];
    for k in keys {
        blocks.push(k.pem().into_bytes());
    }
    blocks.push(b"-----BEGIN RSA PUBLIC KEY-----\nMIIBCgKCAQEAxW4X\n-----END RSA PUBLIC KEY-----\n".to_vec());
    for (i, block) in blocks.iter().enumerate() {
        let at = 10_000 + i * 31_337;
        exe[at..at + block.len()].copy_from_slice(block);
    }
    exe
}

/// Writes a fake install below `root` (the folder is created) and returns where things are.
pub fn build(root: &Path, opts: &FakeOptions) -> FakeDs3 {
    let (k0, k1) = (test_key(0), test_key(1));
    let game = root.join("Game");
    std::fs::create_dir_all(&game).expect("create the Game folder");
    let keys: Vec<TestKey> = opts.exe_keys.iter().map(|i| test_key(*i)).collect();
    let exe = game.join("DarkSoulsIII.exe");
    std::fs::write(&exe, exe_bytes(&keys)).expect("write the program file");

    // Data0: the item text, menu text, regulation, and some junk files
    let mut data0 = ArchiveBuilder::new(11);
    for i in 0..20u64 {
        data0.add_hashed(0x1000_0000u32.wrapping_mul(i as u32 + 1) ^ 0x5555, &noise(i + 1, 8 + (i as usize % 5) * 8));
    }
    let item_dcx = item_msg_dcx(opts.item_msg);
    let item_path = if opts.item_msg == ItemMsg::FrenchOnly { "/msg/FRENCH/item.msgbnd.dcx" } else { ITEM_PATH };
    if opts.collide && opts.item_msg != ItemMsg::Missing {
        data0.add_hashed(crate::hash::path_hash(item_path), b"this is not the file you are looking for");
    }
    if opts.item_msg != ItemMsg::Missing {
        add_encrypted(&mut data0, item_path, &item_dcx, 0x42);
    }
    data0.add("/msg/ENGLISH/menu.msgbnd.dcx", &small_msg_dcx("MenuText.fmg", "Menu"));
    add_encrypted(&mut data0, "/regulation.bin", &noise(77, 3000), 0x17);
    data0.write(&game, "Data0", &k0);

    // Data1: weapon models, under the other key
    let mut data1 = ArchiveBuilder::new(7);
    for (i, path) in MODEL_PATHS.iter().enumerate() {
        let model = path.trim_start_matches("/parts/").trim_end_matches(".partsbnd.dcx");
        if i % 2 == 0 {
            add_encrypted(&mut data1, path, &model_dcx(model), 0x30 + i as u8);
        } else {
            data1.add(path, &model_dcx(model));
        }
    }
    data1.write(&game, "Data1", &k1);

    // DLC1: the DLC item text, under the first key
    let mut dlc1 = ArchiveBuilder::new(3);
    dlc1.add("/msg/ENGLISH/item_dlc1.msgbnd.dcx", &small_msg_dcx("WeaponName_dlc1.fmg", "Dlc"));
    dlc1.write(&game, "DLC1", &k0);

    if opts.broken_archives {
        std::fs::write(game.join("Data9.bhd"), noise(9, 512)).expect("write Data9.bhd"); // no .bdt
        std::fs::write(game.join("Data8.bhd"), noise(8, 1024)).expect("write Data8.bhd");
        std::fs::write(game.join("Data8.bdt"), b"BDF4....").expect("write Data8.bdt");
    }
    FakeDs3 { root: root.to_path_buf(), game, exe }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::Ds3Install;

    #[test]
    fn the_fake_install_opens_and_serves_its_files() {
        let t = tempfile::tempdir().unwrap();
        let fake = build(&t.path().join("DARK SOULS III"), &FakeOptions::default());
        let install = Ds3Install::open(&fake.root, &[]).unwrap();
        assert_eq!(install.game_dir(), fake.game);
        assert_eq!(install.exe().keys.len(), 2);
        assert_eq!((install.exe().pem_blocks, install.exe().pem_rejected), (3, 1), "two keys and one damaged block; the certificate is not counted");
        assert_eq!(install.archives().iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["Data0", "Data1", "DLC1"]);
        assert!(install.archives().iter().all(|a| a.archive.is_ok()));
        assert_eq!(install.open_archives().map(|a| a.key_fingerprint().to_string()).collect::<Vec<_>>(), vec!["87febfc8", "b2969406", "87febfc8"]);
        // the collision: two files under the hash of the item text, the junk one first
        let hits = install.lookup(ITEM_PATH);
        assert_eq!(hits.len(), 2);
        assert_eq!(install.read_hit(&hits[0]).unwrap(), b"this is not the file you are looking for");
        assert_eq!(install.read_hit(&hits[1]).unwrap(), items::item_dcx());
        for path in MODEL_PATHS {
            let model = path.trim_start_matches("/parts/").trim_end_matches(".partsbnd.dcx");
            assert_eq!(install.read(path).unwrap(), model_dcx(model), "{path}");
        }
        assert_eq!(install.read("/regulation.bin").unwrap(), noise(77, 3000));
        assert!(matches!(install.read("/nothing"), Err(crate::install::InstallError::NotInArchives { .. })));
        assert_eq!(install.lookup("msg\\english\\item_dlc1.msgbnd.dcx").len(), 1);
    }

    #[test]
    fn extra_keys_and_the_game_folder_itself_work() {
        let t = tempfile::tempdir().unwrap();
        let opts = FakeOptions { exe_keys: vec![], ..FakeOptions::default() };
        let fake = build(&t.path().join("DS3"), &opts);
        // no keys in the program file: nothing can be opened
        let install = Ds3Install::open(&fake.game, &[]).unwrap();
        assert!(install.exe().keys.is_empty());
        assert!(install.archives().iter().all(|a| a.archive.is_err()));
        assert_eq!(install.open_archives().count(), 0);
        assert!(install.lookup(ITEM_PATH).is_empty());
        // the keys given by the player open them (root = the Game folder here, the Steam folder below)
        let both = [test_key(0).public, test_key(1).public];
        for root in [&fake.game, &fake.root] {
            let install = Ds3Install::open(root, &both).unwrap();
            assert_eq!(install.keys().len(), 2);
            assert_eq!(install.open_archives().count(), 3);
        }
        // only one key: only the archives made with it open
        let install = Ds3Install::open(&fake.root, &[test_key(1).public]).unwrap();
        let state: Vec<(String, bool)> = install.archives().iter().map(|a| (a.name.clone(), a.archive.is_ok())).collect();
        assert_eq!(state, vec![("Data0".to_string(), false), ("Data1".to_string(), true), ("DLC1".to_string(), false)]);
    }

    #[test]
    fn broken_archives_are_listed_with_their_reason_and_the_rest_still_works() {
        let t = tempfile::tempdir().unwrap();
        let fake = build(&t.path().join("DS3"), &FakeOptions { broken_archives: true, ..FakeOptions::default() });
        let install = Ds3Install::open(&fake.root, &[]).unwrap();
        let slots: Vec<(&str, bool, Option<u64>)> = install.archives().iter().map(|a| (a.name.as_str(), a.archive.is_ok(), a.bdt_size)).collect();
        assert_eq!(slots, vec![("Data0", true, slots[0].2), ("Data1", true, slots[1].2), ("Data8", false, Some(8)), ("Data9", false, None), ("DLC1", true, slots[4].2)]);
        let reasons: Vec<String> = install.archives().iter().filter_map(|a| a.archive.as_ref().err().map(|e| e.to_string())).collect();
        assert!(reasons[0].contains("no key matched") && reasons[1].contains("no such file"), "{reasons:?}");
        assert!(reasons.iter().all(|r| !r.contains(fake.root.to_string_lossy().as_ref())), "no folder names in reasons");
        assert_eq!(install.lookup(ITEM_PATH).len(), 2);
    }

    #[test]
    fn nothing_in_the_install_changes_when_it_is_opened() {
        let t = tempfile::tempdir().unwrap();
        let fake = build(&t.path().join("DS3"), &FakeOptions::default());
        let snapshot = |dir: &Path| -> Vec<(String, Vec<u8>)> {
            let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap()).map(|e| (e.file_name().to_string_lossy().to_string(), std::fs::read(e.path()).unwrap())).collect();
            v.sort();
            v
        };
        let before = snapshot(&fake.game);
        let install = Ds3Install::open(&fake.root, &[]).unwrap();
        for path in MODEL_PATHS {
            install.read(path).unwrap();
        }
        install.read(ITEM_PATH).unwrap();
        assert_eq!(snapshot(&fake.game), before);
        assert!(!fake.root.join("Game").join("Data0.bhd.tmp").exists());
    }

    #[test]
    fn a_folder_without_the_program_file_is_refused() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("Game")).unwrap();
        assert!(matches!(Ds3Install::open(t.path(), &[]), Err(crate::install::InstallError::NoExe)));
        assert!(matches!(Ds3Install::open(&t.path().join("missing"), &[]), Err(crate::install::InstallError::NoExe)));
        // names in another letter case are found
        let fake = build(&t.path().join("DS3"), &FakeOptions::default());
        std::fs::rename(&fake.exe, fake.game.join("darksoulsiii.EXE")).unwrap();
        std::fs::rename(&fake.game, fake.root.join("GAME")).unwrap();
        assert!(Ds3Install::open(&fake.root, &[]).is_ok(), "file and folder names are matched without regard to upper and lower case");
    }
}
