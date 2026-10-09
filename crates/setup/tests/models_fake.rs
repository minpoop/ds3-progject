//! `ds3-models` against synthetic installs of both games (no game file involved): the weapons' made-up templates and the
//! made-up weapon containers of the fake Dark Souls III. A trial run writes a report, pictures and candidates next to it and
//! nothing else; an install run puts the containers in the mod folder, lists them, and removes them again when a later run
//! cannot make them.
mod common;
use ashen_ds3data::bnd4::Bnd4;
use ashen_ds3data::dcx;
use ashen_ds3data::flver::Flver;
use ashen_ds3data::testing::install::{build, FakeDs3, FakeOptions};
use ashen_ds3data::tpf::Tpf;
use ashen_setup::ds3::{self, ModelsOpts, MODELS_MANIFEST_FILE, MODELS_REMOVE_REPORT_FILE, MODELS_REPORT_FILE};
use common::{add_made_up_weapons, fake_install, tree};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

struct Env {
    _t: tempfile::TempDir,
    ds3: FakeDs3,
    sm2: PathBuf,
    data: PathBuf,
}

impl Env {
    fn new(weapons: bool) -> Env {
        let t = tempfile::tempdir().unwrap();
        let ds3 = build(&t.path().join("Games").join("DARK SOULS III"), &FakeOptions::default());
        let sm2 = t.path().join("Games").join("Space Marine 2");
        fake_install(&sm2);
        if weapons {
            add_made_up_weapons(&sm2);
        }
        let data = t.path().join("kit");
        fs::create_dir_all(&data).unwrap();
        Env { _t: t, ds3, sm2, data }
    }

    fn out(&self) -> PathBuf {
        self.data.join("ds3-models")
    }

    fn mod_dir(&self) -> PathBuf {
        self.data.join("mod")
    }

    fn run(&self, install: bool) -> ds3::ModelsOutcome {
        ds3::models(&ModelsOpts { ds3: Some(self.ds3.root.clone()), sm2: Some(self.sm2.clone()), keys: None, data: self.data.clone(), out: self.out(), mod_dir: self.mod_dir(), install, remove: false })
    }

    fn report(&self) -> String {
        let text = fs::read_to_string(self.out().join(MODELS_REPORT_FILE)).unwrap();
        if std::env::var_os("ASHEN_SHOW_REPORT").is_some() {
            println!("{text}");
        }
        text
    }
}

/// The model and the textures inside a finished container file.
fn inside(file: &PathBuf) -> (Flver, Tpf) {
    let (inner, _) = dcx::decode(&fs::read(file).unwrap()).unwrap();
    let b = Bnd4::parse(&inner).unwrap();
    let get = |ext: &str| b.files.iter().find(|f| f.name.as_deref().is_some_and(|n| n.ends_with(ext))).unwrap().index;
    (Flver::parse(b.file_bytes(&inner, get(".flver")).unwrap()).unwrap(), Tpf::parse(b.file_bytes(&inner, get(".tpf")).unwrap()).unwrap())
}

#[test]
fn a_trial_run_makes_candidates_and_pictures_and_changes_nothing_else() {
    let env = Env::new(true);
    let (ds3_before, sm2_before) = (tree(&env.ds3.root), tree(&env.sm2));
    let outcome = env.run(false);
    assert!(outcome.ok(), "{:?}", outcome.failed);
    assert_eq!((tree(&env.ds3.root), tree(&env.sm2)), (ds3_before, sm2_before), "the games are untouched");
    assert!(!env.mod_dir().exists(), "a trial run puts nothing where the game loads it");
    // the right-hand and left-hand file of the sword, the pistol's right-hand file (it has no left-hand file in the fake)
    assert_eq!(outcome.ready, vec!["wp_a_0200.partsbnd.dcx".to_string(), "wp_a_0200_l.partsbnd.dcx".to_string(), "wp_a_1409.partsbnd.dcx".to_string()]);
    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    for name in &outcome.ready {
        let (model, tpf) = inside(&env.out().join("candidates").join(name));
        assert_eq!((model.meshes.len(), model.meshes[0].vertex_buffers[0].vertex_count, model.triangles(0).len()), (1, 4, 2), "{name}");
        // the colour map is the picture of the material (red, 8x8 scaled to the old size), the other map one colour
        assert_eq!(tpf.textures.len(), 2);
    }
    let report = env.report();
    for needle in [
        "TRIAL RUN",
        "== chainsword ==",
        "== bolt pistol ==",
        "Space Marine 2 template  tpl/wpn_chainsword_00.tpl: 4 vertices, 2 triangles",
        "picture pct/square_tex.pct.resource: OXT1(BC1) 8x8",
        "placement: the new shape's",
        "winding:",
        "the model written again is byte-identical to the game's file",
        "g_Diffuse: wp_a_9999_a gets the new picture",
        "check: packed as DCX (DCX_DFLT_10000_44_9) and unpacked again: identical",
        "READY: ",
        "/parts/wp_a_1409_l.partsbnd.dcx is not in any archive",
        "3 model file(s) made, 0 not made",
    ] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }
    assert!(!report.contains("PROBLEM") && !report.contains("NOT DONE") && !report.contains("BEGIN RSA"), "{report}");
    for label in ["wp_a_0200", "wp_a_0200_l", "wp_a_1409"] {
        let png = fs::read(env.out().join(format!("{label}-overlay.png"))).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}

#[test]
fn an_install_run_puts_the_containers_in_the_mod_folder_and_lists_them() {
    let env = Env::new(true);
    let outcome = env.run(true);
    assert!(outcome.ok() && outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_eq!(outcome.installed, 3);
    let parts = env.mod_dir().join("parts");
    let mut names: Vec<String> = fs::read_dir(&parts).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    names.sort();
    assert_eq!(names, vec!["wp_a_0200.partsbnd.dcx", "wp_a_0200_l.partsbnd.dcx", "wp_a_1409.partsbnd.dcx"]);
    // no temporary files, and a manifest that names each file with its hash
    assert!(fs::read_dir(&parts).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    let manifest: Value = serde_json::from_str(&fs::read_to_string(env.mod_dir().join(MODELS_MANIFEST_FILE)).unwrap()).unwrap();
    let files = manifest["files"].as_array().unwrap();
    assert_eq!(files.len(), 3);
    for f in files {
        let path = f["path"].as_str().unwrap();
        assert!(path.starts_with("parts/wp_a_"), "{path}");
        let bytes = fs::read(env.mod_dir().join(path)).unwrap();
        assert_eq!(f["bytes"].as_u64().unwrap(), bytes.len() as u64);
        assert_eq!(f["sha256"].as_str().unwrap(), ashen_ds3data::sha256_hex(&bytes));
    }
    assert_eq!(manifest["format"], 1);
    // the same run again gives the same files (the work is deterministic)
    let first: Vec<Vec<u8>> = names.iter().map(|n| fs::read(parts.join(n)).unwrap()).collect();
    let again = env.run(true);
    assert!(again.ok());
    assert_eq!(again.removed_stale, 3, "the earlier models are removed first");
    let second: Vec<Vec<u8>> = names.iter().map(|n| fs::read(parts.join(n)).unwrap()).collect();
    assert_eq!(first, second);
    // a file in the mod folder that the manifest does not list is left alone
    fs::write(env.mod_dir().join("parts").join("mine.txt"), b"mine").unwrap();
    fs::create_dir_all(env.mod_dir().join("msg")).unwrap();
    fs::write(env.mod_dir().join("msg").join("other.bin"), b"other").unwrap();
    assert!(env.run(true).ok());
    assert_eq!(fs::read(env.mod_dir().join("parts").join("mine.txt")).unwrap(), b"mine");
    assert_eq!(fs::read(env.mod_dir().join("msg").join("other.bin")).unwrap(), b"other");
}

#[test]
fn remove_takes_out_exactly_what_an_install_put_there_and_nothing_else() {
    let env = Env::new(true);
    assert!(env.run(true).ok());
    let parts = env.mod_dir().join("parts");
    fs::write(parts.join("mine.txt"), b"mine").unwrap();
    fs::write(env.mod_dir().join("ashenmarine-msg.json"), b"{}").unwrap();
    let (ds3_before, sm2_before) = (tree(&env.ds3.root), tree(&env.sm2));
    let outcome = ds3::models(&ModelsOpts { ds3: Some(env.ds3.root.clone()), sm2: Some(env.sm2.clone()), keys: None, data: env.data.clone(), out: env.out(), mod_dir: env.mod_dir(), install: false, remove: true });
    assert!(outcome.ok() && outcome.removal_only && outcome.removed_stale == 3 && outcome.ready.is_empty(), "{outcome:?}");
    // the three models and the manifest are gone; the player's own file and the other manifest are not
    let mut left: Vec<String> = fs::read_dir(&parts).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    left.sort();
    assert_eq!(left, vec!["mine.txt"]);
    assert!(!env.mod_dir().join(MODELS_MANIFEST_FILE).exists() && env.mod_dir().join("ashenmarine-msg.json").exists());
    assert_eq!((tree(&env.ds3.root), tree(&env.sm2)), (ds3_before, sm2_before), "the games are untouched");
    let report = fs::read_to_string(env.out().join(MODELS_REMOVE_REPORT_FILE)).unwrap();
    assert!(report.contains("take the new weapon models out again") && report.contains("Removed 3 file(s)"), "{report}");
    // nothing left to remove: said plainly, and still a success
    let again = ds3::models(&ModelsOpts { ds3: Some(env.ds3.root.clone()), sm2: Some(env.sm2.clone()), keys: None, data: env.data.clone(), out: env.out(), mod_dir: env.mod_dir(), install: false, remove: true });
    assert!(again.ok() && again.removed_stale == 0);
    assert!(fs::read_to_string(env.out().join(MODELS_REMOVE_REPORT_FILE)).unwrap().contains("There was nothing to remove"));
    // a manifest that points outside the mod folder deletes nothing outside it
    let outside = env.data.join("precious.txt");
    fs::write(&outside, b"keep").unwrap();
    fs::write(env.mod_dir().join(MODELS_MANIFEST_FILE), "{\"format\":1,\"files\":[{\"path\":\"../precious.txt\"}]}").unwrap();
    let hostile = ds3::models(&ModelsOpts { ds3: Some(env.ds3.root.clone()), sm2: Some(env.sm2.clone()), keys: None, data: env.data.clone(), out: env.out(), mod_dir: env.mod_dir(), install: false, remove: true });
    assert!(hostile.ok() && hostile.removed_stale == 0 && outside.exists());
}

#[test]
fn when_a_weapon_cannot_be_made_nothing_stale_stays_and_the_reason_is_in_the_report() {
    let env = Env::new(true);
    assert!(env.run(true).ok());
    assert!(env.mod_dir().join("parts").join("wp_a_1409.partsbnd.dcx").exists());
    // Space Marine 2 without the pistol: its model is not made (and the earlier one is gone), the sword is
    let sm2_bare = env.sm2.parent().unwrap().join("Space Marine 2 without pistol");
    fake_install(&sm2_bare);
    let (tpl, data) = ashen_sm2::testing::made_up_weapon();
    common::make_zip(
        &sm2_bare.join("client_pc/root/paks/client/default/default_tpl_2.pak"),
        &[("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl", tpl), ("tpl/wpn_chainsword_00.tpl/wpn_chainsword_00.tpl_data", data)],
        true,
    );
    let outcome = ds3::models(&ModelsOpts { ds3: Some(env.ds3.root.clone()), sm2: Some(sm2_bare), keys: None, data: env.data.clone(), out: env.out(), mod_dir: env.mod_dir(), install: true, remove: false });
    assert!(outcome.ok());
    assert_eq!(outcome.failed.len(), 1, "{:?}", outcome.failed);
    assert!(!env.mod_dir().join("parts").join("wp_a_1409.partsbnd.dcx").exists(), "the pistol's model of the earlier run is gone");
    assert!(env.mod_dir().join("parts").join("wp_a_0200.partsbnd.dcx").exists());
    let report = env.report();
    assert!(report.contains("NOT DONE: the template folder has no .tpl_data") || report.contains("NOT DONE: no template folder with \"bolt_pistol\""), "{report}");
    // no usable full-detail model at all: nothing is made, and the exit says so
    let nothing = ds3::models(&ModelsOpts { ds3: Some(env.ds3.root.clone()), sm2: Some(env.sm2.parent().unwrap().join("nowhere")), keys: None, data: env.data.clone(), out: env.out(), mod_dir: env.mod_dir(), install: true, remove: false });
    assert!(!nothing.ok());
    assert!(!env.mod_dir().join("parts").join("wp_a_0200.partsbnd.dcx").exists(), "a run that cannot make anything leaves no stale model");
}

#[test]
fn damaged_game_files_are_reported_and_a_folder_inside_a_game_is_refused() {
    // models that are not models: the swap refuses, with the reason
    let t = tempfile::tempdir().unwrap();
    let ds3 = build(&t.path().join("DS3").join("DARK SOULS III"), &FakeOptions { junk_models: true, ..FakeOptions::default() });
    let sm2 = t.path().join("Space Marine 2");
    fake_install(&sm2);
    add_made_up_weapons(&sm2);
    let data = t.path().join("kit");
    fs::create_dir_all(&data).unwrap();
    let outcome = ds3::models(&ModelsOpts { ds3: Some(ds3.root.clone()), sm2: Some(sm2.clone()), keys: None, data: data.clone(), out: data.join("ds3-models"), mod_dir: data.join("mod"), install: true, remove: false });
    assert!(!outcome.ok());
    assert!(outcome.failed.iter().all(|(_, why)| why.contains("cannot be read")), "{:?}", outcome.failed);
    assert!(!data.join("mod").join("parts").exists() || fs::read_dir(data.join("mod").join("parts")).unwrap().count() == 0);
    // a folder inside a game's folder
    let before = (tree(&ds3.root), tree(&sm2));
    for inside in [ds3.game.join("copies"), sm2.join("client_pc").join("copies")] {
        let refused = ds3::models(&ModelsOpts { ds3: Some(ds3.root.clone()), sm2: Some(sm2.clone()), keys: None, data: data.clone(), out: inside.clone(), mod_dir: data.join("mod2"), install: false, remove: false });
        assert!(!refused.ok() && !inside.exists());
        let refused = ds3::models(&ModelsOpts { ds3: Some(ds3.root.clone()), sm2: Some(sm2.clone()), keys: None, data: data.clone(), out: data.join("o"), mod_dir: inside.clone(), install: true, remove: false });
        assert!(!refused.ok() && !inside.exists());
    }
    assert_eq!((tree(&ds3.root), tree(&sm2)), before);
}
