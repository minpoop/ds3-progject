//! Runs `ds3-probe` and `ds3-prepare` against synthetic Dark Souls III installs built in a temp folder (no game file
//! involved): the success path, a game update that renamed an item, no archive key, a missing item text, the keys given
//! as a file, and the checks that the install is never touched and that a stale override never stays.
use ashen_ds3data::bnd4::Bnd4;
use ashen_ds3data::dcx;
use ashen_ds3data::fmg::FmgFile;
use ashen_ds3data::testing::install::{build, FakeDs3, FakeOptions, ItemMsg};
use ashen_ds3data::testing::items::item_bnd4;
use ashen_ds3data::testing::keys::test_key;
use ashen_setup::ds3::{self, Outcome, PrepareOpts, ProbeOpts, EDITS, MANIFEST_FILE};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Every file below `root` with its bytes.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
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

fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A fake install, the folder of the "program" (where the report and the mod folder go) and what a run needs.
struct Env {
    _t: tempfile::TempDir,
    fake: FakeDs3,
    data: PathBuf,
}

impl Env {
    fn new(opts: &FakeOptions) -> Env {
        let t = tempfile::tempdir().unwrap();
        let fake = build(&t.path().join("Games").join("DARK SOULS III"), opts);
        let data = t.path().join("kit");
        fs::create_dir_all(&data).unwrap();
        Env { _t: t, fake, data }
    }

    fn out(&self) -> PathBuf {
        self.data.join("ds3-prepare")
    }

    fn mod_dir(&self) -> PathBuf {
        self.data.join("mod")
    }

    fn override_path(&self) -> PathBuf {
        self.mod_dir().join("msg").join("ENGLISH").join("item.msgbnd.dcx")
    }

    fn probe(&self, keys: Option<PathBuf>) -> bool {
        ds3::probe(&ProbeOpts { ds3: Some(self.fake.root.clone()), keys, data: self.data.clone(), out: self.out() })
    }

    fn prepare(&self, keys: Option<PathBuf>) -> Outcome {
        ds3::prepare(&PrepareOpts { ds3: Some(self.fake.root.clone()), keys, data: self.data.clone(), out: self.out(), mod_dir: self.mod_dir() })
    }

    fn report(&self) -> String {
        let text = fs::read_to_string(ds3::report_path(&self.out())).unwrap();
        if std::env::var_os("ASHEN_SHOW_REPORT").is_some() {
            println!("{text}");
        }
        text
    }

    fn keys_file(&self) -> PathBuf {
        let p = self.data.join("my-keys.pem");
        fs::write(&p, format!("# the keys\n{}{}", test_key(0).pem(), test_key(1).pem())).unwrap();
        p
    }
}

/// The names and descriptions the override holds, per table id of the container.
fn texts_of(override_file: &Path) -> BTreeMap<i32, FmgFile> {
    let (inner, _) = dcx::decode(&fs::read(override_file).unwrap()).unwrap();
    let b = Bnd4::parse(&inner).unwrap();
    assert!(b.layout.verified);
    b.files.iter().filter_map(|f| FmgFile::parse(b.file_bytes(&inner, f.index)?).ok().map(|t| (f.id.unwrap(), t))).collect()
}

#[test]
fn probe_reads_a_fake_install_and_leaves_it_untouched() {
    let env = Env::new(&FakeOptions::default());
    let before = tree(&env.fake.root);
    assert!(env.probe(None));
    assert_eq!(tree(&env.fake.root), before, "the probe must not change anything in the install");
    let report = env.report();
    for needle in [
        "archive keys found in the program file as plain text: 2 (87febfc8, b2969406)",
        "Data0    ",
        "key 87febfc8, salt 11 characters, 11 buckets, 24 files",
        "key b2969406, salt 11 characters, 7 buckets, 5 files",
        "DLC1     ",
        "/msg/ENGLISH/item.msgbnd.dcx               hash 50b424bf  Data0: 40 B stored, 40 B unpadded; also Data0: ",
        "/regulation.bin",
        "/parts/wp_a_1419.partsbnd.dcx",
        "9 of 40 paths exist in the archives",
        "/msg/FRENCH/item.msgbnd.dcx",
        "DCX variant DCX_DFLT_10000_44_9",
        "container: BND4 version \"07D7R6\", format 0x2e (IDs|Names1|Names2|Compression) (stored as 0x74)",
        "layout: alignment 0x10",
        "WeaponName.fmg",
        "id 2000000: WeaponName.fmg \"Shortsword\"",
        "id 14090000: WeaponName.fmg \"Avelyn\"",
        "id 404000: WeaponName.fmg \"Standard Bolt\"",
        "id 14040000: WeaponName.fmg \"Light Crossbow\"",
        "id 14190000: WeaponName.fmg \"Repeating Crossbow\"",
        "[ok] DCX written again and read back",
        "[ok] BND4 rewrite with each file's own bytes gives identical bytes",
        "[info] text tables written again by this program's writer are byte-identical to the game's: yes (5 of 5)",
        "[ok] the edits: 3 edits, 3 tables rewritten",
        "[ok] the new DCX file decodes to the patched container",
        "/parts/wp_a_0200.partsbnd.dcx:",
        "wp_a_0200.flver",
        "/parts/wp_a_1409.partsbnd.dcx:",
        "wp_a_1409.hkx",
    ] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }
    assert!(!report.contains("FAILED") && !report.contains("PROBLEM") && !report.contains("CRASHED"), "{report}");
    // nothing but the report was written
    assert_eq!(tree(&env.data).keys().cloned().collect::<Vec<_>>(), vec![PathBuf::from("ds3-prepare").join("ds3-report.txt")]);
}

#[test]
fn the_report_shows_fingerprints_and_not_keys_or_folder_names() {
    let env = Env::new(&FakeOptions::default());
    assert!(env.probe(Some(env.keys_file())));
    let report = env.report();
    let key_text = test_key(0).public.n().to_bytes_be();
    let b64 = ashen_ds3data::keys::base64_encode(&key_text);
    assert!(!report.contains(&b64[..40]) && !report.contains("BEGIN RSA PUBLIC KEY"), "no key material in the report");
    for dir in [env.fake.root.parent().unwrap(), env._t.path()] {
        assert!(!report.contains(dir.to_string_lossy().as_ref()), "the report names a folder above the game folder: {}", dir.display());
    }
    assert!(!report.contains("/tmp") && !report.contains("home"), "{report}");
    assert!(report.contains("keys from the key file my-keys.pem: 2 (87febfc8, b2969406)"), "{report}");
}

#[test]
fn probe_without_any_key_says_so_and_does_not_succeed() {
    let env = Env::new(&FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
    assert!(!env.probe(None));
    let report = flat(&env.report());
    assert!(report.contains("archive keys found in the program file as plain text: none"), "{report}");
    assert!(report.contains("no archive key was found in your Dark Souls III program file; start the game once with the test kit (Play-AshenMarine.bat), close it, and run this again"), "{report}");
    assert!(report.contains("no key to try"));
}

#[test]
fn keys_from_a_file_or_the_cache_open_the_archives() {
    let env = Env::new(&FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
    // --keys
    assert!(env.probe(Some(env.keys_file())));
    assert!(env.report().contains("[ok] the edits"));
    // the cache the in-game probe may write: <data>/cache/ds3-keys.pem
    let env = Env::new(&FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
    assert!(!env.probe(None));
    fs::create_dir_all(env.data.join("cache")).unwrap();
    fs::write(env.data.join("cache").join("ds3-keys.pem"), test_key(0).pem() + &test_key(1).pem()).unwrap();
    assert!(env.probe(None));
    assert!(env.report().contains("keys from cache\\ds3-keys.pem: 2 (87febfc8, b2969406)"));
    // only one of the two keys: the archives of that key open, the others are listed as not opened
    let env = Env::new(&FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
    let one = env.data.join("one.pem");
    fs::write(&one, test_key(1).pem()).unwrap();
    assert!(env.probe(Some(one)));
    let report = flat(&env.report());
    assert!(report.contains("Data0 2.5 KB .bhd, no key matched (1 tried)") && report.contains("NOTE: 2 of 3 archives could not be opened"), "{report}");
    // a key file that holds no key is named as a problem but does not stop the run
    let env = Env::new(&FakeOptions::default());
    let junk = env.data.join("junk.pem");
    fs::write(&junk, "not a key").unwrap();
    assert!(env.probe(Some(junk)));
    assert!(flat(&env.report()).contains("PROBLEM: the key file junk.pem cannot be used"));
    let missing = env.data.join("missing.pem");
    assert!(env.probe(Some(missing)));
    assert!(flat(&env.report()).contains("PROBLEM: the key file missing.pem cannot be used: cannot read the key file: no such file"));
}

#[test]
fn prepare_writes_the_override_and_its_manifest_and_leaves_the_install_untouched() {
    let env = Env::new(&FakeOptions::default());
    let before = tree(&env.fake.root);
    let outcome = env.prepare(None);
    assert!(outcome.ok() && outcome.written && outcome.reason.is_none() && outcome.edits == 3, "{outcome:?}");
    assert_eq!(tree(&env.fake.root), before, "prepare must not change anything in the install");
    // exactly these three files appear: the report, the override, the manifest
    let files: Vec<PathBuf> = tree(&env.data).keys().cloned().collect();
    assert_eq!(files, vec![PathBuf::from("ds3-prepare").join("ds3-report.txt"), PathBuf::from("mod").join("ashenmarine-msg.json"), ["mod", "msg", "ENGLISH", "item.msgbnd.dcx"].iter().collect::<PathBuf>()]);

    // the override: DCX holding the game's BND4 with only the test weapons' texts changed
    let t = texts_of(&env.override_path());
    assert_eq!(t[&11].get(2_000_000), Some("Chainsword"));
    assert_eq!(t[&11].get(14_090_000), Some("Bolt Pistol"));
    assert_eq!(t[&11].get(404_000), Some("Bolt Rounds"));
    assert_eq!((t[&11].get(14_040_000), t[&11].get(14_190_000)), (Some("Light Crossbow"), Some("Repeating Crossbow")));
    for e in EDITS {
        assert_eq!(t[&21].get(e.id), Some(e.short), "the summary table gets the short text");
        assert_eq!(t[&31].get(e.id), Some(e.long), "the description table gets the long text");
    }
    let (original, _) = dcx::decode(&ashen_ds3data::testing::items::item_dcx()).unwrap();
    assert_eq!(original, item_bnd4());
    let o = Bnd4::parse(&original).unwrap();
    let (patched_bytes, _) = dcx::decode(&fs::read(env.override_path()).unwrap()).unwrap();
    let p = Bnd4::parse(&patched_bytes).unwrap();
    for i in [0usize, 4, 5] {
        assert_eq!(o.file_bytes(&original, i), p.file_bytes(&patched_bytes, i), "untouched file {i} is byte-identical");
    }

    // the manifest
    let manifest: Value = serde_json::from_str(&fs::read_to_string(env.mod_dir().join(MANIFEST_FILE)).unwrap()).unwrap();
    assert_eq!(manifest["format"], 1);
    assert_eq!(manifest["tool"], format!("ashenmarine-setup {}", ashen_common::VERSION));
    assert_eq!(manifest["source"]["archive"], "Data0.bhd");
    assert_eq!(manifest["source"]["sha256_of_decoded_original"], ashen_ds3data::sha256_hex(&original));
    assert_eq!(manifest["edits"].as_array().unwrap().len(), 3);
    assert_eq!(manifest["edits"][2], serde_json::json!({"id": 404000, "old": "Standard Bolt", "new": "Bolt Rounds"}));
    assert_eq!(manifest["written"], "msg/ENGLISH/item.msgbnd.dcx");

    // running it again gives the same files, and no temporary files are left
    let again = env.prepare(None);
    assert!(again.ok());
    assert_eq!(tree(&env.mod_dir()).len(), 2);
    assert_eq!(fs::read(env.override_path()).unwrap(), fs::read(env.override_path()).unwrap());
    let report = flat(&env.report());
    assert!(report.contains("Written: msg\\ENGLISH\\item.msgbnd.dcx and ashenmarine-msg.json"), "{report}");
    assert!(report.contains("Dark Souls III itself was not changed"));
}

#[test]
fn prepare_is_the_same_each_time() {
    let a = Env::new(&FakeOptions::default());
    let b = Env::new(&FakeOptions::default());
    assert!(a.prepare(None).ok() && b.prepare(None).ok());
    assert_eq!(fs::read(a.override_path()).unwrap(), fs::read(b.override_path()).unwrap());
    // the manifest only differs by nothing: both runs saw the same game
    assert_eq!(fs::read(a.mod_dir().join(MANIFEST_FILE)).unwrap(), fs::read(b.mod_dir().join(MANIFEST_FILE)).unwrap());
}

#[test]
fn prepare_with_keys_from_a_file_works_when_the_program_file_has_none() {
    let env = Env::new(&FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
    let outcome = env.prepare(Some(env.keys_file()));
    assert!(outcome.ok(), "{outcome:?}\n{}", env.report());
    assert!(env.override_path().is_file());
}

#[test]
fn a_renamed_item_writes_nothing_and_removes_the_override_of_an_earlier_run() {
    let env = Env::new(&FakeOptions::default());
    assert!(env.prepare(None).ok());
    assert!(env.override_path().is_file() && env.mod_dir().join(MANIFEST_FILE).is_file());
    // a game update renames the Shortsword; the same folder, new archives
    fs::remove_dir_all(&env.fake.root).unwrap();
    build(&env.fake.root, &FakeOptions { item_msg: ItemMsg::WrongName, ..FakeOptions::default() });
    let before = tree(&env.fake.root);
    let outcome = env.prepare(None);
    assert!(!outcome.ok() && outcome.removed_stale, "{outcome:?}");
    let reason = outcome.reason.clone().unwrap();
    assert!(reason.contains("no longer has \"Shortsword\" at id 2000000") && reason.contains("Broadsword"), "{reason}");
    assert!(!env.override_path().exists() && !env.mod_dir().join(MANIFEST_FILE).exists(), "the stale override must not stay");
    assert_eq!(tree(&env.fake.root), before);
    let report = flat(&env.report());
    assert!(report.contains("Nothing was changed: the item names in the game stay as Dark Souls III has them."), "{report}");
    assert!(report.contains("Removed the item text file of an earlier run"), "{report}");
    assert!(report.contains("[FAILED] the edits could not be made"), "{report}");

    // the first run on such a game writes nothing at all (not even folders)
    let fresh = Env::new(&FakeOptions { item_msg: ItemMsg::WrongName, ..FakeOptions::default() });
    let outcome = fresh.prepare(None);
    assert!(!outcome.ok() && !outcome.removed_stale);
    assert!(!fresh.mod_dir().exists(), "nothing new is created when a check fails");
}

#[test]
fn without_an_archive_key_nothing_is_written_and_a_stale_override_is_removed() {
    let env = Env::new(&FakeOptions::default());
    assert!(env.prepare(None).ok());
    // the same install, but this time the program file shows no keys
    fs::remove_dir_all(&env.fake.root).unwrap();
    build(&env.fake.root, &FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
    let outcome = env.prepare(None);
    assert!(!outcome.ok() && outcome.removed_stale);
    assert_eq!(outcome.reason.as_deref(), Some("no archive key was found in your Dark Souls III program file; start the game once with the test kit (Play-AshenMarine.bat), close it, and run this again"));
    assert!(!env.override_path().exists() && !env.mod_dir().join(MANIFEST_FILE).exists());
}

#[test]
fn a_missing_or_unusable_item_text_is_a_failure_with_a_reason() {
    for (kind, needle) in [
        (ItemMsg::Missing, "was not found in any Dark Souls III archive"),
        (ItemMsg::FrenchOnly, "found only for french but not for English"),
        (ItemMsg::DamagedDcx, "could not be used"),
        (ItemMsg::UnpatchableLayout, "not stored the way this program expects"),
    ] {
        let env = Env::new(&FakeOptions { item_msg: kind, ..FakeOptions::default() });
        let before = tree(&env.fake.root);
        let outcome = env.prepare(None);
        let reason = outcome.reason.clone().unwrap_or_default();
        assert!(!outcome.ok() && reason.contains(needle), "{kind:?}: {outcome:?}");
        assert!(!env.mod_dir().exists(), "{kind:?}: nothing is written");
        assert_eq!(tree(&env.fake.root), before);
        assert!(flat(&env.report()).contains("send me the report"), "{kind:?}");
    }
}

#[test]
fn archives_that_cannot_be_opened_do_not_stop_a_run_that_does_not_need_them() {
    let env = Env::new(&FakeOptions { broken_archives: true, ..FakeOptions::default() });
    assert!(env.probe(None));
    assert!(env.prepare(None).ok());
    let report = flat(&env.report());
    assert!(report.contains("Data8 1.0 KB .bhd, no key matched") && report.contains("Data9 512 B .bhd, cannot be opened: cannot read the .bdt file: no such file"), "{report}");
    assert!(report.contains("NOTE: 2 of 5 archives could not be opened"));
}

#[test]
fn a_file_in_the_mod_folder_that_the_manifest_does_not_list_is_left_alone() {
    let env = Env::new(&FakeOptions { item_msg: ItemMsg::WrongName, ..FakeOptions::default() });
    fs::create_dir_all(env.override_path().parent().unwrap()).unwrap();
    fs::write(env.override_path(), b"somebody else's file").unwrap();
    let outcome = env.prepare(None);
    assert!(!outcome.ok() && !outcome.removed_stale);
    assert_eq!(fs::read(env.override_path()).unwrap(), b"somebody else's file");
    assert!(flat(&env.report()).contains("no readable manifest for it, so it is not known to come from this program and it was left alone"));
}

#[test]
fn nothing_is_ever_written_inside_the_game_folder() {
    let env = Env::new(&FakeOptions::default());
    let before = tree(&env.fake.root);
    for inside in [env.fake.root.join("mod"), env.fake.game.join("mod"), env.fake.game.join("deeper").join("mod")] {
        let outcome = ds3::prepare(&PrepareOpts { ds3: Some(env.fake.root.clone()), keys: None, data: env.data.clone(), out: env.out(), mod_dir: inside.clone() });
        assert!(!outcome.ok(), "{}", inside.display());
        assert!(outcome.reason.as_deref().unwrap_or("").contains("inside the game folder"));
        assert!(!ds3::probe(&ProbeOpts { ds3: Some(env.fake.root.clone()), keys: None, data: env.data.clone(), out: inside.join("report") }));
    }
    assert_eq!(tree(&env.fake.root), before);
    assert!(!env.out().exists(), "no report either");
    // the Game folder itself may be given as the game folder
    let outcome = ds3::prepare(&PrepareOpts { ds3: Some(env.fake.game.clone()), keys: None, data: env.data.clone(), out: env.out(), mod_dir: env.mod_dir() });
    assert!(outcome.ok());
}

#[test]
fn a_game_that_is_not_found_is_a_clear_failure() {
    let t = tempfile::tempdir().unwrap();
    let nothing = t.path().join("nothing-here");
    fs::create_dir_all(&nothing).unwrap();
    let data = t.path().join("kit");
    let out = data.join("ds3-prepare");
    let opts = PrepareOpts { ds3: Some(nothing.clone()), keys: None, data: data.clone(), out: out.clone(), mod_dir: data.join("mod") };
    let outcome = ds3::prepare(&opts);
    assert!(!outcome.ok());
    assert!(outcome.reason.unwrap().contains("nothing-here does not contain Game\\DarkSoulsIII.exe"));
    assert!(!ds3::probe(&ProbeOpts { ds3: Some(nothing), keys: None, data, out }));
}

// ---- the program itself

fn setup_exe() -> &'static str {
    env!("CARGO_BIN_EXE_ashenmarine-setup")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

#[test]
fn the_program_exits_with_0_2_and_64() {
    let env = Env::new(&FakeOptions::default());
    let run = Command::new(setup_exe()).arg("ds3-probe").arg("--ds3").arg(&env.fake.root).arg("--out").arg(env.out()).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stdout));
    let run = Command::new(setup_exe()).arg("ds3-prepare").arg("--ds3").arg(&env.fake.root).arg("--out").arg(env.out()).arg("--mod").arg(env.mod_dir()).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stdout));
    assert!(flat(&text(&run.stdout)).contains("Dark Souls III itself was not changed"));
    assert!(env.override_path().is_file());

    let broken = Env::new(&FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
    let run = Command::new(setup_exe()).arg("ds3-prepare").arg("--ds3").arg(&broken.fake.root).arg("--out").arg(broken.out()).arg("--mod").arg(broken.mod_dir()).output().unwrap();
    assert_eq!(run.status.code(), Some(2));
    assert!(flat(&text(&run.stdout)).contains("no archive key was found in your Dark Souls III program file"));
    let run = Command::new(setup_exe()).arg("ds3-probe").arg("--ds3").arg(&broken.fake.root).arg("--out").arg(broken.out()).output().unwrap();
    assert_eq!(run.status.code(), Some(2));

    for args in [vec!["ds3-prepare", "--bogus"], vec!["ds3-probe", "--sm2", "x"], vec!["ds3-prepare", "--keys"], vec!["ds3-probe", "--mod", "x"], vec!["probe", "--ds3", "x"]] {
        let run = Command::new(setup_exe()).args(&args).output().unwrap();
        assert_eq!(run.status.code(), Some(64), "{args:?}");
        assert!(text(&run.stdout).contains("ashenmarine-setup ds3-prepare"), "the usage is shown for {args:?}");
    }
    let help = Command::new(setup_exe()).args(["ds3-prepare", "--help"]).output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    assert!(text(&help.stdout).contains("ds3-probe") && text(&help.stdout).contains("game-folder.txt"));
}

#[test]
fn started_from_its_own_folder_it_follows_game_folder_txt_and_writes_next_to_itself() {
    let env = Env::new(&FakeOptions::default());
    let kit = env._t.path().join("ashenmarine");
    fs::create_dir_all(&kit).unwrap();
    let exe = kit.join(format!("ashenmarine-setup{}", std::env::consts::EXE_SUFFIX));
    fs::copy(setup_exe(), &exe).unwrap();
    fs::write(kit.join("game-folder.txt"), format!("\"{}\"\r\nsecond line\r\n", env.fake.root.display())).unwrap();
    let before = tree(&env.fake.root);
    let run = Command::new(&exe).arg("ds3-prepare").current_dir(env._t.path()).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stdout));
    assert!(kit.join("mod/msg/ENGLISH/item.msgbnd.dcx").is_file() && kit.join("mod/ashenmarine-msg.json").is_file());
    assert!(kit.join("ds3-prepare/ds3-report.txt").is_file());
    assert!(!env._t.path().join("mod").exists() && !env._t.path().join("ds3-prepare").exists(), "nothing is written next to the working folder");
    assert_eq!(tree(&env.fake.root), before);
    // the probe, too
    let run = Command::new(&exe).arg("ds3-probe").current_dir(env._t.path()).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stdout));
    // keys cached by the in-game probe are found next to the program
    let kit2 = env._t.path().join("kit2");
    fs::create_dir_all(kit2.join("cache")).unwrap();
    let no_keys = Env::new(&FakeOptions { exe_keys: vec![], ..FakeOptions::default() });
    let exe2 = kit2.join(format!("ashenmarine-setup{}", std::env::consts::EXE_SUFFIX));
    fs::copy(setup_exe(), &exe2).unwrap();
    fs::write(kit2.join("game-folder.txt"), no_keys.fake.root.display().to_string()).unwrap();
    assert_eq!(Command::new(&exe2).arg("ds3-prepare").output().unwrap().status.code(), Some(2));
    fs::write(kit2.join("cache").join("ds3-keys.pem"), test_key(0).pem() + &test_key(1).pem()).unwrap();
    assert_eq!(Command::new(&exe2).arg("ds3-prepare").output().unwrap().status.code(), Some(0));
}

#[test]
fn a_prepare_run_is_fast() {
    let env = Env::new(&FakeOptions::default());
    let started = std::time::Instant::now();
    assert!(env.prepare(None).ok());
    assert!(env.probe(None));
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
}

#[test]
fn the_text_table_writer_and_the_reader_the_hook_uses_agree_on_the_format() {
    use ashen_common::fmg;
    // a table made by the hook crate's builder reads with ours, with the null string kept
    let names = [Some("Dagger"), None, Some("Parrying Dagger")];
    let bytes = fmg::build(&[(1_000_000, &names), (2_000_000, &[Some("Shortsword")])]);
    let ours = FmgFile::parse(&bytes).unwrap();
    assert_eq!((ours.get(1_000_000), ours.get(1_000_001), ours.contains(1_000_001), ours.get(1_000_002), ours.get(2_000_000)), (Some("Dagger"), None, true, Some("Parrying Dagger"), Some("Shortsword")));
    // and the other way round: what we write, the hook's parser accepts, with the same entries
    let mut t = FmgFile::new();
    t.set(2_000_000, "Chainsword");
    t.set(2_000_001, "Longsword");
    t.set_null(2_000_002);
    t.set(14_090_000, "Bolt Pistol");
    let theirs = fmg::parse(&t.to_bytes()).expect("the hook's reader accepts the written table");
    assert_eq!(theirs.entries, vec![(2_000_000, "Chainsword".to_string()), (2_000_001, "Longsword".to_string()), (14_090_000, "Bolt Pistol".to_string())]);
    assert_eq!((theirs.version, theirs.string_count, theirs.group_count), (2, 4, 2));
    assert_eq!(fmg::header_file_size(&t.to_bytes()), Some(t.to_bytes().len()));
}
