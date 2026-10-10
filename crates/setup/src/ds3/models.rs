//! `ds3-models`: the weapon model swap. For each of the two weapons it reads the Space Marine 2 shape and picture, the Dark
//! Souls III weapon container that is replaced, runs the swap (see `ashen_ds3data::weaponswap`: every step checked, failing
//! closed), writes a picture of the result and a candidate container.
//!
//! Without `--install` it is a dry run: the candidates go into the report folder next to the report and nothing is put where
//! the game would load it. With `--install` the finished containers go to `<mod>/parts/` (ModEngine2 loads them instead of
//! the archived ones), listed in `ashenmarine-models.json`; the files an earlier run wrote are removed first, so a stale model
//! never stays when something is wrong. With `--remove` nothing is made: the files an earlier `--install` wrote are taken out
//! again (only the ones the manifest lists), so the game shows its own weapon models. The games' folders are only read.
use super::{find_step, json_text, keys_step, leaf, name_of, read_valid, safe_relative, shown, stage, write_atomic, Found, MAX_MODEL_FILE};
use crate::meshprobe::{texture_family, texture_stem};
use crate::report::{panic_text, Report};
use crate::{find_ds3, find_sm2, human, open_paks};
use anyhow::Result;
use ashen_common::weapons::{sheet_weapons, ModelHints};
use ashen_common::VERSION;
use ashen_ds3data::dcx;
use ashen_ds3data::dds::Image as Rgba;
use ashen_ds3data::install::find_game_dir;
use ashen_ds3data::sha256_hex;
use ashen_sm2::pak::PakSet;
use ashen_sm2::texture;
use ashen_sm2::tpl::Template;
use crate::weaponmodel::{make_weapon_container, Sm2Shape};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

pub const MODELS_REPORT_FILE: &str = "ds3-models-report.txt";
/// The report of `--remove`.
pub const MODELS_REMOVE_REPORT_FILE: &str = "ds3-models-remove.txt";
/// The manifest in the mod folder, next to the finished containers.
pub const MODELS_MANIFEST_FILE: &str = "ashenmarine-models.json";

pub struct ModelsOpts {
    pub ds3: Option<PathBuf>,
    pub sm2: Option<PathBuf>,
    pub keys: Option<PathBuf>,
    /// The folder the program lives in (`{data}`).
    pub data: PathBuf,
    /// The folder the report, the pictures and (in a dry run) the candidates go to.
    pub out: PathBuf,
    /// The ModEngine2 mod folder (`{data}/mod`).
    pub mod_dir: PathBuf,
    /// Write the finished containers where the game loads them.
    pub install: bool,
    /// Take out what an earlier `--install` put there; nothing is made.
    pub remove: bool,
}

/// How a run ended.
#[derive(Debug, Default)]
pub struct ModelsOutcome {
    /// The containers that passed every check (what was made).
    pub ready: Vec<String>,
    /// What could not be made, with the reason.
    pub failed: Vec<(String, String)>,
    /// Containers written below `<mod>/parts` (only with `--install`).
    pub installed: usize,
    /// Files of an earlier run that were removed.
    pub removed_stale: usize,
    /// The run only took models out (`--remove`).
    pub removal_only: bool,
}

impl ModelsOutcome {
    /// A trial or install run is done when something was made; a removal is done when nothing went wrong.
    pub fn ok(&self) -> bool {
        if self.removal_only {
            self.failed.is_empty()
        } else {
            !self.ready.is_empty()
        }
    }
}

/// The two weapons: a name, a word in the Space Marine 2 template folder name, and the Dark Souls III containers it replaces
/// (the right-hand and the left-hand file; one of them may not exist).
const JOBS: [(&str, &str, [&str; 2]); 2] = [
    ("chainsword", "chainsword", ["/parts/wp_a_0200.partsbnd.dcx", "/parts/wp_a_0200_l.partsbnd.dcx"]),
    ("bolt pistol", "bolt_pistol", ["/parts/wp_a_1409.partsbnd.dcx", "/parts/wp_a_1409_l.partsbnd.dcx"]),
];

/// How long the new shape is compared with the weapon it replaces (1: the same length, so the reach the moves were made for).
const LENGTH_FACTOR: f32 = 1.0;
/// Texture coordinates of the Space Marine 2 reader run the other way up from the model files of Dark Souls III.
const FLIP_V: bool = true;
const MAX_TEMPLATE_FILE: u64 = 64 << 20;

/// What was found in the Space Marine 2 install for one weapon, and how the weapons sheet says it is placed.
struct Sm2Weapon {
    shape: Sm2Shape,
    picture: Option<Rgba>,
    hints: ModelHints,
}

/// The `model_*` columns of the weapons sheet for the weapon whose id is `id` (the sheet's rows are named like the jobs below);
/// none when the sheet has no such row.
fn sheet_hints(id: &str) -> ModelHints {
    sheet_weapons().into_iter().find(|w| w.id == id).map(|w| w.model).unwrap_or_default()
}

fn load_sm2_weapon(rep: &mut Report, paks: &mut PakSet, kw: &str) -> Result<Sm2Weapon, String> {
    let names = paks.find(|k| k.starts_with("tpl/") && k.contains(kw));
    let folder = names.iter().filter_map(|n| n.split('/').nth(1).map(str::to_string)).min_by_key(|f| (f.len(), f.clone())).ok_or_else(|| format!("no template folder with \"{kw}\" in its name was found in Space Marine 2"))?;
    let own: Vec<&String> = names.iter().filter(|n| n.split('/').nth(1) == Some(folder.as_str())).collect();
    let tpl_name = own.iter().find(|n| n.ends_with(".tpl")).ok_or("the template folder has no .tpl")?;
    let data_name = own.iter().find(|n| n.ends_with(".tpl_data")).ok_or("the template folder has no .tpl_data")?;
    let tpl = paks.read(tpl_name, MAX_TEMPLATE_FILE).map_err(|e| format!("{tpl_name} cannot be read: {e:#}"))?;
    let data = paks.read(data_name, MAX_TEMPLATE_FILE).map_err(|e| format!("{data_name} cannot be read: {e:#}"))?;
    let template = Template::parse(&tpl).map_err(|e| format!("the template {tpl_name} is not read completely: {e}"))?;
    let shape = Sm2Shape::load(&template, &data)?;
    rep.say(format!("  Space Marine 2 template  tpl/{folder}: {} vertices, {} triangles; the grip comes from {}", shape.positions.len(), shape.triangles.len(), shape.grip_source));
    // the picture of the material
    let picture = match shape.texture.clone() {
        None => {
            rep.say("  the material names no texture, so the new weapon gets a plain colour");
            None
        }
        Some(name) => match texture_family(paks, &name).into_iter().find(|r| texture_stem(r) == name) {
            None => {
                rep.say_wrapped("  ", &format!("NOTE: no picture named {name:?} was found in Space Marine 2, so the new weapon gets a plain colour."));
                None
            }
            Some(resource) => match texture::load_top(paks, &resource).map_err(|e| format!("{e:#}")).and_then(|t| t.decode().map(|img| (t.desc.format, img)).map_err(|e| format!("{e:#}"))) {
                Err(why) => {
                    rep.say_wrapped("  ", &format!("NOTE: the picture {resource} could not be read ({why}), so the new weapon gets a plain colour."));
                    None
                }
                Ok((format, img)) => {
                    rep.say(format!("  picture {resource}: {} {}x{}", texture::format_name(format), img.width, img.height));
                    Rgba::new(img.width as usize, img.height as usize, img.rgba).ok()
                }
            },
        },
    };
    Ok(Sm2Weapon { shape, picture, hints: sheet_hints(kw) })
}

/// The container of a path made new: (the DCX bytes, report lines), or why not. `dir` gets the picture of the result.
fn make_container(rep: &mut Report, found: &Found, weapon: &Sm2Weapon, label: &str, dir: &Path) -> Result<Vec<u8>, String> {
    let made = make_weapon_container(&found.decoded, &weapon.shape, weapon.picture.as_ref(), LENGTH_FACTOR, FLIP_V, &weapon.hints, &mut |line| rep.say_wrapped("    ", line))?;
    if let Some(png) = &made.picture {
        let file = dir.join(format!("{label}-overlay.png"));
        match std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&file, png)) {
            Ok(()) => rep.say(format!("    picture of the result (the weapon that is replaced in orange, the new shape on top): {}", shown(&file, dir.parent().unwrap_or(dir)))),
            Err(e) => rep.say(format!("    the picture could not be written ({e})")),
        }
    }
    let encoded = dcx::encode(&made.container, &found.dcx_info).map_err(|e| format!("the container cannot be packed as DCX: {e}"))?;
    match dcx::decode(&encoded) {
        Ok((back, _)) if back == made.container => {}
        _ => return Err("the packed DCX does not unpack to the container that was packed".to_string()),
    }
    rep.say(format!("    check: packed as DCX ({}) and unpacked again: identical", found.dcx_info.variant_name()));
    Ok(encoded)
}

/// Removes the containers an earlier `--install` run wrote (the manifest lists them).
fn remove_earlier(rep: &mut Report, mod_dir: &Path) -> usize {
    let manifest = mod_dir.join(MODELS_MANIFEST_FILE);
    let Ok(text) = std::fs::read_to_string(&manifest) else { return 0 };
    let mut removed = 0;
    let listed: Vec<String> = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["files"].as_array().map(|a| a.iter().filter_map(|f| f["path"].as_str().map(str::to_string)).collect())).unwrap_or_default();
    for rel in listed {
        let Some(parts) = safe_relative(&rel) else { continue };
        let path = parts.iter().fold(mod_dir.to_path_buf(), |p, part| p.join(part));
        match std::fs::remove_file(&path) {
            Ok(()) => {
                removed += 1;
                rep.say(format!("  Removed the model of an earlier run ({}).", shown(&path, mod_dir)));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => rep.say_wrapped("  ", &format!("WARNING: the model of an earlier run ({}) could not be deleted ({e}). Is Dark Souls III running? Close it and run this again, or delete that file by hand.", shown(&path, mod_dir))),
        }
    }
    let _ = std::fs::remove_file(&manifest);
    removed
}

fn manifest_text(files: &[(String, usize, String, String)]) -> String {
    let list: Vec<String> = files.iter().map(|(path, bytes, sha, source)| format!("{{\"path\":{},\"bytes\":{bytes},\"sha256\":{},\"weapon\":{}}}", json_text(path), json_text(sha), json_text(source))).collect();
    format!("{{\"format\":1,\"tool\":{},\"files\":[{}]}}", json_text(&format!("ashenmarine-setup {VERSION}")), list.join(","))
}

/// `ds3-models`. See the module notes.
pub fn models(opts: &ModelsOpts) -> ModelsOutcome {
    let mut outcome = ModelsOutcome::default();
    // nothing may ever be written inside a game's folder
    let ds3_root = find_ds3(opts.ds3.as_deref()).ok();
    let sm2_root = find_sm2(opts.sm2.as_deref()).ok().map(|f| f.0);
    for (what, dir) in [("report", &opts.out), ("mod", &opts.mod_dir)] {
        let inside_ds3 = ds3_root.as_ref().is_some_and(|r| crate::prepare::is_inside(dir, r) || find_game_dir(r).is_ok_and(|g| crate::prepare::is_inside(dir, &g)));
        let inside_sm2 = sm2_root.as_ref().is_some_and(|r| crate::prepare::is_inside(dir, r));
        if inside_ds3 || inside_sm2 {
            println!("Ashen Marine - the weapon models (version {VERSION})");
            println!("  The {what} folder ({}) is inside a game's folder. This program never writes anything into a game folder, so nothing was done.", name_of(dir));
            println!("  Please start it again with the other folder chosen by you (--out / --mod), or move this program out of the game folder.");
            outcome.failed.push(("start".to_string(), "a folder to write to is inside a game folder".to_string()));
            return outcome;
        }
    }
    if opts.remove {
        outcome.removal_only = true;
        let report = opts.out.join(MODELS_REMOVE_REPORT_FILE);
        let mut rep = Report::create(&report);
        rep.say(format!("Ashen Marine - take the new weapon models out again (version {VERSION})"));
        rep.say_wrapped("  ", "This deletes the weapon model files that an earlier run of this program put into the mod folder (only the ones listed in ashenmarine-models.json). Dark Souls III then shows its own weapon models again. Nothing else is changed.");
        outcome.removed_stale = remove_earlier(&mut rep, &opts.mod_dir);
        if outcome.removed_stale == 0 {
            rep.say("  There was nothing to remove: no weapon model of this program is in the mod folder.");
        } else {
            rep.say_wrapped("  ", &format!("Removed {} file(s). Dark Souls III shows its own weapon models again the next time you press Play.", outcome.removed_stale));
        }
        return outcome;
    }
    let report = opts.out.join(MODELS_REPORT_FILE);
    let mut rep = Report::create(&report);
    rep.say(format!("Ashen Marine - the weapon models (version {VERSION})"));
    rep.say_wrapped("  ", "This READS your Dark Souls III archives and your Space Marine 2 files and builds the new weapon models in memory. It never changes either game.");
    if opts.install {
        rep.say_wrapped("  ", "The finished models are put in the mod folder, where Dark Souls III loads them instead of its own.");
    } else {
        rep.say_wrapped("  ", "This is a TRIAL RUN: the models are only checked and drawn (a picture and a candidate file go next to the report). Nothing is put where the game would load it.");
    }
    rep.say_wrapped("  ", &format!("The report is written to:  {}", shown(&report, opts.out.parent().unwrap_or(&opts.out))));

    if opts.install {
        rep.section("Models of an earlier run");
        outcome.removed_stale = remove_earlier(&mut rep, &opts.mod_dir);
        if outcome.removed_stale == 0 {
            rep.say("  none");
        }
    }
    let Ok(root) = stage(&mut rep, "1/5 Finding Dark Souls III", |rep| find_step(rep, opts.ds3.as_deref())) else { return finish(rep, outcome) };
    let Ok(sources) = stage(&mut rep, "2/5 The program file and the archive keys", |rep| keys_step(rep, &root, opts.keys.as_deref(), &opts.data)) else { return finish(rep, outcome) };
    let Ok(install) = stage(&mut rep, "3/5 The game archives", |rep| super::archives_step(rep, sources)) else { return finish(rep, outcome) };
    let Ok(mut paks) = stage(&mut rep, "4/5 Space Marine 2", |rep| {
        let (root, _) = find_sm2(opts.sm2.as_deref())?;
        rep.say(format!("  Space Marine 2 folder: {}", name_of(&root)));
        open_paks(rep, &root)
    }) else {
        return finish(rep, outcome);
    };
    rep.section("5/5 The weapon models");
    let mut made: Vec<(String, usize, String, String)> = Vec::new();
    for (name, kw, containers) in JOBS {
        rep.say(format!("== {name} =="));
        let weapon = match catch_unwind(AssertUnwindSafe(|| load_sm2_weapon(&mut rep, &mut paks, kw))) {
            Ok(Ok(w)) => w,
            Ok(Err(why)) => {
                rep.say_wrapped("  ", &format!("NOT DONE: {why}"));
                outcome.failed.push((name.to_string(), why));
                continue;
            }
            Err(p) => {
                let why = format!("this part of the program crashed ({})", panic_text(&*p));
                rep.say_wrapped("  ", &format!("NOT DONE: {why}"));
                outcome.failed.push((name.to_string(), why));
                continue;
            }
        };
        for path in containers {
            let file = leaf(path).to_string();
            rep.say(format!("  {path}:"));
            let found = match read_valid(&install, path, MAX_MODEL_FILE) {
                Ok(f) => f,
                Err(why) => {
                    // the left-hand file may simply not exist: that is a note, not a failure
                    let missing = why.contains("is not in any archive");
                    rep.say_wrapped("    ", &format!("{}: {why}", if missing { "NOTE" } else { "NOT DONE" }));
                    if !missing {
                        outcome.failed.push((file, why));
                    }
                    continue;
                }
            };
            rep.say(format!("    found in {}, {}, DCX {}", found.archive, human(found.dcx_len as u64), found.dcx_info.variant_name()));
            let label = file.trim_end_matches(".partsbnd.dcx").to_string();
            let result = catch_unwind(AssertUnwindSafe(|| make_container(&mut rep, &found, &weapon, &label, &opts.out))).unwrap_or_else(|p| Err(format!("this part of the program crashed ({})", panic_text(&*p))));
            match result {
                Err(why) => {
                    rep.say_wrapped("    ", &format!("NOT DONE: {why}"));
                    outcome.failed.push((file, why));
                }
                Ok(bytes) => {
                    let target = if opts.install { opts.mod_dir.join("parts").join(&file) } else { opts.out.join("candidates").join(&file) };
                    match write_atomic(&target, &bytes) {
                        Err(e) => {
                            let why = format!("cannot be written ({e})");
                            rep.say_wrapped("    ", &format!("NOT DONE: {why}"));
                            outcome.failed.push((file, why));
                        }
                        Ok(()) => {
                            rep.say_wrapped("    ", &format!("READY: {} bytes written to {}", bytes.len(), shown(&target, opts.mod_dir.parent().unwrap_or(&opts.mod_dir))));
                            if opts.install {
                                outcome.installed += 1;
                                made.push((format!("parts/{file}"), bytes.len(), sha256_hex(&bytes), name.to_string()));
                            }
                            outcome.ready.push(file);
                        }
                    }
                }
            }
        }
    }
    if opts.install && !made.is_empty() {
        if let Err(e) = write_atomic(&opts.mod_dir.join(MODELS_MANIFEST_FILE), manifest_text(&made).as_bytes()) {
            rep.say_wrapped("  ", &format!("WARNING: the list of the models ({MODELS_MANIFEST_FILE}) could not be written ({e}); an older run's models cannot be removed automatically."));
        }
    }
    finish(rep, outcome)
}

fn finish(mut rep: Report, outcome: ModelsOutcome) -> ModelsOutcome {
    rep.section("Done");
    rep.say_wrapped(
        "  ",
        &format!("{} model file(s) made, {} not made{}. Nothing of either game was changed.", outcome.ready.len(), outcome.failed.len(), if outcome.installed > 0 { format!(", {} put in the mod folder", outcome.installed) } else { String::new() }),
    );
    for (what, why) in &outcome.failed {
        rep.say_wrapped("  ", &format!("- {what}: {why}"));
    }
    rep.say(format!("  Finished in {:.0} s.", rep.elapsed()));
    outcome
}
