//! Runs `prepare` against a synthetic Space Marine 2 install built in a temp folder (no game file involved) and checks
//! the sound files, the two JSON files, the report - and that the install is byte-for-byte untouched.
//! What the fake install holds is described at `common::weapon_sounds`.
mod common;
use ashen_setup::prepare::{self, Opts, Outcome};
use ashen_sm2::bnk::{fnv1_lower, testbank::Builder};
use ashen_sm2::wem::{self, Pcm};
use common::{fake_install, pcm_wem, sound_install, tree};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The slots the fake install can make, with how many different versions of each it holds. The sheet has more rows; the
/// fake install lacks their events.
const MADE: [(&str, usize); 4] = [("chainsword_swing_1", 3), ("chainsword_swing_2", 1), ("chainsword_idle", 1), ("boltpistol_fire", 1)];

/// How many files `prepare` must make for a slot of `MADE`: as many takes as the sheet asks for, but no more than the
/// install has versions.
fn files_for(slot: &str) -> usize {
    takes_of(slot).min(MADE.iter().find(|m| m.0 == slot).unwrap().1)
}

/// How many takes the sheet asks for.
fn takes_of(slot: &str) -> usize {
    prepare::slots().unwrap().into_iter().find(|s| s.id == slot).unwrap_or_else(|| panic!("design/sheets/sounds.json has no row {slot}")).takes
}

fn total_files() -> usize {
    MADE.iter().map(|m| files_for(m.0)).sum()
}

/// A fake install and an output folder in a temp folder, with `prepare` run on them.
struct Run {
    _t: tempfile::TempDir,
    out: PathBuf,
    outcome: Outcome,
}

impl Run {
    fn new() -> Run {
        let t = tempfile::tempdir().unwrap();
        let sm2 = t.path().join("Space Marine 2");
        let out = t.path().join("ashenmarine").join("assets");
        fake_install(&sm2);
        let outcome = prepare::run(&Opts { sm2: Some(sm2), out: out.clone(), normalize: false, ab: false });
        let run = Run { _t: t, out, outcome };
        if std::env::var_os("ASHEN_SHOW_REPORT").is_some() {
            println!("{}", run.report());
        }
        run
    }

    fn sounds(&self) -> PathBuf {
        self.out.join("sounds")
    }

    /// The report with every run of white space made one space, so a phrase can be looked for however the lines were broken.
    fn report(&self) -> String {
        flat(&fs::read_to_string(prepare::report_path(&self.out)).unwrap())
    }

    fn index(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.sounds().join("index.json")).unwrap()).unwrap()
    }

    fn wav(&self, name: &str) -> Pcm {
        wem::decode(&fs::read(self.sounds().join(name)).unwrap()).unwrap()
    }
}

fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every file below `dir`.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(files_under(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn prepare_makes_the_sounds_and_leaves_the_install_untouched() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    fake_install(&sm2);
    let before = tree(&sm2);
    let outcome = prepare::run(&Opts { sm2: Some(sm2.clone()), out: t.path().join("assets"), normalize: false, ab: false });
    assert_eq!(tree(&sm2), before, "prepare must not change anything in the install");
    assert!(outcome.ok() && outcome.ready);
    assert_eq!((outcome.slots, outcome.prepared, outcome.files), (prepare::slots().unwrap().len(), 4, total_files()));
    assert_eq!(outcome.failed.len(), outcome.slots - 4);
}

#[test]
fn index_json_follows_the_contract() {
    let run = Run::new();
    let raw = fs::read_to_string(run.sounds().join("index.json")).unwrap();
    let swing_files: Vec<String> = (1..=files_for("chainsword_swing_1")).map(|n| format!("\"chainsword_swing_1_{n}.wav\"")).collect();
    let start = format!("{{\"format\":1,\"source\":\"Space Marine 2 unknown\",\"sounds\":[{{\"slot\":\"chainsword_swing_1\",\"event\":\"wpn_melee_chswd_light_1hit\",\"loop\":false,\"files\":[{}],\"seconds\":[", swing_files.join(","));
    assert!(raw.starts_with(&start), "the documented key order and shape:\n{raw}");
    let index = run.index();
    assert_eq!(index["format"], 1);
    assert_eq!(index["source"], "Space Marine 2 unknown");
    assert_eq!(index.as_object().unwrap().len(), 3);

    // the slots that were made, in the order of the sheet
    let table = prepare::slots().unwrap();
    let wanted: Vec<&str> = table.iter().map(|s| s.id.as_str()).filter(|id| MADE.iter().any(|m| m.0 == *id)).collect();
    let sounds = index["sounds"].as_array().unwrap();
    assert_eq!(sounds.iter().map(|s| s["slot"].as_str().unwrap()).collect::<Vec<_>>(), wanted);
    for entry in sounds {
        let slot = entry["slot"].as_str().unwrap();
        let row = table.iter().find(|s| s.id == slot).unwrap();
        let mut keys: Vec<&str> = entry.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["event", "files", "loop", "seconds", "slot"], "{slot}");
        assert_eq!(entry["event"], row.event.as_str());
        assert_eq!(entry["loop"], row.looped, "{slot}: the loop flag comes from the sheet");
        let files = entry["files"].as_array().unwrap();
        let seconds = entry["seconds"].as_array().unwrap();
        assert_eq!(files.len(), files_for(slot), "{slot}");
        assert_eq!(files.len(), seconds.len());
        for (i, (f, s)) in files.iter().zip(seconds).enumerate() {
            let name = f.as_str().unwrap();
            assert_eq!(name, format!("{slot}_{}.wav", i + 1), "names are relative to the sounds folder and number up from 1");
            let bytes = fs::read(run.sounds().join(name)).unwrap();
            assert!(&bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE", "{name} is a wav file");
            let real = wem::decode(&bytes).unwrap().seconds();
            assert!((s.as_f64().unwrap() - real).abs() <= 0.005 + 1e-9, "{name}: index says {s}, the file is {real} s long");
        }
    }
    // the loop flag really differs between slots, so the checks above mean something
    let flags: Vec<bool> = sounds.iter().map(|s| s["loop"].as_bool().unwrap()).collect();
    assert!(flags.contains(&true) && flags.contains(&false));
    // no other wav files than the ones listed
    let mut on_disk: Vec<String> = fs::read_dir(run.sounds()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".wav")).collect();
    let mut listed: Vec<String> = sounds.iter().flat_map(|s| s["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string())).collect();
    on_disk.sort();
    listed.sort();
    assert_eq!(on_disk, listed);
}

#[test]
fn ready_json_is_exact_and_is_written_last() {
    let run = Run::new();
    let raw = fs::read_to_string(run.out.join("ready.json")).unwrap();
    assert_eq!(raw, format!("{{\"format\":1,\"tool\":\"ashenmarine-setup {}\",\"slots\":4,\"files\":{}}}", ashen_common::VERSION, total_files()));
    let ready_time = fs::metadata(run.out.join("ready.json")).unwrap().modified().unwrap();
    let mut others = 0;
    for f in files_under(&run.out).into_iter().filter(|f| f.file_name().unwrap() != "ready.json") {
        others += 1;
        assert!(fs::metadata(&f).unwrap().modified().unwrap() <= ready_time, "{} is newer than ready.json", f.display());
    }
    assert_eq!(others, total_files() + 1, "the wav files and the index");
    assert!(!run.out.join("ready.json.tmp").exists() && !run.sounds().join("index.json.tmp").exists());
}

#[test]
fn takes_differ_and_layers_are_added() {
    let run = Run::new();
    // chainsword_swing_1: a random pick of three sounds (1000, 2000, 3000), layered with a fourth (100)
    let n = files_for("chainsword_swing_1");
    assert!(n >= 2, "the sheet asks for several takes of the chainsword swing, so that this test can see them differ");
    let takes: Vec<Pcm> = (1..=n).map(|i| run.wav(&format!("chainsword_swing_1_{i}.wav"))).collect();
    let mut firsts: Vec<i16> = takes.iter().map(|t| t.samples[0]).collect();
    firsts.sort_unstable();
    firsts.dedup();
    assert_eq!(firsts.len(), n, "every take is a different one");
    assert!(firsts.iter().all(|f| [1100, 2100, 3100].contains(f)), "each take is one of the three plus the extra layer: {firsts:?}");
    for t in &takes {
        assert_eq!((t.channels, t.sample_rate), (1, 44100));
        // as long as the longer of the picked sound and the extra layer (6615 frames); the end is what is left of the longer one
        let (frames, last) = match t.samples[0] {
            1100 => (6615, 100),
            2100 => (8820, 2000),
            _ => (13230, 3000),
        };
        assert_eq!((t.samples.len(), *t.samples.last().unwrap()), (frames, last));
    }

    // boltpistol_fire: mono 1000 (0.1 s) + stereo 2000/-2000 (0.2 s) = a stereo mix; the mono one is on both sides
    let shot = run.wav("boltpistol_fire_1.wav");
    assert_eq!((shot.channels, shot.sample_rate, shot.samples.len()), (2, 44100, 8820 * 2));
    assert_eq!(&shot.samples[..2], &[3000, -1000], "the sum of the layers");
    assert_eq!(&shot.samples[5000 * 2..5000 * 2 + 2], &[2000, -2000], "after the short layer ends only the long one is left");
    // the sheet asks for several takes of it but the game only has one version of it
    assert!(!run.sounds().join("boltpistol_fire_2.wav").exists());

    // chainsword_swing_2: a layer of one good file, one that is not in the zip and one that is damaged: only the good one
    let hit = run.wav("chainsword_swing_2_1.wav");
    assert_eq!((hit.samples.len(), hit.samples[0], hit.peak()), (4410, 1500, 1500));

    // chainsword_idle: the sound that is stored in the bank, and not the sound the event's stop action points to
    let idle = run.wav("chainsword_idle_1.wav");
    assert_eq!((idle.samples.len(), idle.samples[0], idle.peak()), (8820, 500, 500));
    assert!(run.report().contains("action 314: type 0x0102, not a play action, ignored"));
}

#[test]
fn the_same_install_always_gives_the_same_files() {
    let a = Run::new();
    let b = Run::new();
    assert_eq!(tree(&a.out), tree(&b.out), "the seeded generator makes the takes reproducible");
}

#[test]
fn slots_that_cannot_be_made_are_reported_and_do_not_stop_the_others() {
    let run = Run::new();
    let report = run.report();
    let table = prepare::slots().unwrap();
    assert_eq!(run.outcome.failed.len(), table.len() - 4);
    let why = |slot: &str| run.outcome.failed.iter().find(|f| f.0 == slot).unwrap_or_else(|| panic!("{slot} should have failed")).1.clone();

    // an event that the weapon bank does not have
    assert!(why("chainsword_strong").contains("has no event called \"wpn_melee_chswd_slash_1hit\" in sounds/desktop/wpn.bnk"));
    for needle in [
        "The event is NOT in sounds/desktop/wpn.bnk, which holds 6 events.",
        // names from the sheet that do exist, and from the game's own event list (the csv of the fake install)
        "The closest event names that do exist in sounds/desktop/wpn.bnk: wpn_melee_chswd_light_1hit",
        "wpn_melee_chainsword_swing",
    ] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }
    assert!(report.contains(&format!("{:#010x}", fnv1_lower("wpn_melee_chswd_slash_1hit"))), "the report names the event id");

    // an event that is in another bank
    let other = why("chainsword_swing_4");
    assert!(other.contains("sounds/desktop/wpn_melee.bnk") && other.contains("does not read yet"), "{other}");
    assert!(report.contains("It does exist in another sound bank, sounds/desktop/wpn_melee.bnk, but this tool only reads sounds/desktop/wpn.bnk."), "{report}");

    // an event whose only sound file cannot be decoded
    let broken = why("chainsword_swing_3");
    assert!(broken.contains("none of its 1 sound files could be used") && broken.contains("media 7008"), "{broken}");

    // layers that are left out are named, with the reason
    for needle in ["media 7006: LEFT OUT, there is no 7006.wem in sounds/desktop/wpn.zip", "media 7007: LEFT OUT, it could not be decoded"] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }

    // what each event reaches, and what each take is made of
    for needle in [
        "random/sequence container 210: plays one of its 3 children, picked at random",
        "layer container 211: plays all of its 2 children",
        "sound 113: media 7004, stream type 2",
        "variation 1: sound files",
        "media 7004: 0.15 s, mono, 44100 Hz, loudest sample 100 (0% of full scale), read from wpn.zip",
        "media 7010: 0.20 s, mono, 44100 Hz, loudest sample 500 (2% of full scale), read from the bank",
        "media 888: 0.10 s, mono, 44100 Hz, loudest sample 1000 (3% of full scale), read from wpn.zip",
        "media 7009: 0.20 s, stereo, 44100 Hz, loudest sample 2000 (6% of full scale), read from wpn.zip",
        "-> boltpistol_fire_1.wav: 0.20 s, stereo, 44100 Hz, loudest sample 3000 (mix of 2 layers)",
    ] {
        assert!(report.contains(needle), "report is missing {needle:?}:\n{report}");
    }

    // the takes of the chainsword swing, and the note that the game has fewer versions of the bolt pistol shot than the sheet asks for
    let last = format!("-> chainsword_swing_1_{}.wav", files_for("chainsword_swing_1"));
    assert!(report.contains(&last), "report is missing {last:?}:\n{report}");
    assert!(takes_of("boltpistol_fire") > 1);
    let few = format!("The game has only 1 different version of this sound, so 1 file was made instead of {}.", takes_of("boltpistol_fire"));
    assert!(report.contains(&few), "report is missing {few:?}:\n{report}");

    // the end of the report: the summary line and the list of what failed
    let summary = format!("prepared 4 of {} slots, {} files", table.len(), total_files());
    let at = report.find(&summary).unwrap_or_else(|| panic!("no summary line {summary:?}:\n{report}"));
    let tail = &report[at..];
    assert!(tail.contains("could NOT be prepared") && tail.contains("chainsword_swing_3: none of its") && tail.contains("boltpistol_unequip: Space Marine 2 has no event called"), "{tail}");
    for (slot, _) in MADE {
        assert!(!tail.contains(&format!("- {slot}:")), "{slot} did not fail");
    }
    // and none of the failed slots is in the index or on disk
    let listed: Vec<String> = run.index()["sounds"].as_array().unwrap().iter().map(|s| s["slot"].as_str().unwrap().to_string()).collect();
    for (slot, _) in &run.outcome.failed {
        assert!(!listed.contains(slot), "{slot} must not be in the index");
        assert!(fs::read_dir(run.sounds()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(&format!("{slot}_"))), "{slot} has no files");
    }
    // the words a player reads
    assert!(report.contains("This only READS your Space Marine 2 files. It never changes, moves or uploads anything of the game."), "{report}");
    assert!(report.contains("This usually takes less than a minute"), "{report}");
}

#[test]
fn a_second_run_replaces_what_the_first_left_and_removes_what_is_stale() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("assets");
    fake_install(&sm2);
    let opts = Opts { sm2: Some(sm2), out: out.clone(), normalize: false, ab: false };
    assert!(prepare::run(&opts).ok());
    let first = tree(&out);
    let sounds = out.join("sounds");

    // a damaged file, a take that no longer exists, a file of a slot that has no sound, a file that is not ours, old markers
    fs::write(sounds.join("chainsword_swing_1_1.wav"), b"garbage").unwrap();
    fs::write(sounds.join("chainsword_swing_1_9.wav"), b"stale take").unwrap();
    fs::write(sounds.join("chainsword_idle_start_1.wav"), b"stale: that slot has no sound now").unwrap();
    fs::write(sounds.join("notes.txt"), b"mine").unwrap();
    fs::write(sounds.join("index.json"), b"{\"format\":0}").unwrap();
    fs::write(out.join("ready.json"), b"old").unwrap();

    assert!(prepare::run(&opts).ok());
    let mut expected = first;
    expected.insert(Path::new("sounds").join("notes.txt"), b"mine".to_vec());
    assert_eq!(tree(&out), expected, "everything of ours is as after the first run; only the file that is not ours is still there");
}

#[test]
fn a_failed_rerun_never_looks_finished() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("assets");
    // the weapon bank has an event of the sheet, but its sound file is not in the zip: nothing can be made
    let mut b = Builder::new(150);
    b.sound(1, 5, 1).action(2, 0x0403, 1).event(fnv1_lower("wpn_melee_chswd_light_1hit"), &[2]);
    sound_install(&sm2, b.build(), &[("1.wem", pcm_wem(44100, &[1], 10))]);
    // what an earlier, good run left
    let sounds = out.join("sounds");
    fs::create_dir_all(&sounds).unwrap();
    fs::write(out.join("ready.json"), b"{\"format\":1}").unwrap();
    fs::write(sounds.join("index.json"), b"{\"format\":1}").unwrap();
    fs::write(sounds.join("chainsword_swing_1_1.wav"), b"old").unwrap();
    fs::write(sounds.join("notes.txt"), b"mine").unwrap();

    let outcome = prepare::run(&Opts { sm2: Some(sm2), out: out.clone(), normalize: false, ab: false });
    assert!(!outcome.ok() && outcome.prepared == 0 && !outcome.ready);
    assert!(!out.join("ready.json").exists(), "an old ready.json must not stay when the new run made nothing");
    assert!(!sounds.join("index.json").exists() && !sounds.join("chainsword_swing_1_1.wav").exists());
    assert!(sounds.join("notes.txt").exists());
    let report = flat(&fs::read_to_string(prepare::report_path(&out)).unwrap());
    assert!(report.contains("prepared 0 of") && report.contains("Nothing could be prepared") && report.contains("media 5: there is no 5.wem in sounds/desktop/wpn.zip"), "{report}");
}

#[test]
fn without_space_marine_2_nothing_is_written_but_the_report() {
    let t = tempfile::tempdir().unwrap();
    let empty = t.path().join("nothing");
    fs::create_dir_all(&empty).unwrap();
    let out = t.path().join("assets");
    let outcome = prepare::run(&Opts { sm2: Some(empty), out: out.clone(), normalize: false, ab: false });
    assert!(!outcome.ok() && !outcome.ready && outcome.prepared == 0);
    assert!(!out.exists(), "no assets folder, no ready.json");
    let report = flat(&fs::read_to_string(prepare::report_path(&out)).unwrap());
    assert!(report.contains("no .pak files found") && report.contains("Is that really the Space Marine 2 folder") && report.contains("Verify integrity"), "{report}");
    assert!(report.contains("Nothing was prepared") && report.contains("send me the report"), "{report}");

    // an install without the weapon bank
    let no_bank = t.path().join("no bank");
    common::make_zip(&no_bank.join("client_pc/root/paks/client/a.pak"), &[("ssl/x.cls", b"x".to_vec())], true);
    let outcome = prepare::run(&Opts { sm2: Some(no_bank), out: out.clone(), normalize: false, ab: false });
    assert!(!outcome.ok() && !out.exists());
    let report = flat(&fs::read_to_string(prepare::report_path(&out)).unwrap());
    assert!(report.contains("has no weapon sound bank (sounds/desktop/wpn.bnk)"), "{report}");
}

#[test]
fn nothing_is_ever_written_inside_the_game_folder() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    fake_install(&sm2);
    let before = tree(&sm2);
    // an assets folder (and with it the report folder) inside the game: refused before anything is created
    for out in [sm2.join("assets"), sm2.join("ashenmarine").join("assets"), sm2.clone()] {
        let outcome = prepare::run(&Opts { sm2: Some(sm2.clone()), out: out.clone(), normalize: false, ab: false });
        assert!(!outcome.ok() && outcome.prepared == 0 && outcome.files == 0, "{}", out.display());
        assert_eq!(tree(&sm2), before, "{}", out.display());
    }
    // the program says why
    let run = Command::new(setup_exe()).arg("prepare").arg("--sm2").arg(&sm2).arg("--out").arg(sm2.join("assets")).output().unwrap();
    assert_eq!(run.status.code(), Some(2));
    let shown = flat(&text(&run.stdout));
    assert!(shown.contains("is inside your Space Marine 2 folder") && shown.contains("--out"), "{shown}");
    assert_eq!(tree(&sm2), before);
    // a folder next to the game whose name only starts the same is fine
    let beside = t.path().join("Space Marine 2 sounds");
    assert!(prepare::run(&Opts { sm2: Some(sm2), out: beside.join("assets"), normalize: false, ab: false }).ok());
}

// ---- the program itself

fn setup_exe() -> &'static str {
    env!("CARGO_BIN_EXE_ashenmarine-setup")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

#[test]
fn the_program_exits_with_0_when_sounds_were_made() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    fake_install(&sm2);
    let out = t.path().join("assets");
    let run = Command::new(setup_exe()).arg("prepare").arg("--sm2").arg(&sm2).arg("--out").arg(&out).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stdout));
    let shown = text(&run.stdout);
    assert!(shown.contains(&format!("prepared 4 of {} slots, {} files", prepare::slots().unwrap().len(), total_files())), "{shown}");
    assert!(out.join("ready.json").is_file() && out.join("sounds/index.json").is_file());
}

#[test]
fn the_program_exits_with_2_and_writes_no_ready_json_when_it_cannot_do_it() {
    let t = tempfile::tempdir().unwrap();
    let empty = t.path().join("nothing");
    fs::create_dir_all(&empty).unwrap();
    let out = t.path().join("assets");
    let run = Command::new(setup_exe()).arg("prepare").arg("--sm2").arg(&empty).arg("--out").arg(&out).output().unwrap();
    assert_eq!(run.status.code(), Some(2));
    let shown = flat(&text(&run.stdout));
    assert!(shown.contains("no .pak files found") && shown.contains("Nothing was prepared"), "{shown}");
    assert!(!out.join("ready.json").exists());
    assert!(t.path().join("prepare-sm2/prepare-report.txt").is_file(), "the report is next to the assets folder");
}

#[test]
fn the_program_exits_with_64_for_bad_arguments_and_0_for_help() {
    for args in [vec!["prepare", "--bogus"], vec!["frobnicate"], vec!["prepare", "--out"], vec!["--sm2"], vec!["prepare", "extra"]] {
        let run = Command::new(setup_exe()).args(&args).output().unwrap();
        assert_eq!(run.status.code(), Some(64), "{args:?}");
        assert!(text(&run.stdout).contains("ashenmarine-setup prepare"), "the usage is shown for {args:?}");
    }
    let help = Command::new(setup_exe()).args(["prepare", "--help"]).output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    assert!(text(&help.stdout).contains("probe") && text(&help.stdout).contains("prepare"));
}

#[test]
fn started_from_its_own_folder_it_follows_the_hint_file_and_writes_next_to_itself() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Games").join("Space Marine 2");
    fake_install(&sm2);
    // a copy of the program in a "kit" folder, with the hint file the player would write
    let kit = t.path().join("ashenmarine");
    fs::create_dir_all(&kit).unwrap();
    let exe = kit.join(format!("ashenmarine-setup{}", std::env::consts::EXE_SUFFIX));
    fs::copy(setup_exe(), &exe).unwrap();
    fs::write(kit.join("sm2-folder.txt"), format!("\"{}\"\r\nsecond line\r\n", sm2.display())).unwrap();
    let before = tree(&sm2);

    let run = Command::new(&exe).arg("prepare").current_dir(t.path()).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", text(&run.stdout));
    assert!(kit.join("assets/ready.json").is_file() && kit.join("assets/sounds/chainsword_swing_1_1.wav").is_file());
    assert!(kit.join("prepare-sm2/prepare-report.txt").is_file());
    assert!(!t.path().join("assets").exists(), "nothing is written next to the working folder");
    assert_eq!(tree(&sm2), before);
}

/// A bank in the real v150 layout (every object readable exactly), so `prepare` takes the exact route: the sounds keep the
/// volumes the bank gives them, and the loudness step brings the loudest sound near full scale.
#[test]
fn the_exact_reading_and_the_loudness_step_work_through_the_whole_program() {
    use ashen_sm2::bnk::fnv1_lower;
    use ashen_sm2::hirc::testenc as enc;
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("ashenmarine").join("assets");
    // light_1hit: a random container over three sounds (media 7001..=7003) layered with a fourth (7004, 20 ms late), all at 0 dB
    // bolt pistol: one stereo sound (7009: 2000 / -2000) at -6.02 dB, i.e. half as loud
    let bank = enc::bank(vec![
        enc::obj(2, 1, &enc::sound(210, 7001, 0.0, 0)),
        enc::obj(2, 2, &enc::sound(210, 7002, 0.0, 0)),
        enc::obj(2, 3, &enc::sound(210, 7003, 0.0, 0)),
        enc::obj(2, 4, &enc::sound(211, 7004, 0.0, 20)),
        enc::obj(5, 210, &enc::ranseq(211, &[(1, 50), (2, 50), (3, 50)], false)),
        enc::obj(9, 211, &enc::layer(0, &[210, 4], 0.0)),
        enc::action(300, 0x0403, 211),
        enc::event(fnv1_lower("wpn_melee_chswd_light_1hit"), &[300]),
        enc::obj(2, 5, &enc::sound(0, 7009, -6.0206, 0)),
        enc::action(301, 0x0403, 5),
        enc::event(fnv1_lower("wpn_firearm_shoot_2d_bolt_pistol"), &[301]),
    ]);
    common::sound_install(
        &sm2,
        bank,
        &[
            ("7001.wem", common::pcm_wem(44100, &[1000], 4410)),
            ("7002.wem", common::pcm_wem(44100, &[2000], 8820)),
            ("7003.wem", common::pcm_wem(44100, &[3000], 13230)),
            ("7004.wem", common::pcm_wem(44100, &[100], 6615)),
            ("7009.wem", common::pcm_wem(44100, &[2000, -2000], 8820)),
        ],
    );
    let outcome = prepare::run(&Opts { sm2: Some(sm2), out: out.clone(), normalize: true, ab: false });
    assert!(outcome.ok(), "{outcome:?}");
    assert_eq!(outcome.prepared, 2, "only the two events this bank has: {outcome:?}");
    let report = flat(&fs::read_to_string(prepare::report_path(&out)).unwrap());
    assert!(report.contains("using the exact reading"), "{report}");
    assert!(report.contains("starting 20 ms late") && report.contains("played at -6.0 dB"), "{report}");
    assert!(report.contains("louder together"), "{report}");

    let wav = |name: &str| wem::decode(&fs::read(out.join("sounds").join(name)).unwrap()).unwrap();
    let shot = wav("boltpistol_fire_1.wav");
    let swings: Vec<Pcm> = (1..=files_for("chainsword_swing_1")).map(|i| wav(&format!("chainsword_swing_1_{i}.wav"))).collect();
    // the loudest swing take (3000 plus 100 once the late layer comes in) sits at 90% of full scale ...
    let loudest = swings.iter().map(|p| p.peak() as i32).max().unwrap();
    assert!((loudest - 29_490).abs() <= 3, "{loudest}");
    // ... and the shot, half as loud as 2000 = 1000 against the swing's 3100, is brought up by the same factor
    let factor = 29_490.0 / 3_100.0;
    assert!((shot.peak() as f64 - 1000.0 * factor).abs() <= 4.0, "{} vs {}", shot.peak(), 1000.0 * factor);
    assert_eq!((shot.samples[0] > 0, shot.samples[1] < 0), (true, true), "the sides keep their signs");
    // no stale temp files, and ready.json says what was made
    assert!(out.join("ready.json").is_file() && !out.join("ready.json.tmp").exists());
}

/// With both sets wanted (what the program does), `sounds` is the way kit 4 made it - every sound at full volume - and
/// `sounds-exact` has the volumes of the bank; both are indexed, `ready.json` names the second set.
#[test]
fn both_sets_are_made_and_the_first_stays_as_kit_4_made_it() {
    use ashen_sm2::bnk::fnv1_lower;
    use ashen_sm2::hirc::testenc as enc;
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("ashenmarine").join("assets");
    // one stereo sound at -6.02 dB under the bolt pistol event, one at 0 dB under a chainsword event
    let bank = enc::bank(vec![
        enc::obj(2, 5, &enc::sound(0, 7009, -6.0206, 0)),
        enc::action(301, 0x0403, 5),
        enc::event(fnv1_lower("wpn_firearm_shoot_2d_bolt_pistol"), &[301]),
        enc::obj(2, 6, &enc::sound(0, 7010, 0.0, 0)),
        enc::action(302, 0x0403, 6),
        enc::event(fnv1_lower("wpn_melee_chswd_light_1hit"), &[302]),
    ]);
    common::sound_install(&sm2, bank, &[("7009.wem", common::pcm_wem(44100, &[2000, -2000], 8820)), ("7010.wem", common::pcm_wem(44100, &[2000], 8820))]);
    let outcome = prepare::run(&Opts { sm2: Some(sm2), out: out.clone(), normalize: false, ab: true });
    assert!(outcome.ok(), "{outcome:?}");
    assert_eq!((outcome.prepared, outcome.alt_prepared), (2, 2), "{outcome:?}");

    let wav = |dir: &str, name: &str| wem::decode(&fs::read(out.join(dir).join(name)).unwrap()).unwrap();
    // the first set: every sound at full volume, as the approximate walk makes them
    assert_eq!(wav("sounds", "boltpistol_fire_1.wav").peak(), 2000);
    // the second set: the bank's -6 dB is applied to the bolt pistol
    let exact = wav("sounds-exact", "boltpistol_fire_1.wav").peak() as f64;
    assert!((exact - 1000.0).abs() <= 3.0, "{exact}");
    assert_eq!(wav("sounds-exact", "chainsword_swing_1_1.wav").peak(), 2000);

    assert!(out.join("sounds/index.json").is_file() && out.join("sounds-exact/index.json").is_file());
    let ready = fs::read_to_string(out.join("ready.json")).unwrap();
    assert!(ready.contains("\"alt_sets\":[\"sounds-exact\"]"), "{ready}");
    let report = flat(&fs::read_to_string(prepare::report_path(&out)).unwrap());
    assert!(report.contains("The second set") && report.contains("F9"), "{report}");

    // asked for one set only, or with a bank that cannot be read exactly, there is no second folder
    let t2 = tempfile::tempdir().unwrap();
    let sm2b = t2.path().join("Space Marine 2");
    let out2 = t2.path().join("assets");
    fake_install(&sm2b);
    let outcome = prepare::run(&Opts { sm2: Some(sm2b), out: out2.clone(), normalize: false, ab: true });
    assert!(outcome.ok() && outcome.alt_prepared == 0, "{outcome:?}");
    assert!(!out2.join("sounds-exact").exists());
    assert!(!fs::read_to_string(out2.join("ready.json")).unwrap().contains("alt_sets"));
}

/// An older second set must not survive a run that makes none (a stale folder would be played by the game).
#[test]
fn a_stale_second_set_is_removed_when_this_run_makes_none() {
    let t = tempfile::tempdir().unwrap();
    let sm2 = t.path().join("Space Marine 2");
    let out = t.path().join("assets");
    fake_install(&sm2);
    fs::create_dir_all(out.join("sounds-exact")).unwrap();
    fs::write(out.join("sounds-exact/index.json"), b"{\"format\":1,\"sounds\":[]}").unwrap();
    let outcome = prepare::run(&Opts { sm2: Some(sm2), out: out.clone(), normalize: false, ab: true });
    assert!(outcome.ok(), "{outcome:?}");
    assert!(!out.join("sounds-exact/index.json").exists(), "the old index of the second set is gone");
}
