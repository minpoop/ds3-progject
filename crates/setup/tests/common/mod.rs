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
pub fn descriptor(name: &str, format: u32, size: u32, mips: usize) -> Vec<u8> {
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

    let (wpn_bnk, wpn_zip) = weapon_sounds();
    make_zip(
        &paks.join("default/default_sound_0.pak"),
        &[("sounds/desktop/wpn.bnk", wpn_bnk), ("sounds/desktop/wpn.zip", wpn_zip), ("sounds/desktop/wpn_melee.bnk", melee_bank())],
        true,
    );
}

/// A plain 16-bit PCM wem (format tag 1) of `frames` identical frames; `frame` holds one value per channel.
pub fn pcm_wem(rate: u32, frame: &[i16], frames: usize) -> Vec<u8> {
    let data: Vec<u8> = (0..frames).flat_map(|_| frame.iter().flat_map(|s| s.to_le_bytes())).collect();
    test_wem(1, frame.len() as u16, rate, 16, &[], &data)
}

/// A Wwise Vorbis wem that is cut off: it reads as Vorbis but cannot be decoded.
pub fn damaged_vorbis() -> Vec<u8> {
    test_wem(0xFFFF, 2, 48000, 0, &[0u8; 8], &[1, 2, 3, 4])
}

/// The fake weapon bank and the stored zip of its sound files. Like the real bank it holds the object table only and
/// the sound files are in the zip - except one, to cover a sound that is stored in the bank. The events (the names are the
/// real ones of design/sheets/sounds.json, hashed at build time):
///
/// * `wpn_melee_chainsword_swing`: a random container over two (undecodable) Vorbis sounds; older than `prepare`, the probe test counts it
/// * `wpn_firearm_shoot_2d_bolt_pistol`: a LAYER of two sounds: mono 1000 for 0.1 s (stream type 0 but not in the bank: found in the
///   zip) and stereo 2000/-2000 for 0.2 s. Mixed: 3000/-1000 for 0.1 s, then 2000/-2000.
/// * `wpn_melee_chswd_light_1hit`: a LAYER of a random container over three sounds (1000 for 0.1 s, 2000 for 0.2 s, 3000 for 0.3 s)
///   and a fourth, streamed with a prefetch (100 for 0.15 s). Takes: 1100, 2100 or 3100 first, 0.15 / 0.2 / 0.3 s long.
/// * `wpn_melee_chswd_light_2hit`: a LAYER of a good sound (1500, 0.1 s), a sound whose file is not in the zip and one that is damaged
/// * `wpn_melee_chswd_light_3hit`: one sound that is damaged
/// * `wpn_melee_chswd_idle_loop`: one sound stored in the bank (500 for 0.2 s), plus a STOP action aimed at a swing sound
///   (which must not be played)
///
/// Returns `(wpn.bnk, wpn.zip)`.
pub fn weapon_sounds() -> (Vec<u8>, Vec<u8>) {
    let mut b = Builder::new(150);
    b.sound(100, 777, 0).sound(101, 888, 0).sound(102, 889, 1).container(5, 200, &[100, 102]);
    b.action(300, 0x0403, 200);
    b.event(fnv1_lower("wpn_melee_chainsword_swing"), &[300]);

    b.sound(118, 7009, 1).container(9, 213, &[101, 118]).action(301, 0x0403, 213);
    b.event(fnv1_lower("wpn_firearm_shoot_2d_bolt_pistol"), &[301]);

    b.sound(110, 7001, 1).sound(111, 7002, 1).sound(112, 7003, 1).sound(113, 7004, 2);
    b.container(5, 210, &[110, 111, 112]).container(9, 211, &[210, 113]).action(310, 0x0403, 211);
    b.event(fnv1_lower("wpn_melee_chswd_light_1hit"), &[310]);

    b.sound(114, 7005, 1).sound(115, 7006, 1).sound(116, 7007, 1).container(9, 212, &[114, 115, 116]).action(311, 0x0403, 212);
    b.event(fnv1_lower("wpn_melee_chswd_light_2hit"), &[311]);

    b.sound(117, 7008, 1).action(312, 0x0403, 117);
    b.event(fnv1_lower("wpn_melee_chswd_light_3hit"), &[312]);

    b.sound(119, 7010, 0).media(7010, &pcm_wem(44100, &[500], 8820));
    b.action(313, 0x0403, 119).action(314, 0x0102, 111);
    b.event(fnv1_lower("wpn_melee_chswd_idle_loop"), &[313, 314]);

    let vorbis = test_wem(0xFFFF, 1, 48000, 0, &REAL_VORBIS_FMT_EXTRA, &[7u8; 900]);
    let zip = tempfile::NamedTempFile::new().unwrap();
    make_zip(
        zip.path(),
        &[
            ("777.wem", vorbis.clone()),
            ("888.wem", pcm_wem(44100, &[1000], 4410)),
            ("889.wem", vorbis),
            ("123456.wem", vec![0u8; 10]),
            ("7001.wem", pcm_wem(44100, &[1000], 4410)),
            ("7002.wem", pcm_wem(44100, &[2000], 8820)),
            ("7003.wem", pcm_wem(44100, &[3000], 13230)),
            ("7004.wem", pcm_wem(44100, &[100], 6615)),
            ("7005.wem", pcm_wem(44100, &[1500], 4410)),
            ("7007.wem", damaged_vorbis()),
            ("7008.wem", damaged_vorbis()),
            ("7009.wem", pcm_wem(44100, &[2000, -2000], 8820)),
        ],
        true,
    );
    (b.build(), fs::read(zip.path()).unwrap())
}

/// A second bank next to the weapon bank, holding an event of the sheet (`wpn_melee_chswd_light_4hit`) that the weapon bank lacks.
pub fn melee_bank() -> Vec<u8> {
    let mut b = Builder::new(150);
    b.sound(1, 9001, 1).action(2, 0x0403, 1).event(fnv1_lower("wpn_melee_chswd_light_4hit"), &[2]);
    b.build()
}

/// An install with nothing but a weapon bank and the zip of its sound files (for tests of how `prepare` copes with trouble).
pub fn sound_install(root: &Path, bank: Vec<u8>, wems: &[(&str, Vec<u8>)]) {
    fs::create_dir_all(root).unwrap();
    let zip = tempfile::NamedTempFile::new().unwrap();
    make_zip(zip.path(), wems, true);
    make_zip(&root.join("client_pc/root/paks/client/default/default_sound_0.pak"), &[("sounds/desktop/wpn.bnk", bank), ("sounds/desktop/wpn.zip", fs::read(zip.path()).unwrap())], true);
}

/// Adds made-up templates of a chainsword and a bolt pistol (one square each, full-detail level defined) and the picture
/// their material names (an 8x8 red picture) to a fake install.
pub fn add_made_up_weapons(sm2: &Path) {
    let (tpl, data) = ashen_sm2::testing::made_up_weapon();
    let red: Vec<u8> = [0x00u8, 0xF8, 0x1F, 0x00, 0, 0, 0, 0].iter().cycle().take(32).copied().collect();
    make_zip(
        &sm2.join("client_pc/root/paks/client/default/default_tpl_2.pak"),
        &[
            ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl", tpl.clone()),
            ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl_data", data.clone()),
            ("tpl/wpn_bolt_pistol_00.tpl/wpn_bolt_pistol_00.tpl", tpl),
            ("tpl/wpn_bolt_pistol_00.tpl/wpn_bolt_pistol_00.tpl_data", data),
            ("pct/square_tex.pct.resource", descriptor("square_tex", 12, 8, 2)),
            ("pct/square_tex_1.pct_mip", red.clone()),
            ("pct/square_tex_2.pct_mip", red[..8].to_vec()),
        ],
        true,
    );
}
