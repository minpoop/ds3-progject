//! The model probe: a read-only report about the Space Marine 2 chainsword and bolt pistol templates
//! (`tpl/wpn_*.tpl/*`). It reads each template with the same reader the model converter uses and says what came out: the
//! objects and detail levels, the full-detail model (one merged sub mesh with its counts, bounds and texture coordinate
//! ranges) and the textures its material names, with what the game's archives hold for them. The report is plain text
//! (names and numbers); no game file is copied. It never writes anything inside the game folder.
use crate::report::{panic_text, Report};
use crate::{find_sm2, human, open_paks};
use anyhow::Result;
use ashen_common::VERSION;
use ashen_sm2::mesh::{bounds, decode_sub_mesh, DecodedMesh};
use ashen_sm2::pak::PakSet;
use ashen_sm2::texture::{self, format_name};
use ashen_sm2::tpl::Template;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

pub struct Opts {
    pub sm2: Option<PathBuf>,
    pub out: PathBuf,
}

/// The templates to look at: the first word is looked for in the template folder names, the shortest match is taken.
const WEAPONS: [&str; 2] = ["chainsword", "bolt_pistol"];
/// A template file bigger than this is not read (the real ones are a few MB at most).
const MAX_TEMPLATE_FILE: u64 = 64 << 20;
/// The most texture descriptors of one family that are listed (the count is always given).
const MAX_LISTED: usize = 40;
/// The longest ending after the texture name that still counts as the same family (`_nm`, `_spec`, ...).
const MAX_FAMILY_SUFFIX: usize = 8;

pub fn report_path(out: &Path) -> PathBuf {
    out.join("mesh-report.txt")
}

fn template_folder(name: &str) -> String {
    name.split('/').nth(1).unwrap_or("").to_string()
}

/// The file name of a pak entry without the folders and the `.pct.resource` ending.
fn texture_stem(entry: &str) -> &str {
    let leaf = entry.rsplit('/').next().unwrap_or(entry);
    leaf.strip_suffix(".pct.resource").unwrap_or(leaf)
}

/// The descriptors of a texture and its short-suffix companions (`<name>`, `<name>_nm`, `<name>_spec`, ...), shortest first.
/// Skins and colour variants (`<name>_artificer_01_01_red`) have longer endings and are not in the family.
pub fn texture_family(paks: &PakSet, name: &str) -> Vec<String> {
    let mut found = paks.find(|k| {
        if !k.ends_with(".pct.resource") {
            return false;
        }
        let stem = texture_stem(k);
        stem == name || stem.strip_prefix(name).and_then(|rest| rest.strip_prefix('_')).is_some_and(|rest| !rest.is_empty() && rest.len() <= MAX_FAMILY_SUFFIX && !rest.chars().all(|c| c.is_ascii_digit()))
    });
    found.sort_by_key(|k| (texture_stem(k).len(), k.clone()));
    found
}

/// Returns true when the probe could run.
pub fn run(opts: &Opts) -> bool {
    let found = find_sm2(opts.sm2.as_deref());
    // nothing may ever be written inside the game's folder: not even the report
    if let Ok(f) = &found {
        if crate::prepare::is_inside(&opts.out, &f.0) {
            println!("Ashen Marine - Space Marine 2 model probe v{VERSION} (read-only)");
            println!("  The report folder is inside your Space Marine 2 folder. This program never writes anything into the game folder, so nothing was done.");
            println!("  Please start it again with  --out \"<a folder somewhere else>\"  or move this program out of the game folder.");
            return false;
        }
    }
    let mut rep = Report::create(&report_path(&opts.out));
    rep.say(format!("Ashen Marine - Space Marine 2 model probe v{VERSION} (read-only)"));
    rep.say("  This READS the weapon model files of your Space Marine 2 and writes numbers and names about them (no pictures, no copies of the files).");
    rep.say_wrapped("  ", &format!("The report is written to:  {}", report_path(&opts.out).display()));

    let found = match found {
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

fn fixed(v: f32) -> String {
    format!("{v:.3}")
}

fn range(lo: f32, hi: f32) -> String {
    format!("{}..{}", fixed(lo), fixed(hi))
}

/// "x -0.093..0.093, y ..., z ..." of the positions, in the template's own units (metres).
fn bounds_text(mesh: &DecodedMesh) -> String {
    match bounds(mesh) {
        Some((lo, hi)) => format!("x {}, y {}, z {}", range(lo[0], hi[0]), range(lo[1], hi[1]), range(lo[2], hi[2])),
        None => "no vertices".to_string(),
    }
}

fn uv_text(mesh: &DecodedMesh) -> String {
    if mesh.uvs.is_empty() {
        return "no texture coordinates".to_string();
    }
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for uv in &mesh.uvs {
        for a in 0..2 {
            lo[a] = lo[a].min(uv[a]);
            hi[a] = hi[a].max(uv[a]);
        }
    }
    format!("texture coordinates u {}, v {}", range(lo[0], hi[0]), range(lo[1], hi[1]))
}

/// What the vertices of a decoded mesh carry, in words.
fn carries_text(mesh: &DecodedMesh) -> String {
    let n = mesh.positions.len();
    let has = |len: usize| if len == n && n > 0 { "yes" } else { "no" };
    format!("normals {}, tangents {}, texture coordinates {}, bone numbers {}, bone weights {}", has(mesh.normals.len()), has(mesh.tangents.len()), has(mesh.uvs.len()), has(mesh.bones.len()), has(mesh.weights.len()))
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
    let tpl_name = own.iter().find(|n| n.ends_with(".tpl")).cloned();
    let data_name = own.iter().find(|n| n.ends_with(".tpl_data")).cloned();
    let (Some(tpl_name), Some(data_name)) = (tpl_name, data_name) else {
        rep.say("  the folder has no .tpl and .tpl_data pair, so the model cannot be read");
        return Ok(());
    };
    let tpl = paks.read(&tpl_name, MAX_TEMPLATE_FILE)?;
    let data = paks.read(&data_name, MAX_TEMPLATE_FILE)?;
    let (template, problem) = Template::parse_partial(&tpl);
    match &problem {
        None => rep.say(format!("  the .tpl was read to its last byte ({} bytes)", tpl.len())),
        Some(e) => rep.say_wrapped("  ", &format!("THE .tpl WAS NOT READ COMPLETELY: {e} (read {} of {} bytes). The converter cannot use this file until the reader is updated.", template.end_at, tpl.len())),
    }
    rep.say(format!(
        "  name {:?}; {} bones; {} animations; {} detail levels defined {:?}",
        template.name.as_deref().unwrap_or("-"),
        template.skin.as_ref().map_or(0, |s| s.bone_count),
        template.anim_sequences.len(),
        template.lods.len(),
        template.lods.iter().map(|l| (l.index, l.object)).collect::<Vec<_>>()
    ));
    let Some(g) = &template.geometry else {
        rep.say("  the file holds no geometry");
        return Ok(());
    };
    rep.say(format!("  geometry: {} objects, {} buffers, {} meshes, {} sub meshes", g.objects.len(), g.buffers.len(), g.meshes.len(), g.sub_meshes.len()));
    let full = template.full_detail_sub_meshes();
    if full.is_empty() {
        rep.say("  NO FULL-DETAIL MODEL: no sub mesh belongs to the object that detail level 0 names, so the converter would have nothing to use");
    }
    let mut texture_names: Vec<String> = Vec::new();
    for &i in &full {
        let sub = &g.sub_meshes[i];
        let object = g.objects.iter().find(|o| i32::from(o.id) == i32::from(sub.node));
        rep.say(format!("  full-detail sub mesh {i}: object {} ({})", sub.node, object.and_then(|o| o.name.as_deref()).unwrap_or("no name")));
        match decode_sub_mesh(g, &data, i) {
            Err(e) => rep.say_wrapped("    ", &format!("COULD NOT BE READ: {e}")),
            Ok(m) => {
                rep.say(format!("    {} vertices, {} triangles; {}", m.positions.len(), m.triangles.len(), bounds_text(&m)));
                rep.say(format!("    {}", uv_text(&m)));
                rep.say(format!("    carries: {}", carries_text(&m)));
                let longest = bounds(&m).map(|(lo, hi)| (0..3).map(|a| hi[a] - lo[a]).fold(0f32, f32::max)).unwrap_or(0.0);
                rep.say(format!("    longest side {} m", fixed(longest)));
            }
        }
        let tex = sub.texture_name().unwrap_or("");
        let mtl = sub.shader_name().unwrap_or("");
        rep.say(format!("    material: texture {tex:?}, shader {mtl:?}, {} entries", sub.material.len()));
        for (k, v) in sub.material.iter().take(12) {
            rep.detail(format!("      {k} = {}", v.show().chars().take(240).collect::<String>()));
        }
        if !tex.is_empty() && !texture_names.iter().any(|t| t == tex) {
            texture_names.push(tex.to_string());
        }
    }
    // the other detail levels: only their size, so a change of the layout shows
    for l in template.lods.iter().filter(|l| l.index != 0) {
        for (i, s) in g.sub_meshes.iter().enumerate().filter(|(_, s)| s.node == l.object) {
            rep.detail(format!("  detail level {}: sub mesh {i} has {} vertices, {} triangles", l.index, s.vertex_count, s.face_count));
        }
    }
    for name in &texture_names {
        textures_of(rep, paks, name);
    }
    Ok(())
}

/// What the archives hold for a texture name: its descriptor, the format and size of the largest mip that is there, and the
/// companions with short endings.
fn textures_of(rep: &mut Report, paks: &mut PakSet, name: &str) {
    let family = texture_family(paks, name);
    let all = paks.find(|k| k.ends_with(".pct.resource") && texture_stem(k).starts_with(name));
    rep.say(format!("  texture {name:?}: {} descriptors in its family, {} more whose names start with it (colour variants and skins)", family.len(), all.len().saturating_sub(family.len())));
    for r in family.iter().take(MAX_LISTED) {
        match texture::load_top(paks, r) {
            Err(e) => rep.say_wrapped("    ", &format!("{r}: {e:#}")),
            Ok(t) => {
                let colour = match t.decode() {
                    Ok(img) => {
                        let m = img.mean_rgba();
                        format!("average colour rgba({},{},{},{})", m[0], m[1], m[2], m[3])
                    }
                    Err(e) => format!("could not be decoded ({e:#})"),
                };
                rep.say(format!(
                    "    {:<44} {} {}x{} (mip {} of {}, {}); {colour}",
                    r.rsplit('/').next().unwrap_or(r),
                    format_name(t.desc.format),
                    t.width,
                    t.height,
                    t.level,
                    t.desc.n_mip_map,
                    human(t.data.len() as u64)
                ));
            }
        }
    }
    for r in all.iter().filter(|r| !family.contains(r)).take(MAX_LISTED) {
        rep.detail(format!("    (variant) {r}"));
    }
}
