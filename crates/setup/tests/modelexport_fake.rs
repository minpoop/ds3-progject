//! `sm2-export-models` against a synthetic Space Marine 2 install: it copies the template files of the two weapons to a
//! folder, byte for byte, and never touches the install or writes inside it.
mod common;
use ashen_setup::modelexport::{report_path, run, Opts};
use ashen_sm2::testing::made_up_weapon;
use common::{descriptor, fake_install, make_zip, tree};
use std::fs;

fn install_with_models(root: &std::path::Path) -> Vec<(&'static str, Vec<u8>)> {
    fake_install(root);
    let files: Vec<(&'static str, Vec<u8>)> = vec![
        ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl", b"1SERtpl\0 made-up template".to_vec()),
        ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl_data", (0..5000u32).map(|i| (i * 7) as u8).collect()),
        ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.lods_base", b"lodInfo = {\n  maxLodDist = [ 10, 30 ]\n}\n".to_vec()),
        ("tpl/wpn_bolt_pistol_00.tpl/wpn_bolt_pistol_00.tpl", b"1SERtpl\0 pistol".to_vec()),
        ("tpl/wpn_bolt_pistol_00.tpl/wpn_bolt_pistol_00.tpl_data", vec![0xAB; 777]),
        ("tpl/wpn_thunder_hammer_00.tpl/wpn_thunder_hammer_00.tpl", b"not wanted".to_vec()),
    ];
    make_zip(&root.join("client_pc/root/paks/client/default/default_tpl_2.pak"), &files, true);
    files
}

#[test]
fn the_two_weapons_template_files_are_copied_exactly_and_nothing_else() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("kit").join("model-files");
    let files = install_with_models(&sm2);
    let before = tree(&sm2);

    assert!(run(&Opts { sm2: Some(sm2.clone()), out: out.clone() }));

    assert_eq!(tree(&sm2), before, "the install is untouched");
    for (name, bytes) in files.iter().filter(|(n, _)| !n.contains("thunder")) {
        let rel = name.trim_start_matches("tpl/");
        assert_eq!(&fs::read(out.join(rel)).unwrap(), bytes, "{name}");
    }
    assert!(!out.join("wpn_thunder_hammer_00.tpl").exists(), "only the chainsword and the bolt pistol");
    let report = fs::read_to_string(report_path(&out)).unwrap();
    for needle in ["template folder  tpl/wpn_chainsword_00.tpl  with 3 files", "template folder  tpl/wpn_bolt_pistol_00.tpl  with 2 files", "wpn_chainsword_00.tpl_data", "sha256 ", "Copied 5 files"] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }
    assert!(!report.contains("STEP FAILED") && !report.contains("STEP CRASHED") && !report.contains("PROBLEM"), "{report}");
}

#[test]
fn the_textures_the_full_detail_material_names_are_copied_with_their_data_files() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("kit").join("model-files");
    fake_install(&sm2);
    let (tpl, data) = made_up_weapon();
    let red: Vec<u8> = [0x00u8, 0xF8, 0x1F, 0x00, 0, 0, 0, 0].iter().cycle().take(32).copied().collect();
    let entries: Vec<(&str, Vec<u8>)> = vec![
        ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl", tpl.clone()),
        ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl_data", data.clone()),
        ("pct/square_tex.pct.resource", descriptor("square_tex", 12, 8, 2)),
        ("pct/square_tex_nm.pct.resource", descriptor("square_tex_nm", 12, 8, 2)),
        ("pct/square_tex_artificer_01_01_red.pct.resource", descriptor("square_tex_artificer_01_01_red", 12, 8, 2)),
        ("pct/square_tex_1.pct_mip", red.clone()),
        ("pct/square_tex_2.pct_mip", red[..8].to_vec()),
        ("pct/square_tex_nm_1.pct_mip", red.clone()),
        // the second size of the normal map is not shipped
        ("pct/square_tex_artificer_01_01_red_1.pct_mip", red.clone()),
    ];
    make_zip(&sm2.join("client_pc/root/paks/client/default/default_tpl_2.pak"), &entries, true);
    let before = tree(&sm2);

    assert!(run(&Opts { sm2: Some(sm2.clone()), out: out.clone() }));

    assert_eq!(tree(&sm2), before, "the install is untouched");
    let dir = out.join("wpn_chainsword_00.tpl");
    assert_eq!(fs::read(dir.join("wpn_chainsword_00.tpl")).unwrap(), tpl);
    assert_eq!(fs::read(dir.join("wpn_chainsword_00.tpl_data")).unwrap(), data);
    assert_eq!(fs::read(dir.join("pct/square_tex_1.pct_mip")).unwrap(), red);
    assert_eq!(fs::read(dir.join("pct/square_tex_2.pct_mip")).unwrap(), red[..8].to_vec());
    assert!(dir.join("pct/square_tex.pct.resource").exists() && dir.join("pct/square_tex_nm.pct.resource").exists() && dir.join("pct/square_tex_nm_1.pct_mip").exists());
    assert!(!dir.join("pct/square_tex_artificer_01_01_red.pct.resource").exists(), "colour variants are not copied");
    let report = fs::read_to_string(report_path(&out)).unwrap();
    for needle in ["texture \"square_tex\": 2 descriptors in its family", "square_tex_nm_2.pct_mip: not in any pak", "square_tex.pct.resource", "3 files"] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }
    assert!(!report.contains("STEP FAILED") && !report.contains("STEP CRASHED"), "{report}");
}

#[test]
fn an_output_folder_inside_the_game_is_refused_and_a_missing_game_is_told() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    install_with_models(&sm2);
    let before = tree(&sm2);
    let inside = sm2.join("client_pc").join("model-files");
    assert!(!run(&Opts { sm2: Some(sm2.clone()), out: inside.clone() }));
    assert_eq!(tree(&sm2), before, "nothing was created in the install");
    assert!(!inside.exists());

    let elsewhere = t.path().join("out2");
    assert!(!run(&Opts { sm2: Some(t.path().join("nowhere")), out: elsewhere.clone() }));
    assert!(fs::read_to_string(report_path(&elsewhere)).unwrap().contains("PROBLEM"));
}

#[test]
fn a_game_without_the_weapons_copies_nothing_and_says_so() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    fs::create_dir_all(sm2.join("client_pc/root/paks/client/default")).unwrap();
    make_zip(&sm2.join("client_pc/root/paks/client/default/default_other.pak"), &[("ssl/readme.cls", b"nothing about weapons".to_vec())], true);
    let out = t.path().join("out");
    assert!(!run(&Opts { sm2: Some(sm2), out: out.clone() }));
    let report = fs::read_to_string(report_path(&out)).unwrap();
    assert!(report.contains("no template folder with \"chainsword\"") && report.contains("Nothing was copied"), "{report}");
}
