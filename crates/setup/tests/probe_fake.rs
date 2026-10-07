//! Runs the probe against a synthetic Space Marine 2 install built in a temp folder (no game file involved) and
//! checks the report, the pictures, the sample sound - and that the install is byte-for-byte untouched.
mod common;
use ashen_setup::probe::{run, Opts};
use common::{fake_install, tree};
use std::fs;

#[test]
fn probe_reads_a_synthetic_install_and_leaves_it_untouched() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("out");
    fake_install(&sm2);
    let before = tree(&sm2);

    assert!(run(&Opts { sm2: Some(sm2.clone()), out: out.clone() }));

    assert_eq!(tree(&sm2), before, "the probe must not change anything in the install");
    let report = fs::read_to_string(out.join("probe-report.txt")).unwrap();
    if std::env::var_os("ASHEN_SHOW_REPORT").is_some() {
        println!("{report}");
    }
    for needle in [
        "opened 4 archives",
        "chainsword: 4 template files in 1 template folders",
        "files of  tpl/wpn_chainsword_01.tpl",
        "wpn_chainsword_01.lods_base  300 B",
        "text of wpn_chainsword_01.tpl_markup",
        "descriptor text of pct/wpn_chainsword_01.pct.resource",
        "__type: res_desc_pct",
        "OXT1(BC1) 8x8 (mip 0 of 2)",
        "average colour rgba(255,0,0,255)",
        "average colour rgba(0,0,255,255)",
        "mention ssl/weapons/melee/weapon_actors/wpn_melee_chainsword.cls",
        "full text of ssl/weapons/melee/weapon_actors/wpn_melee_chainsword.cls",
        "damage   =   42.5",
        "magazine   =   14",
        "weapon bank sounds/desktop/wpn.bnk: Wwise bank version 150",
        "guessed weapon event names that exist: 0",
        "sound events about the chainsword: 1",
        "wpn_melee_chainsword_swing",
        "sound events about the bolt pistol: 1",
        "sound events about pistols and firearms in general: 1",
        "wpn_firearm_shoot_2d_bolt_pistol",
        "1 zip files inside the paks",
        // 13 sound files are named by the weapon bank; 11 are in its zip (one is stored in the bank, one is missing)
        "weapon bank: 13 distinct media ids are referenced; 11 of them are in the zips",
        "codec Wwise Vorbis (0xFFFF): 4 files",
        "codec PCM (0x0001): 7 files",
        "kept 2 small sound clips",
    ] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }
    assert!(!report.contains("STEP FAILED") && !report.contains("STEP CRASHED"), "{report}");
    assert!(out.join("textures/wpn_chainsword_01.png").is_file());
    assert!(out.join("textures/index.html").is_file());
    let samples: Vec<String> = fs::read_dir(out.join("samples")).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert!(samples.iter().any(|n| n.starts_with("wpn_melee_chainsword_swing__") && n.ends_with(".wem")), "{samples:?}");
    assert!(samples.iter().any(|n| n == "index.txt"), "{samples:?}");
}

#[test]
fn probe_reports_a_missing_install_instead_of_crashing() {
    let t = tempfile::tempdir().unwrap();
    let empty = t.path().join("nothing");
    fs::create_dir_all(&empty).unwrap();
    let out = t.path().join("out");
    assert!(!run(&Opts { sm2: Some(empty), out: out.clone() }));
    let report = fs::read_to_string(out.join("probe-report.txt")).unwrap();
    assert!(report.contains("THIS STEP FAILED") && report.contains("no .pak files found"), "{report}");
}
