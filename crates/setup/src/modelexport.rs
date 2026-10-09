//! `sm2-export-models`: OPTIONAL. Copies the template files of the Space Marine 2 chainsword and bolt pistol
//! (`tpl/wpn_*.tpl/*`, a few megabytes) out of the player's own game into a folder, so that the player can choose to send
//! them to the person who builds the converter for the models. Nothing is uploaded by this program; it only writes files
//! next to itself. The game is only read, and the output can never be inside the game's folder.
//!
//! Next to the template files go the textures the full-detail material names (the colour map and its short-suffix companions
//! such as `_nm` and `_spec`, with every data file their descriptors list), so the converter can be tried on real pictures.
//!
//! Why this exists: the structure of these files is not documented anywhere official, and a report of numbers and names (the
//! `sm2-mesh-probe` command) is not enough to write a reader for them. The copies are for analysis only - the mashup never
//! contains game files; the converter that comes out of this runs on the player's own PC.
use crate::meshprobe::texture_family;
use crate::report::{panic_text, Report};
use crate::{find_sm2, human, open_paks};
use anyhow::{bail, Result};
use ashen_common::VERSION;
use ashen_ds3data::sha256_hex;
use ashen_sm2::pak::PakSet;
use ashen_sm2::texture::{find_mip_file, TexDesc};
use ashen_sm2::tpl::Template;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

pub struct Opts {
    pub sm2: Option<PathBuf>,
    /// The folder the copies go to (a sub folder per template).
    pub out: PathBuf,
}

/// The templates to copy: the first word is looked for in the template folder names, the shortest match is taken.
const WEAPONS: [&str; 2] = ["chainsword", "bolt_pistol"];
/// A single file bigger than this, or more than this in all, is not copied (the real files are about 3 MB together).
const MAX_FILE: u64 = 48 << 20;
const MAX_TOTAL: u64 = 96 << 20;
pub const REPORT_FILE: &str = "export-report.txt";

pub fn report_path(out: &Path) -> PathBuf {
    out.join(REPORT_FILE)
}

fn template_folder(name: &str) -> String {
    name.split('/').nth(1).unwrap_or("").to_string()
}

/// A path from inside a pak, made safe to join below a folder: only plain file names are accepted.
fn plain_name(name: &str) -> Option<&str> {
    let leaf = name.rsplit('/').next()?;
    (!leaf.is_empty() && leaf != "." && leaf != ".." && !leaf.contains(['\\', ':', '\0'])).then_some(leaf)
}

/// Returns true when the files could be copied.
pub fn run(opts: &Opts) -> bool {
    let found = find_sm2(opts.sm2.as_deref());
    // nothing may ever be written inside the game's folder: not even the report
    if let Ok(f) = &found {
        if crate::prepare::is_inside(&opts.out, &f.0) {
            println!("Ashen Marine - copy the Space Marine 2 weapon model files (version {VERSION})");
            println!("  The output folder is inside your Space Marine 2 folder. This program never writes anything into the game folder, so nothing was done.");
            println!("  Please start it again with  --out \"<a folder somewhere else>\"  or move this program out of the game folder.");
            return false;
        }
    }
    let mut rep = Report::create(&report_path(&opts.out));
    rep.say(format!("Ashen Marine - copy the Space Marine 2 weapon model files (version {VERSION})"));
    rep.say_wrapped("  ", "This READS the chainsword and bolt pistol model files of your Space Marine 2 and writes plain copies of them into a folder next to this program. Nothing is uploaded and nothing in the game is changed. The copies are only for you to send me, if you want to, so that I can build the model converter.");
    rep.say_wrapped("  ", &format!("The copies and a list of them go to:  {}", opts.out.display()));

    let found = match found {
        Ok(f) => f,
        Err(e) => {
            rep.say_wrapped("  ", &format!("PROBLEM: {e:#}"));
            return false;
        }
    };
    rep.say_wrapped("  ", &format!("Space Marine 2:  {}{}", found.0.display(), found.1.as_deref().map(|b| format!("  (Steam build id {b})")).unwrap_or_default()));
    let mut paks = match open_paks(&mut rep, &found.0) {
        Ok(p) => p,
        Err(e) => {
            rep.say_wrapped("  ", &format!("PROBLEM: the game files could not be opened: {e:#}"));
            return false;
        }
    };
    let mut total = 0u64;
    let mut copied = 0usize;
    for kw in WEAPONS {
        rep.section(&format!("Copying the model files of the {kw}"));
        match catch_unwind(AssertUnwindSafe(|| weapon(&mut rep, &mut paks, kw, &opts.out, &mut total))) {
            Ok(Ok(n)) => copied += n,
            Ok(Err(e)) => rep.say_wrapped("  ", &format!("THIS STEP FAILED: {e:#}")),
            Err(p) => rep.say_wrapped("  ", &format!("THIS STEP CRASHED: {}", panic_text(&*p))),
        }
    }
    rep.section("Done");
    if copied == 0 {
        rep.say_wrapped("  ", "Nothing was copied; the message above says why.");
        return false;
    }
    rep.say_wrapped("  ", &format!("Copied {copied} files, {} in all, in {:.0} s. They are in the folder named above.", human(total), rep.elapsed()));
    true
}

fn weapon(rep: &mut Report, paks: &mut PakSet, kw: &str, out: &Path, total: &mut u64) -> Result<usize> {
    let names = paks.find(|k| k.starts_with("tpl/") && k.contains(kw));
    let Some(folder) = names.iter().map(|n| template_folder(n)).min_by_key(|f| (f.len(), f.clone())) else {
        rep.say(format!("  no template folder with \"{kw}\" in its name was found"));
        return Ok(0);
    };
    let Some(dir_name) = plain_name(&folder) else { bail!("the template folder name {folder:?} is not usable") };
    let own: Vec<String> = names.into_iter().filter(|n| template_folder(n) == folder).collect();
    rep.say(format!("  template folder  tpl/{folder}  with {} files", own.len()));
    let dir = out.join(dir_name);
    std::fs::create_dir_all(&dir).map_err(|e| anyhow::anyhow!("cannot create the output folder: {e}"))?;
    let mut n = 0;
    let mut tpl_bytes: Option<Vec<u8>> = None;
    for name in &own {
        let Some(leaf) = plain_name(name) else {
            rep.say(format!("  skipped {name:?}: not a plain file name"));
            continue;
        };
        let info = paks.info(name)?;
        if info.size > MAX_FILE || *total + info.size > MAX_TOTAL {
            rep.say(format!("  skipped {leaf} ({}): too big", human(info.size)));
            continue;
        }
        let bytes = match paks.read(name, MAX_FILE) {
            Ok(b) => b,
            Err(e) => {
                rep.say(format!("  {leaf}: could not be read ({e:#})"));
                continue;
            }
        };
        std::fs::write(dir.join(leaf), &bytes).map_err(|e| anyhow::anyhow!("cannot write {leaf}: {e}"))?;
        if leaf.ends_with(".tpl") {
            tpl_bytes = Some(bytes.clone());
        }
        *total += bytes.len() as u64;
        n += 1;
        rep.say(format!("  {:<44} {:>10}  sha256 {}", leaf, human(bytes.len() as u64), &sha256_hex(&bytes)[..16]));
    }
    if let Some(tpl) = tpl_bytes {
        n += textures(rep, paks, &tpl, &dir, total)?;
    }
    Ok(n)
}

/// Copies the textures the full-detail material names and their short-suffix companions: each descriptor and every data
/// file it lists, below `<template folder>/pct`. Returns how many files were written.
fn textures(rep: &mut Report, paks: &mut PakSet, tpl: &[u8], dir: &Path, total: &mut u64) -> Result<usize> {
    let names = Template::parse_partial(tpl).0.full_detail_texture_names();
    if names.is_empty() {
        rep.say("  the template names no texture for its full-detail model, so no textures are copied");
        return Ok(0);
    }
    let pct = dir.join("pct");
    let mut n = 0;
    for name in &names {
        let family = texture_family(paks, name);
        rep.say(format!("  texture {name:?}: {} descriptors in its family", family.len()));
        for resource in &family {
            // the descriptor, then the data files it lists
            let descriptor = match paks.read(resource, 1 << 20) {
                Ok(b) => b,
                Err(e) => {
                    rep.say(format!("    {resource}: could not be read ({e:#})"));
                    continue;
                }
            };
            let mut entries: Vec<(String, Vec<u8>)> = vec![(resource.clone(), descriptor.clone())];
            match TexDesc::parse(&String::from_utf8_lossy(&descriptor)) {
                Err(e) => rep.say(format!("    {resource}: {e:#}; only the descriptor is copied")),
                Ok(desc) => {
                    for mip in &desc.mip_maps {
                        let Some(entry) = find_mip_file(paks, mip) else {
                            rep.say(format!("    {mip}: not in any pak (the game does not ship every size)"));
                            continue;
                        };
                        match paks.info(&entry) {
                            Ok(i) if i.size <= MAX_FILE && *total + i.size <= MAX_TOTAL => match paks.read(&entry, MAX_FILE) {
                                Ok(b) => entries.push((entry, b)),
                                Err(e) => rep.say(format!("    {entry}: could not be read ({e:#})")),
                            },
                            Ok(i) => rep.say(format!("    skipped {entry} ({}): too big", human(i.size))),
                            Err(e) => rep.say(format!("    {entry}: {e:#}")),
                        }
                    }
                }
            }
            let mut size = 0u64;
            let mut count = 0;
            for (entry, bytes) in &entries {
                let Some(leaf) = plain_name(entry) else {
                    rep.say(format!("    skipped {entry:?}: not a plain file name"));
                    continue;
                };
                if *total + bytes.len() as u64 > MAX_TOTAL {
                    rep.say(format!("    skipped {leaf}: the copies would be bigger than {}", human(MAX_TOTAL)));
                    continue;
                }
                std::fs::create_dir_all(&pct).map_err(|e| anyhow::anyhow!("cannot create the output folder: {e}"))?;
                std::fs::write(pct.join(leaf), bytes).map_err(|e| anyhow::anyhow!("cannot write {leaf}: {e}"))?;
                *total += bytes.len() as u64;
                size += bytes.len() as u64;
                count += 1;
            }
            n += count;
            rep.say(format!("    {:<52} {count} files, {}", resource.rsplit('/').next().unwrap_or(resource), human(size)));
        }
    }
    Ok(n)
}
