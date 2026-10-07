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
        "chainsword: 2 template files in 1 template folders",
        "files of  tpl/wpn_chainsword_01",
        "wpn_chainsword_01.lods_base  300 B",
        "descriptor text of pct/wpn_chainsword_01_d_0.pct.resource",
        "OXT1(BC1) 8x8",
        "average colour rgba(255,0,0,255)",
        "average colour rgba(0,0,255,255)",
        "mention ssl/weapons/chainsword/chainsword_01.cls",
        "weapon bank sounds/desktop/wpn.bnk: Wwise bank version 150",
        "guessed weapon event names that exist: 2",
        "play_chainsword_swing",
        "1 zip files inside the paks",
        "weapon bank streamed sounds found in the zips: 1 of 1",
        "codec PCM (0x0001): 1 files",
        "event play_chainsword_swing: media 777 decoded",
        "event play_bolt_pistol_fire: media 888 decoded",
    ] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }
    assert!(!report.contains("STEP FAILED") && !report.contains("STEP CRASHED"), "{report}");
    assert!(out.join("textures/wpn_chainsword_01_d_0.png").is_file());
    assert!(out.join("textures/index.html").is_file());
    let wavs: Vec<_> = fs::read_dir(out.join("sounds")).unwrap().filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "wav")).collect();
    assert_eq!(wavs.len(), 2);
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
