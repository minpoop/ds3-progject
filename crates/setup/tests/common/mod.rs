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

/// A texture descriptor in the shape the real game uses (`res_desc_pct`, one data file per mip, `_1` is the top mip).
fn descriptor(name: &str, format: u32, size: u32, mips: usize) -> Vec<u8> {
    let block = if format == 51 || format == 36 { 16u64 } else { 8 };
    let mut levels = Vec::new();
    let (mut off, mut edge) = (0u64, size);
    for _ in 0..mips {
        let n = (edge.div_ceil(4) as u64).pow(2) * block;
        levels.push((off, n));
        off += n;
        edge = (edge / 2).max(1);
    }
    let mut s = format!("__type: res_desc_pct\ndownsampled: true\nheader:\n  faceSize: {off}\n  format: {format}\n  mipLevel:\n");
    for (o, n) in &levels {
        s += &format!("  - offset: {o}\n    size: {n}\n");
    }
    s += &format!("  nFaces: 1\n  nMipMap: {mips}\n  sign: 1346978644\n  size: {off}\n  sx: {size}\n  sy: {size}\n  sz: 1\nlinkTd: res://td/{name}.td.resource\nmipMaps:\n");
    for i in 1..=mips {
        s += &format!("- {name}_{i}.pct_mip\n");
    }
    s += &format!("texName: {name}\ntexType: ''\n");
    s.into_bytes()
}

/// The Wwise Vorbis header bytes of a real SM2 weapon sound (the 50 bytes after the 16 standard fmt bytes).
const REAL_VORBIS_FMT_EXTRA: [u8; 50] = [
    0x30, 0x00, 0x00, 0x00, 0x01, 0x41, 0x00, 0x00, 0x80, 0xa9, 0x03, 0x00, 0xcb, 0x00, 0x00, 0x00, 0x16, 0xd3, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x78, 0x03, 0x00, 0x00, 0x43, 0x04, 0x00, 0x00, 0xcd, 0x00, 0x00, 0x00, 0x04, 0x47, 0x00, 0x00, 0xcc, 0x48,
    0x00, 0x00, 0x2f, 0x7c, 0x1e, 0x08, 0x00, 0x00,
];

pub fn fake_install(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("Warhammer 40000 Space Marine 2.exe"), b"MZ").unwrap();
    let paks = root.join("client_pc/root/paks/client");
    let red: Vec<u8> = [0x00u8, 0xF8, 0x1F, 0x00, 0, 0, 0, 0].iter().cycle().take(32).copied().collect(); // 8x8 BC1, all red
    let blue_block = [0x00u8, 0xF8, 0x1F, 0x00, 0x55, 0x55, 0x55, 0x55];
    let blue: Vec<u8> = (0..4).flat_map(|_| blue_block).collect(); // 8x8 BC1, all blue

    make_zip(
        &paks.join("resources.pak"),
        &[
            ("pct/wpn_chainsword_01.pct.resource", descriptor("wpn_chainsword_01", 12, 8, 2)),
            ("pct/wpn_bolt_pistol_01_artificer_01.pct.resource", descriptor("wpn_bolt_pistol_01_artificer_01", 12, 8, 2)),
            ("tpl/wpn_chainsword_01.tpl/wpn_chainsword_01.tpl.resource", b"__type: res_desc_tpl\nname: wpn_chainsword_01\n".to_vec()),
        ],
        false,
    );
    make_zip(
        &paks.join("default/default_tpl_1.pak"),
        &[
            ("tpl/wpn_chainsword_01.tpl/wpn_chainsword_01.tpl", vec![1u8; 100]),
            ("tpl/wpn_chainsword_01.tpl/wpn_chainsword_01.lods_base", vec![2u8; 300]),
            ("tpl/wpn_chainsword_01.tpl/wpn_chainsword_01.tpl_markup", b"<markup>\n  <part name=\"base_01\"/>\n</markup>\n".to_vec()),
            ("tpl/wpn_bolt_pistol_01.tpl/wpn_bolt_pistol_01.tpl", vec![3u8; 50]),
            ("pct/wpn_chainsword_01_1.pct_mip", red.clone()),
            ("pct/wpn_chainsword_01_2.pct_mip", red[..8].to_vec()),
            ("pct/wpn_bolt_pistol_01_artificer_01_1.pct_mip", blue),
            ("pct/wpn_bolt_pistol_01_artificer_01_2.pct_mip", blue_block.to_vec()),
        ],
        true,
    );
    let cls = "properties   =   {\n   damage   =   42.5\n   swing_sound   =   \"Wpn_Melee_Chainsword_Swing\"\n}\n__type   =   \"wpn_melee_chainsword\"\n";
    make_zip(
        &paks.join("default/default_other.pak"),
        &[
            ("ssl/weapons/melee/weapon_actors/wpn_melee_chainsword.cls", cls.as_bytes().to_vec()),
            ("ssl/weapons/common/firearm/firearm_versions/hgun_bolt_pistol/hgun_bolt_pistol_authority.cls", b"properties   =   {\n   magazine   =   14\n}\n__type   =   \"hgun_bolt_pistol_authority\"\n".to_vec()),
            ("ssl/weapons/common/firearm/firearm_versions/hgun_bolt_pistol/hgun_bolt_pistol_artificer_01_authority.cls", b"properties   =   {\n}\n".to_vec()),
            ("sounds/stats/editor.test.scn.windows.events.csv", b"test.scn,wpn,wpn_firearm_shoot_2d_bolt_pistol,1,100,100,0.4\ntest.scn,wpn,wpn_melee_chainsword_swing,1,100,100,0.5\ntest.scn,wpn,unrelated_event,1,1,1,1\n".to_vec()),
        ],
        false,
    );

    // a weapon bank like the real one: only the object table (no DIDX/DATA); every media file is in the bank's zip
    let mut b = Builder::new(150);
    b.sound(100, 777, 0).sound(101, 888, 0).sound(102, 889, 1).container(5, 200, &[100, 102]);
    b.action(300, 0x0403, 200).action(301, 0x0403, 101);
    b.event(fnv1_lower("wpn_melee_chainsword_swing"), &[300]).event(fnv1_lower("wpn_firearm_shoot_2d_bolt_pistol"), &[301]);
    let bank = b.build();
    let vorbis = test_wem(0xFFFF, 1, 48000, 0, &REAL_VORBIS_FMT_EXTRA, &[7u8; 900]);
    let pcm = test_wem(1, 1, 22050, 16, &[], &vec![0u8; 4410]);
    let media_zip = tempfile::NamedTempFile::new().unwrap();
    make_zip(media_zip.path(), &[("777.wem", vorbis.clone()), ("888.wem", pcm), ("889.wem", vorbis), ("123456.wem", vec![0u8; 10])], true);
    make_zip(&paks.join("default/default_sound_0.pak"), &[("sounds/desktop/wpn.bnk", bank), ("sounds/desktop/wpn.zip", fs::read(media_zip.path()).unwrap())], true);
}
