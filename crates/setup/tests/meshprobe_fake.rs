//! Runs the model probe against a synthetic Space Marine 2 install: the template files hold made-up geometry, and the
//! report must describe the files, find the buffers and leave the install untouched.
mod common;
use ashen_setup::meshprobe::{report_path, run, Opts};
use common::{fake_install, make_zip, tree};
use std::fs;

/// A 30 x 30 sheet as vertices (32 bytes each: position, packed normal, packed uv, padding) and 16-bit triangle indices.
fn geometry() -> (Vec<u8>, Vec<u8>) {
    let n = 30usize;
    let mut verts = Vec::new();
    for y in 0..n {
        for x in 0..n {
            for c in [0.01 + x as f32 * 0.03, 0.01 + y as f32 * 0.03, 0.04 * ((x + y) as f32 * 0.5 + 0.3).sin()] {
                verts.extend(c.to_le_bytes());
            }
            verts.extend([0x80u8, 0x80, 0xFF, 0x7F, 0x12, 0x34, 0x56, 0x78]);
            verts.extend([0u8; 12]);
        }
    }
    let mut idx = Vec::new();
    for y in 0..n - 1 {
        for x in 0..n - 1 {
            let a = (y * n + x) as u16;
            let (b, c) = (a + 1, a + n as u16);
            idx.extend([a, b, c, b, c + 1, c].iter().flat_map(|i| i.to_le_bytes()));
        }
    }
    (verts, idx)
}

#[test]
fn the_model_probe_describes_the_template_files_and_finds_the_buffers() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("out");
    fake_install(&sm2);
    // a second pak with a model data file in the real game's shape: a text descriptor, a "1SER" file, and a data file
    let (verts, idx) = geometry();
    let mut data = vec![0u8; 256];
    data.extend(&verts);
    data.extend(vec![0u8; 64]);
    data.extend(&idx);
    let mut tpl = b"1SERtpl\0".to_vec();
    tpl.extend(vec![0u8; 28]);
    tpl.extend(b"S3DRESO\0objGEOM_MNG\0objGEOM_VBUFFER_INFO\0\x01\x02\x03\x04");
    make_zip(
        &sm2.join("client_pc/root/paks/client/default/default_tpl_2.pak"),
        &[
            ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl", tpl),
            ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl_data", data),
            ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.lods_base", b"lodInfo = {\n  maxLodDist = [ 10, 30 ]\n}\n".to_vec()),
        ],
        true,
    );
    let before = tree(&sm2);

    assert!(run(&Opts { sm2: Some(sm2.clone()), out: out.clone() }));

    assert_eq!(tree(&sm2), before, "the probe must not change anything in the install");
    let report = fs::read_to_string(report_path(&out)).unwrap();
    if std::env::var_os("ASHEN_SHOW_REPORT").is_some() {
        println!("{report}");
    }
    for needle in [
        "Model files of the chainsword",
        "template folder  tpl/wpn_chainsword_00.tpl  with 3 files",
        "wpn_chainsword_00.tpl_data",
        "binary, ",
        "31 53 45 52 74 70 6c 00", // "1SERtpl"
        "objGEOM_MNG",
        "around \"objGEOM_MNG\"",
        "text, ", // the descriptor
        "maxLodDist = [ 10, 30 ]",
        "stride 32",
        "900 records", // the vertices of the 30 x 30 sheet
        "3 x float32",
        "16 bit",
        "1682 triangles", // 2 x 29 x 29
        "could belong with: 3 x float32",
        "Model files of the bolt_pistol",
    ] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }
    assert!(!report.contains("STEP FAILED") && !report.contains("STEP CRASHED"), "{report}");
}

#[test]
fn without_space_marine_2_it_says_so_and_returns_false() {
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("out");
    assert!(!run(&Opts { sm2: Some(t.path().join("nowhere")), out: out.clone() }));
    let report = fs::read_to_string(report_path(&out)).unwrap();
    assert!(report.contains("PROBLEM"), "{report}");
}

#[test]
fn a_report_folder_inside_the_game_is_refused_and_nothing_is_created_there() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    fake_install(&sm2);
    let before = tree(&sm2);
    let inside = sm2.join("client_pc").join("reports");
    assert!(!run(&Opts { sm2: Some(sm2.clone()), out: inside.clone() }));
    assert_eq!(tree(&sm2), before);
    assert!(!inside.exists());
}
