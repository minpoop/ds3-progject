//! Runs the model probe against a synthetic Space Marine 2 install: the template files hold a made-up weapon (one square as
//! the full-detail model), and the report must read it with the converter's reader, describe the full-detail model and
//! the textures its material names, and leave the install untouched.
mod common;
use ashen_setup::meshprobe::{report_path, run, Opts};
use ashen_sm2::testing::made_up_weapon;
use common::{descriptor, fake_install, make_zip, tree};
use std::fs;

/// Adds a made-up chainsword template (and the textures its material names) to a fake install.
fn add_weapon(sm2: &std::path::Path) {
    let (tpl, data) = made_up_weapon();
    let red: Vec<u8> = [0x00u8, 0xF8, 0x1F, 0x00, 0, 0, 0, 0].iter().cycle().take(32).copied().collect(); // 8x8 BC1, all red
    make_zip(
        &sm2.join("client_pc/root/paks/client/default/default_tpl_2.pak"),
        &[
            ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl", tpl),
            ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl_data", data),
            ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.lods_base", b"lodInfo = {\n  maxLodDist = [ 10, 30 ]\n}\n".to_vec()),
            ("pct/square_tex.pct.resource", descriptor("square_tex", 12, 8, 2)),
            ("pct/square_tex_nm.pct.resource", descriptor("square_tex_nm", 12, 8, 2)),
            ("pct/square_tex_artificer_01_01_red.pct.resource", descriptor("square_tex_artificer_01_01_red", 12, 8, 2)),
            ("pct/square_tex_1.pct_mip", red.clone()),
            ("pct/square_tex_2.pct_mip", red[..8].to_vec()),
            ("pct/square_tex_nm_1.pct_mip", red.clone()),
            ("pct/square_tex_nm_2.pct_mip", red[..8].to_vec()),
        ],
        true,
    );
}

#[test]
fn the_model_probe_reads_the_template_and_describes_the_full_detail_model_and_its_textures() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("out");
    fake_install(&sm2);
    add_weapon(&sm2);
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
        "the .tpl was read to its last byte",
        "name \"made_up_square\"; 0 bones; 0 animations; 1 detail levels defined [(0, 0)]",
        "geometry: 0 objects, 4 buffers, 1 meshes, 1 sub meshes",
        "full-detail sub mesh 0: object 0 (no name)",
        "4 vertices, 2 triangles; x -2.000..2.000, y -2.000..2.000, z 1.000..1.000",
        "texture coordinates u 0.000..2.000, v -1.000..1.000",
        "carries: normals yes, tangents yes, texture coordinates yes, bone numbers yes, bone weights no",
        "longest side 4.000 m",
        "material: texture \"square_tex\"",
        // the family has the texture and its normal map, not the colour variant
        "texture \"square_tex\": 2 descriptors in its family, 1 more whose names start with it",
        "square_tex.pct.resource",
        "square_tex_nm.pct.resource",
        "OXT1(BC1) 8x8",
        // the pistol folder of the fake install has no data file
        "Model files of the bolt_pistol",
        "the folder has no .tpl and .tpl_data pair",
    ] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }
    assert!(!report.contains("STEP FAILED") && !report.contains("STEP CRASHED") && !report.contains("NOT READ COMPLETELY"), "{report}");
}

#[test]
fn a_template_that_does_not_read_is_named_and_the_run_goes_on() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("out");
    fake_install(&sm2);
    // a template whose start is right and whose rest is wrong, with a data file
    let (mut tpl, data) = made_up_weapon();
    let cut = tpl.len() - 40;
    tpl.truncate(cut);
    make_zip(
        &sm2.join("client_pc/root/paks/client/default/default_tpl_2.pak"),
        &[("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl", tpl), ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl_data", data)],
        true,
    );
    assert!(run(&Opts { sm2: Some(sm2.clone()), out: out.clone() }));
    let report = fs::read_to_string(report_path(&out)).unwrap();
    assert!(report.contains("THE .tpl WAS NOT READ COMPLETELY"), "{report}");
    assert!(report.contains("Model files of the bolt_pistol") && !report.contains("STEP CRASHED"), "{report}");
}

#[test]
fn a_template_without_levels_says_there_is_no_full_detail_model() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("out");
    fake_install(&sm2);
    let (tpl, data) = ashen_sm2::testing::made_up_model();
    make_zip(&sm2.join("client_pc/root/paks/client/default/default_tpl_2.pak"), &[("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl", tpl), ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl_data", data)], true);
    assert!(run(&Opts { sm2: Some(sm2.clone()), out: out.clone() }));
    let report = fs::read_to_string(report_path(&out)).unwrap();
    assert!(report.contains("NO FULL-DETAIL MODEL"), "{report}");
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
