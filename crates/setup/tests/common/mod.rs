//! A synthetic Space Marine 2 install (no game file involved) shared by the tests and the Wine test.
#![allow(dead_code)]
use ashen_sm2::bnk::{fnv1_lower, testbank::Builder};
use ashen_sm2::wem::test_wem;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub fn make_zip(path: &Path, entries: &[(&str, Vec<u8>)], stored: bool) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut z = ZipWriter::new(fs::File::create(path).unwrap());
    for (name, data) in entries {
        let m = if stored { CompressionMethod::Stored } else { CompressionMethod::Deflated };
        z.start_file(*name, SimpleFileOptions::default().compression_method(m)).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap();
}

pub fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.insert(p.strip_prefix(root).unwrap().to_path_buf(), fs::read(&p).unwrap());
            }
        }
    }
    let mut m = BTreeMap::new();
    walk(root, root, &mut m);
    m
}

fn descriptor(mips: &[&str], levels: &[(u64, u64)]) -> Vec<u8> {
    let mut s = format!("header:\n  format: 12\n  nMipMap: {}\n  sx: 8\n  sy: 8\n  mipLevel:\n", levels.len());
    for (o, n) in levels {
        s += &format!("  - offset: {o}\n    size: {n}\n");
    }
    s += "mipMaps:\n";
    for m in mips {
        s += &format!("- {m}\n");
    }
    s.into_bytes()
}

pub fn fake_install(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("Warhammer 40000 Space Marine 2.exe"), b"MZ").unwrap();
    let paks = root.join("client_pc/root/paks/client");
    let red: Vec<u8> = [0x00u8, 0xF8, 0x1F, 0x00, 0, 0, 0, 0].iter().cycle().take(32).copied().collect();
    let blue_block = [0x00u8, 0xF8, 0x1F, 0x00, 0x55, 0x55, 0x55, 0x55];

    make_zip(
        &paks.join("resources.pak"),
        &[
            ("pct/wpn_chainsword_01_d_0.pct.resource", descriptor(&["wpn_chainsword_01_d_0.pct_mip"], &[(0, 32)])),
            ("pct/wpn_bolt_pistol_01_d_0.pct.resource", descriptor(&["wpn_bolt_pistol_01_d_0.pct_mip"], &[(0, 32), (32, 8)])),
        ],
        false,
    );
    let mut all_mips: Vec<u8> = (0..4).flat_map(|_| blue_block).collect();
    all_mips.extend_from_slice(&blue_block); // the 4x4 mip after the 8x8 one
    make_zip(
        &paks.join("default/default_tpl_0.pak"),
        &[
            ("tpl/wpn_chainsword_01/wpn_chainsword_01.tpl", vec![1u8; 100]),
            ("tpl/wpn_chainsword_01/wpn_chainsword_01.lods_base", vec![2u8; 300]),
            ("tpl/wpn_bolt_pistol_01/wpn_bolt_pistol_01.tpl", vec![3u8; 50]),
            ("pct/wpn_chainsword_01_d_0.pct_mip", red),
            ("pct/wpn_bolt_pistol_01_d_0.pct_mip", all_mips),
        ],
        true,
    );
    make_zip(
        &paks.join("default/default_other.pak"),
        &[("ssl/weapons/chainsword/chainsword_01.cls", b"{\n  name: wpn_chainsword_01\n  damage: 42.5\n  swing_sound: Play_Chainsword_Swing\n}\n".to_vec())],
        false,
    );

    // a weapon bank: swing is streamed (wem in the nested zip), the pistol shot is stored in the bank
    let mut b = Builder::new(150);
    b.sound(100, 777, 1).sound(101, 888, 0).container(5, 200, &[100]);
    b.action(300, 0x0403, 200).action(301, 0x0403, 101);
    b.event(fnv1_lower("play_chainsword_swing"), &[300]).event(fnv1_lower("play_bolt_pistol_fire"), &[301]);
    b.media(888, &test_wem(1, 1, 22050, 16, &[], &vec![0u8; 4410]));
    let bank = b.build();
    let swing = test_wem(1, 2, 44100, 16, &[], &(0..8820).map(|i| (i % 200) as u8).collect::<Vec<_>>());
    let media_zip = tempfile::NamedTempFile::new().unwrap();
    make_zip(media_zip.path(), &[("777.wem", swing), ("999.wem", test_wem(0xFFFF, 2, 48000, 0, &[0u8; 8], &[1, 2, 3, 4]))], false);
    make_zip(&paks.join("default/default_sound_0.pak"), &[("sounds/desktop/wpn.bnk", bank), ("sounds/media/wpn_media.zip", fs::read(media_zip.path()).unwrap())], true);
}

