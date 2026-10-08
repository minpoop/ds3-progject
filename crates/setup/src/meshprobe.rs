//! The model probe: a read-only structural report about the Space Marine 2 chainsword and bolt pistol template files
//! (`tpl/wpn_*.tpl/*`). Their format is not documented anywhere official, so before a converter can be written the
//! files have to be looked at: how they start, which names they mention, whether the geometry is compressed, and where
//! vertex and index buffers sit. The report is plain text (hex of the first bytes, names, numbers about the buffers); no
//! game file is copied. It never writes anything inside the game folder.
use crate::report::{panic_text, Report};
use crate::{find_sm2, human, open_paks};
use anyhow::Result;
use ashen_common::VERSION;
use ashen_sm2::geom::{self, PosKind};
use ashen_sm2::pak::PakSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

pub struct Opts {
    pub sm2: Option<PathBuf>,
    pub out: PathBuf,
}

/// The templates to look at: the first word is looked for in the template folder names, the shortest match is taken.
const WEAPONS: [&str; 2] = ["chainsword", "bolt_pistol"];
/// Files bigger than this are described but not searched for geometry (the search is quick, but there is no need).
const MAX_SCAN: u64 = 48 << 20;

pub fn report_path(out: &Path) -> PathBuf {
    out.join("mesh-report.txt")
}

fn template_folder(name: &str) -> String {
    name.split('/').nth(1).unwrap_or("").to_string()
}

fn looks_like_text(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    !head.is_empty() && head.iter().filter(|b| b.is_ascii_graphic() || b.is_ascii_whitespace()).count() * 10 >= head.len() * 9
}

/// Returns true when the probe could run.
pub fn run(opts: &Opts) -> bool {
    let mut rep = Report::create(&report_path(&opts.out));
    rep.say(format!("Ashen Marine - Space Marine 2 model probe v{VERSION} (read-only)"));
    rep.say("  This READS the weapon model files of your Space Marine 2 and writes numbers and names about them (no pictures, no copies of the files).");
    rep.say_wrapped("  ", &format!("The report is written to:  {}", report_path(&opts.out).display()));

    let found = match find_sm2(opts.sm2.as_deref()) {
        Ok(f) => f,
        Err(e) => {
            rep.say_wrapped("  ", &format!("PROBLEM: {e:#}"));
            return false;
        }
    };
    rep.say_wrapped("  ", &format!("Space Marine 2:  {}", found.0.display()));
    let mut paks = match open_paks(&mut rep, &found.0) {
        Ok(p) => p,
        Err(e) => {
            rep.say_wrapped("  ", &format!("PROBLEM: the game files could not be opened: {e:#}"));
            return false;
        }
    };
    for kw in WEAPONS {
        rep.section(&format!("Model files of the {kw}"));
        match catch_unwind(AssertUnwindSafe(|| weapon(&mut rep, &mut paks, kw))) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => rep.say_wrapped("  ", &format!("THIS STEP FAILED: {e:#}")),
            Err(p) => rep.say_wrapped("  ", &format!("THIS STEP CRASHED: {}", panic_text(&*p))),
        }
    }
    rep.section("Done");
    rep.say(format!("  finished in {:.0} s. Send the file  mesh-report.txt  back (Send-Logs.bat includes it).", rep.elapsed()));
    true
}

fn weapon(rep: &mut Report, paks: &mut PakSet, kw: &str) -> Result<()> {
    let names = paks.find(|k| k.starts_with("tpl/") && k.contains(kw));
    let Some(folder) = names.iter().map(|n| template_folder(n)).min_by_key(|f| (f.len(), f.clone())) else {
        rep.say(format!("  no template folder with \"{kw}\" in its name was found"));
        return Ok(());
    };
    let own: Vec<String> = names.into_iter().filter(|n| template_folder(n) == folder).collect();
    rep.say(format!("  template folder  tpl/{folder}  with {} files:", own.len()));
    for n in &own {
        if let Ok(i) = paks.info(n) {
            rep.say(format!("      {:<44} {:>10}  stored: {}", n.rsplit('/').next().unwrap_or(n), human(i.size), if i.stored { "yes" } else { "compressed" }));
        }
    }
    for n in &own {
        let short = n.rsplit('/').next().unwrap_or(n).to_string();
        let info = paks.info(n)?;
        rep.detail("");
        rep.detail(format!("---- {short} ({}) ----", human(info.size)));
        if info.size > MAX_SCAN {
            rep.detail("  too big to search; only the start is shown");
        }
        let bytes = match paks.read(n, MAX_SCAN.max(4096)) {
            Ok(b) => b,
            Err(e) => {
                rep.detail(format!("  could not be read: {e:#}"));
                continue;
            }
        };
        describe(rep, &short, &bytes);
    }
    Ok(())
}

fn describe(rep: &mut Report, short: &str, bytes: &[u8]) {
    if looks_like_text(bytes) {
        // descriptors are text: show them (the big one, lods_base, only its start)
        let limit = if bytes.len() > 8192 { 6000 } else { bytes.len() };
        rep.detail(format!("  text, {} bytes; {}:", bytes.len(), if limit < bytes.len() { "the first part" } else { "all of it" }));
        for l in String::from_utf8_lossy(&bytes[..limit]).lines().take(160) {
            rep.detail(format!("    | {l}"));
        }
        return;
    }
    rep.detail(format!("  binary, {} bytes; how it starts:", bytes.len()));
    let data_file = short.ends_with(".tpl_data");
    for l in geom::hexdump(bytes, 0, if data_file { 128 } else { 320 }) {
        rep.detail(format!("    {l}"));
    }
    let entropy = geom::entropy_blocks(bytes, 16);
    rep.detail(format!("  entropy per sixteenth (8.0 = looks compressed): {}", entropy.iter().map(|e| format!("{e:.1}")).collect::<Vec<_>>().join(" ")));

    // names only: runs of printable characters that are mostly letters (numbers stored as floats look like text too)
    let wordy = |s: &str| s.chars().filter(|c| c.is_ascii_alphabetic()).count() * 10 >= s.chars().count() * 7 && s.chars().filter(|c| c.is_ascii_alphabetic()).count() >= 4;
    let strings: Vec<(usize, String)> = if data_file { Vec::new() } else { geom::ascii_strings(bytes, 5, 4000).into_iter().filter(|(_, s)| wordy(s)).take(220).collect() };
    if !strings.is_empty() {
        rep.detail(format!("  names and words in the file ({} shown):", strings.len()));
        for (at, s) in &strings {
            rep.detail(format!("    {at:>9}  {}", s.chars().take(100).collect::<String>()));
        }
        // the neighbourhood of the first few names shows how the file is put together
        if short.ends_with(".tpl") || short.ends_with(".cdt") || short.ends_with(".geom_dbg") {
            for (at, s) in strings.iter().filter(|(_, s)| s.starts_with("obj") || s.contains("GEOM") || s.contains("MNG") || s.contains("VBUFFER")).take(24) {
                rep.detail(format!("  around {s:?} at {at}:"));
                for l in geom::hexdump(bytes, at.saturating_sub(16), 96) {
                    rep.detail(format!("    {l}"));
                }
            }
        }
    }

    // geometry: only the data files that can hold it
    if short.ends_with(".tpl_data") || short.ends_with(".tpl") || short.ends_with(".cdt") {
        search_geometry(rep, bytes);
    }
}

fn search_geometry(rep: &mut Report, bytes: &[u8]) {
    rep.detail("  searching for vertex and index buffers by what they look like ...");
    let mut verts = Vec::new();
    for kind in [PosKind::F32, PosKind::F16, PosKind::I16] {
        let runs = geom::find_vertex_runs(bytes, kind);
        rep.detail(format!("  vertex position candidates, {}: {}", kind.name(), runs.len()));
        for r in &runs {
            rep.detail(format!(
                "    at {:>9}  stride {:>2}  {:>6} records  box {:.3?}..{:.3?}  smoothness {:.3}",
                r.offset, r.stride, r.count, r.min, r.max, r.smoothness
            ));
            // the first records and the bytes right before the run show the other attributes and what introduces the buffer
            rep.detail("      the bytes just before it, and its first records:");
            let from = r.offset.saturating_sub(48);
            for l in geom::hexdump(bytes, from, (r.offset - from) + r.stride * 3) {
                rep.detail(format!("        {l}"));
            }
        }
        verts.extend(runs);
    }
    let indices = geom::find_index_runs(bytes);
    rep.detail(format!("  triangle index candidates: {}", indices.len()));
    for i in &indices {
        rep.detail(format!("    at {:>9}  {} bit  {:>6} triangles  indices {}..{}", i.offset, i.bits, i.triangles, i.min_index, i.max_index));
        let from = i.offset.saturating_sub(32);
        for l in geom::hexdump(bytes, from, (i.offset - from) + 48) {
            rep.detail(format!("        {l}"));
        }
        let pairs: Vec<String> = geom::pair_up(i, &verts).iter().map(|v| format!("{} at {} (stride {}, {} records)", v.kind.name(), v.offset, v.stride, v.count)).collect();
        if !pairs.is_empty() {
            rep.detail(format!("      could belong with: {}", pairs.join("; ")));
        }
    }
}
