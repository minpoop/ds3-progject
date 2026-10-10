//! A whole synthetic Dark Souls III install: a program file with the test keys among junk, three archives
//! (`Data0` and `DLC1` with test key 0, `Data1` with test key 1) holding fake `item_dlc2.msgbnd.dcx` and `item_dlc1.msgbnd.dcx`
//! (the item text a real, fully updated game has: its archives hold no `item.msgbnd.dcx`), `menu.msgbnd.dcx`,
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
    /// `/msg/engUS/item_dlc2.msgbnd.dcx` and `item_dlc1.msgbnd.dcx` as a fully updated game has them (and no `item.msgbnd.dcx`:
    /// the game asks ModEngine2 for `item_dlc2` and the real archives hold no other).
    Good,
    /// Only `/msg/engUS/item.msgbnd.dcx`: an install without the downloadable content.
    BaseOnly,
    /// Like [`ItemMsg::Good`], but in `item_dlc1` "Shortsword" at id 2000000 is called "Broadsword".
    Dlc1Renamed,
    /// ... but "Shortsword" at id 2000000 is called "Broadsword" (a game update that renamed things).
    WrongName,
    /// There is no item text at all.
    Missing,
    /// Only `/msg/frafr/item.msgbnd.dcx` exists.
    FrenchOnly,
    /// The DCX file is damaged (its checksum fails).
    DamagedDcx,
    /// The BND4 inside is laid out in a way that cannot be patched.
    UnpatchableLayout,
    /// The right item text, but stored under a path nobody expects (`/msg/zzTEXT/item.msgbnd.dcx`): only a look at what the
    /// files contain finds it.
    UnexpectedPath,
    /// ... and with "Shortsword" renamed, so that it is not the text the mod needs.
    UnexpectedPathWrongName,
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
    /// `Data0.bhd` is a plain (not encrypted) table of contents, as in the real game.
    pub plain_data0: bool,
    /// The weapon containers hold files named `.flver` and `.tpf` that are not models or textures (noise).
    pub junk_models: bool,
}

impl Default for FakeOptions {
    fn default() -> Self {
        FakeOptions { exe_keys: vec![0, 1], item_msg: ItemMsg::Good, collide: true, broken_archives: false, plain_data0: false, junk_models: false }
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

/// The English item text a fully updated game asks for.
pub const ITEM_PATH: &str = "/msg/engUS/item_dlc2.msgbnd.dcx";
/// ... and the one of the first downloadable content, which the real archives hold as well.
pub const ITEM_DLC1_PATH: &str = "/msg/engUS/item_dlc1.msgbnd.dcx";
/// ... and the base game's, which the real archives do not hold ([`ItemMsg::BaseOnly`] has only this).
pub const BASE_ITEM_PATH: &str = "/msg/engUS/item.msgbnd.dcx";
/// Where [`ItemMsg::UnexpectedPath`] puts the item text.
pub const UNEXPECTED_ITEM_PATH: &str = "/msg/zzTEXT/item.msgbnd.dcx";
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
    model_dcx_of(model, false)
}

/// [`model_dcx`], or with files named like a model and a texture container that are noise (`junk`).
pub fn model_dcx_of(model: &str, junk: bool) -> Vec<u8> {
    let mut spec = Bnd4Spec::new(0x74);
    let base = format!("N:\\FDP\\data\\INTERROOT_win64\\parts\\weapon\\{model}");
    if junk {
        spec = spec.file(100, &format!("{base}\\{model}.flver"), &noise(model.len() as u64 * 31, 700));
        spec = spec.file(101, &format!("{base}\\{model}.tpf"), &noise(model.len() as u64 * 37, 400));
    } else {
        // a model and a texture container that really parse (made up), so the model report can be tried on them
        let mut flver = super::flver::sample_flver();
        flver.header.bounding_box_max[0] += model.len() as f32;
        spec = spec.file(100, &format!("{base}\\{model}.flver"), &flver.write().expect("write the made-up model"));
        spec = spec.file(101, &format!("{base}\\{model}.tpf"), &super::flver::sample_tpf().write());
        // the real Shortsword has a second model beside the weapon: its scabbard (`WP_A_0200_1.flver`, `WP_A_0200_1_L.flver`)
        if model.starts_with("wp_a_0200") {
            let scabbard = match model.strip_suffix("_l") {
                Some(right) => format!("{right}_1_l"),
                None => format!("{model}_1"),
            };
            let mut sheath = super::flver::sample_flver();
            sheath.header.bounding_box_max[1] += 7.0;
            spec = spec.file(102, &format!("{base}\\{scabbard}.flver"), &sheath.write().expect("write the made-up scabbard"));
        }
    }
    spec = spec.file(103, &format!("{base}\\{model}.hkx"), &noise(model.len() as u64 * 1299709, 120));
    dcx::encode(&spec.build(), &DcxInfo::ds3_default()).expect("encode")
}

fn small_msg_dcx(table_name: &str, text: &str) -> Vec<u8> {
    let mut fmg = FmgFile::new();
    fmg.set(1, text);
    let bnd = Bnd4Spec::new(0x74).file(1, &game_path(table_name), &fmg.to_bytes()).build();
    dcx::encode(&bnd, &DcxInfo::ds3_default()).expect("encode")
}

/// The item container with a made-up file of `pad` bytes in it (the game's real ones are big; a search that skips small
/// files needs the fake to be big, too).
fn padded_item_dcx(tables: &[items::Table], pad: usize) -> Vec<u8> {
    dcx::encode(&bnd4_of(tables, &[(99, "Readme.txt", noise(0xAD, pad))]), &DcxInfo::ds3_default()).expect("encode")
}

/// The DCX bytes of the item text for an option.
pub fn item_msg_dcx(kind: ItemMsg) -> Vec<u8> {
    match kind {
        ItemMsg::Good | ItemMsg::Dlc1Renamed | ItemMsg::BaseOnly | ItemMsg::FrenchOnly => items::item_dcx(),
        ItemMsg::UnexpectedPath => padded_item_dcx(&item_tables(), 6000),
        ItemMsg::UnexpectedPathWrongName => {
            let mut tables = item_tables();
            tables[1].fmg.set(2_000_000, "Broadsword");
            padded_item_dcx(&tables, 6000)
        }
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
    let item_path = match opts.item_msg {
        ItemMsg::FrenchOnly => "/msg/frafr/item.msgbnd.dcx",
        ItemMsg::UnexpectedPath | ItemMsg::UnexpectedPathWrongName => UNEXPECTED_ITEM_PATH,
        ItemMsg::BaseOnly => BASE_ITEM_PATH,
        _ => ITEM_PATH,
    };
    if opts.collide && opts.item_msg != ItemMsg::Missing {
        data0.add_hashed(crate::hash::path_hash(item_path), b"this is not the file you are looking for");
    }
    if opts.item_msg != ItemMsg::Missing {
        add_encrypted(&mut data0, item_path, &item_dcx, 0x42);
    }
    if matches!(opts.item_msg, ItemMsg::Good | ItemMsg::Dlc1Renamed) {
        // the real game's item_dlc1 is another full copy of the text (without the second downloadable content's tables)
        let mut tables = item_tables();
        if opts.item_msg == ItemMsg::Dlc1Renamed {
            tables[1].fmg.set(2_000_000, "Broadsword");
        }
        add_encrypted(&mut data0, ITEM_DLC1_PATH, &padded_item_dcx(&tables, 100), 0x43);
    }
    data0.add("/msg/engUS/menu.msgbnd.dcx", &small_msg_dcx("MenuText.fmg", "Menu"));
    add_encrypted(&mut data0, "/regulation.bin", &noise(77, 3000), 0x17);
    if opts.plain_data0 {
        data0.write_plain(&game, "Data0");
    } else {
        data0.write(&game, "Data0", &k0);
    }

    // Data1: weapon models, under the other key
    let mut data1 = ArchiveBuilder::new(7);
    for (i, path) in MODEL_PATHS.iter().enumerate() {
        let model = path.trim_start_matches("/parts/").trim_end_matches(".partsbnd.dcx");
        if i % 2 == 0 {
            add_encrypted(&mut data1, path, &model_dcx_of(model, opts.junk_models), 0x30 + i as u8);
        } else {
            data1.add(path, &model_dcx_of(model, opts.junk_models));
        }
    }
    data1.write(&game, "Data1", &k1);

    // DLC1: some DLC menu text, under the first key
    let mut dlc1 = ArchiveBuilder::new(3);
    dlc1.add("/msg/engUS/menu_dlc1.msgbnd.dcx", &small_msg_dcx("MenuText_dlc1.fmg", "Dlc"));
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
    use crate::install::{look_at_bhd, BhdLook, Ds3Install};

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
        assert_eq!(install.lookup("msg\\ENGUS\\menu_dlc1.msgbnd.dcx").len(), 1);
        assert_eq!(install.lookup(ITEM_DLC1_PATH).len(), 1);
        assert!(install.lookup(BASE_ITEM_PATH).is_empty(), "the real archives hold no item.msgbnd.dcx");
    }

    #[test]
    fn a_header_no_key_opens_is_described_from_the_outside_and_the_keys_that_fit_are_named() {
        let t = tempfile::tempdir().unwrap();
        let both = [test_key(0).public, test_key(1).public];
        // a header made with key 0 that does not start with BHD5: the archive does not open, but key 0 is the right one
        let mut plain = b"ZZZZ a different kind of header".to_vec();
        plain.resize(600, 7);
        let odd = test_key(0).encrypt_header(&plain);
        assert_eq!(odd.len(), 3 * 256);
        let path = t.path().join("DLC9.bhd");
        std::fs::write(&path, &odd).unwrap();
        let look = look_at_bhd(&path, &both).unwrap();
        assert_eq!((look.size, look.head.len(), look.tail.len(), look.keys_tried), (768, 64, 64, 2));
        assert_eq!(look.head, odd[..64]);
        assert_eq!(look.tail, odd[768 - 64..]);
        assert_eq!(look.whole.as_deref(), Some(&odd[..]), "a small file is kept whole");
        assert_eq!(look.plain_by.len(), 1, "only key 0 fits: {look:?}");
        assert_eq!(look.plain_by[0].0, "87febfc8");
        assert!(look.plain_by[0].1.starts_with(b"ZZZZ a different kind") && look.plain_by[0].1.len() == 32);
        assert!(BhdLook::readable_start(&look.plain_by[0].1) && !BhdLook::readable_start(&[0, 1, 2, 3]) && !BhdLook::readable_start(b"ab"));
        // a file that is not a whole number of blocks, and one shorter than a block
        let mut odd_size = odd.clone();
        odd_size.truncate(700);
        std::fs::write(&path, &odd_size).unwrap();
        assert_eq!(look_at_bhd(&path, &both).unwrap().plain_by.len(), 1, "the first block is all that is tried");
        std::fs::write(&path, [1u8, 2, 3]).unwrap();
        let short = look_at_bhd(&path, &both).unwrap();
        assert_eq!((short.size, short.keys_tried, short.plain_by.len(), short.head.clone(), short.tail.clone()), (3, 0, 0, vec![1, 2, 3], vec![1, 2, 3]));
        // a big file is not kept whole
        std::fs::write(&path, vec![9u8; 5000]).unwrap();
        assert_eq!(look_at_bhd(&path, &both).unwrap().whole, None);
        // nothing there
        assert!(look_at_bhd(&t.path().join("none.bhd"), &both).is_err());
        // the slots carry the path of the file
        let fake = build(&t.path().join("DS3"), &FakeOptions { broken_archives: true, ..FakeOptions::default() });
        let install = Ds3Install::open(&fake.root, &[]).unwrap();
        let broken: Vec<_> = install.archives().iter().filter(|s| s.archive.is_err()).collect();
        assert_eq!(broken.len(), 2);
        for slot in broken {
            assert_eq!(slot.bhd_path, fake.game.join(format!("{}.bhd", slot.name)));
            let look = look_at_bhd(&slot.bhd_path, install.keys()).unwrap();
            assert_eq!(look.size, slot.bhd_size);
        }
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

    /// The plain table of contents of every archive of a fake install, as the running game would hold them.
    fn memory_headers(fake: &FakeDs3) -> Vec<(String, Vec<u8>)> {
        let keys = [("Data0", 0usize), ("Data1", 1), ("DLC1", 0)];
        keys.iter()
            .map(|(name, k)| {
                let bhd = std::fs::read(fake.game.join(format!("{name}.bhd"))).unwrap();
                (name.to_string(), crate::rsa::decrypt_header(&test_key(*k).public, &bhd).unwrap())
            })
            .collect()
    }

    fn save_headers(dir: &Path, headers: &[(String, Vec<u8>)]) {
        std::fs::create_dir_all(dir).unwrap();
        for (name, bytes) in headers {
            std::fs::write(dir.join(format!("{name}.bin")), bytes).unwrap();
        }
    }

    #[test]
    fn saved_plain_headers_open_the_archives_without_any_key() {
        use crate::archive::HeaderSource;
        use crate::install::PlainHeader;
        let t = tempfile::tempdir().unwrap();
        let fake = build(&t.path().join("DS3"), &FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
        let reference = {
            let keyed = Ds3Install::open(&fake.root, &[test_key(0).public, test_key(1).public]).unwrap();
            (keyed.read(ITEM_PATH).unwrap(), keyed.read("/regulation.bin").unwrap(), keyed.read(MODEL_PATHS[2]).unwrap())
        };
        let headers = memory_headers(&fake);
        let dir = t.path().join("cache").join("bhd5");
        save_headers(&dir, &headers);
        std::fs::write(dir.join("notes.txt"), "not a header").unwrap();
        let loaded = PlainHeader::load_dir(&dir);
        assert_eq!(loaded.iter().map(|h| h.label.as_str()).collect::<Vec<_>>(), vec!["DLC1.bin", "Data0.bin", "Data1.bin"], "in name order");

        let exe = crate::install::ExeInfo::scan(&fake.exe).unwrap();
        assert!(exe.keys.is_empty());
        let install = Ds3Install::open_with_sources(fake.game.clone(), exe, &[], &loaded, &mut |_| {}).unwrap();
        assert_eq!(install.open_archives().count(), 3);
        for a in install.open_archives() {
            assert!(matches!(a.source(), HeaderSource::GameMemory(label) if label == &format!("{}.bin", a.name())), "{:?}", a.source());
            assert_eq!(a.key_fingerprint(), "");
            assert_eq!(a.files_outside_bdt(), 0);
        }
        // the files come out the same as with the keys
        assert_eq!(install.lookup(ITEM_PATH).len(), 2);
        assert_eq!(install.read(ITEM_PATH).unwrap(), reference.0, "the first of the two hits");
        assert_eq!(install.read_hit(&install.lookup(ITEM_PATH)[1]).unwrap(), items::item_dcx());
        assert_eq!(install.read("/regulation.bin").unwrap(), reference.1);
        assert_eq!(install.read(MODEL_PATHS[2]).unwrap(), reference.2);
    }

    #[test]
    fn a_saved_header_goes_to_the_archive_it_fits_whatever_it_is_called() {
        use crate::install::PlainHeader;
        let t = tempfile::tempdir().unwrap();
        let fake = build(&t.path().join("DS3"), &FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
        let mut headers = memory_headers(&fake);
        // swap the names of two files and add a copy that is cut off and one that is damaged
        let (a, b) = (headers[0].1.clone(), headers[1].1.clone());
        headers[0].1 = b;
        headers[1].1 = a;
        let mut cut = headers[2].1.clone();
        cut.truncate(cut.len() / 2);
        headers.push(("Cut".to_string(), cut));
        let mut damaged = headers[2].1.clone();
        damaged[0x14] = 0xFF; // the bucket table is nowhere near
        headers.push(("Damaged".to_string(), damaged));
        let dir = t.path().join("bhd5");
        save_headers(&dir, &headers);
        let install = Ds3Install::open_with_sources(fake.game.clone(), crate::install::ExeInfo::scan(&fake.exe).unwrap(), &[], &PlainHeader::load_dir(&dir), &mut |_| {}).unwrap();
        let state: Vec<(&str, bool, String)> = install.archives().iter().map(|a| (a.name.as_str(), a.archive.is_ok(), a.archive.as_ref().map(|x| x.source().to_string()).unwrap_or_default())).collect();
        assert!(state.iter().all(|(_, ok, _)| *ok), "{state:?}");
        assert!(state[0].2.contains("Data1.bin"), "Data0 got the header that is called Data1: {state:?}");
        assert!(state[1].2.contains("Data0.bin"), "{state:?}");
        assert!(state[2].2.contains("DLC1.bin"), "{state:?}");
        assert_eq!(install.lookup(ITEM_PATH).len(), 2, "the files are the right ones");
        assert_eq!(install.read("/regulation.bin").unwrap(), noise(77, 3000));
    }

    #[test]
    fn a_key_is_preferred_to_a_header_and_a_header_that_does_not_fit_is_ignored() {
        use crate::archive::HeaderSource;
        use crate::install::PlainHeader;
        let t = tempfile::tempdir().unwrap();
        let fake = build(&t.path().join("DS3"), &FakeOptions::default());
        let dir = t.path().join("bhd5");
        save_headers(&dir, &memory_headers(&fake));
        let headers = PlainHeader::load_dir(&dir);
        let install = Ds3Install::open_with_sources(fake.game.clone(), crate::install::ExeInfo::scan(&fake.exe).unwrap(), &[], &headers, &mut |_| {}).unwrap();
        assert!(install.open_archives().all(|a| matches!(a.source(), HeaderSource::Key(_))));
        // only the header of Data1 is there, and the keys are missing: Data1 opens, the others report the key problem
        let one: Vec<PlainHeader> = headers.iter().filter(|h| h.label == "Data1.bin").cloned().collect();
        let nokeys = build(&t.path().join("DS3b"), &FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
        let install = Ds3Install::open_with_sources(nokeys.game.clone(), crate::install::ExeInfo::scan(&nokeys.exe).unwrap(), &[], &one, &mut |_| {}).unwrap();
        let state: Vec<(String, bool)> = install.archives().iter().map(|a| (a.name.clone(), a.archive.is_ok())).collect();
        assert_eq!(state, vec![("Data0".to_string(), false), ("Data1".to_string(), true), ("DLC1".to_string(), false)]);
        // a header whose files reach past the .bdt (a different, bigger archive) is not accepted for this one
        let other = tempfile::tempdir().unwrap();
        let big = build(&other.path().join("DS3"), &FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
        std::fs::write(big.game.join("Data1.bdt"), b"BDF4....").unwrap();
        let install = Ds3Install::open_with_sources(big.game.clone(), crate::install::ExeInfo::scan(&big.exe).unwrap(), &[], &one, &mut |_| {}).unwrap();
        assert_eq!(install.open_archives().count(), 0, "every file of Data1 now lies outside its .bdt");
        assert!(PlainHeader::load_dir(&other.path().join("nothing")).is_empty());
    }

    #[test]
    fn a_plain_data0_opens_without_any_key_and_the_other_archives_keep_needing_theirs() {
        use crate::archive::HeaderSource;
        let t = tempfile::tempdir().unwrap();
        let fake = build(&t.path().join("DS3"), &FakeOptions { exe_keys: vec![], plain_data0: true, ..FakeOptions::default() });
        assert!(std::fs::read(fake.game.join("Data0.bhd")).unwrap().starts_with(b"BHD5"));
        let install = Ds3Install::open(&fake.root, &[]).unwrap();
        let state: Vec<(String, bool)> = install.archives().iter().map(|a| (a.name.clone(), a.archive.is_ok())).collect();
        assert_eq!(state, vec![("Data0".to_string(), true), ("Data1".to_string(), false), ("DLC1".to_string(), false)]);
        let data0 = install.archives()[0].archive.as_ref().unwrap();
        assert_eq!(data0.source(), &HeaderSource::Plain);
        assert_eq!((data0.key_fingerprint(), data0.files_outside_bdt()), ("", 0));
        // the item text (which the fake keeps in Data0) is there and reads the same as when the table was encrypted
        let hits = install.lookup(ITEM_PATH);
        assert_eq!(hits.len(), 2);
        assert_eq!(install.read_hit(&hits[1]).unwrap(), items::item_dcx());
        // with the keys the other archives open too
        let all = Ds3Install::open(&fake.root, &[test_key(0).public, test_key(1).public]).unwrap();
        assert_eq!(all.open_archives().count(), 3);
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
