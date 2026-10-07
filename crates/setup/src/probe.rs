//! The probe: a read-only look at the player's own Space Marine 2 install through the real readers of `ashen-sm2`.
//! It writes `probe-report.txt` (everything it found, line by line) plus a few preview pictures into the output
//! folder. It never writes anything inside the game folder.
use crate::report::{panic_text, Report};
use anyhow::{bail, Result};
use ashen_common::{steam, SM2_APP_ID, VERSION};
use ashen_sm2::bnk::{self, Bank};
use ashen_sm2::pak::{self, NestedZip, PakSet};
use ashen_sm2::texture::{self, format_name};
use ashen_sm2::wem::{self, WemInfo};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

pub struct Opts {
    pub sm2: Option<PathBuf>,
    pub out: PathBuf,
}

const WEAPON_WORDS: [&str; 4] = ["chainsword", "bolt_pistol", "bolt_rifle", "bolt_carbine"];

fn human(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

fn safe_name(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn step(rep: &mut Report, title: &str, f: impl FnOnce(&mut Report) -> Result<()>) {
    rep.section(title);
    match catch_unwind(AssertUnwindSafe(|| f(rep))) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => rep.say(format!("  THIS STEP FAILED: {e:#}")),
        Err(p) => rep.say(format!("  THIS STEP CRASHED: {}", panic_text(&*p))),
    }
}

/// Returns true when the whole probe could run (individual steps may still have reported problems).
pub fn run(opts: &Opts) -> bool {
    let mut rep = Report::create(&opts.out.join("probe-report.txt"));
    rep.say(format!("Ashen Marine - Space Marine 2 probe v{VERSION} (read-only)"));
    rep.say(format!("report folder: {}", opts.out.display()));

    let mut install: Option<(PathBuf, Option<String>)> = None;
    step(&mut rep, "1/6 Finding Space Marine 2", |rep| {
        let found = match &opts.sm2 {
            Some(p) => (p.clone(), None),
            None => match steam::locate(SM2_APP_ID) {
                Some(x) => x,
                None => bail!("Space Marine 2 was not found through Steam (app {SM2_APP_ID}). Start the probe again with  --sm2 \"<its folder>\""),
            },
        };
        rep.say(format!("  folder:   {}", found.0.display()));
        rep.say(format!("  Steam build id: {}", found.1.as_deref().unwrap_or("(unknown)")));
        for rel in ["Warhammer 40000 Space Marine 2.exe", "start_protected_game.exe", "client_pc", "server_pc", "EasyAntiCheat"] {
            rep.detail(format!("  {} {rel}", if found.0.join(rel).exists() { "present:" } else { "MISSING:" }));
        }
        install = Some(found);
        Ok(())
    });
    let Some((root, _)) = install else { return false };

    let mut pakset: Option<PakSet> = None;
    step(&mut rep, "2/6 Opening the game archives (.pak)", |rep| {
        let mut dir = None;
        for rel in ["client_pc/root/paks/client", "client_pc/root/paks"] {
            let p = root.join(rel);
            if p.is_dir() && pak::list_paks(&p).map(|v| !v.is_empty()).unwrap_or(false) {
                dir = Some(p);
                break;
            }
        }
        let Some(dir) = dir else { bail!("no .pak files found below {}\\client_pc\\root\\paks", root.display()) };
        let files = pak::list_paks(&dir)?;
        rep.say(format!("  {} archives below {}", files.len(), dir.display()));
        let mut set = PakSet::new();
        let total: u64 = files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
        rep.say(format!("  together {}", human(total)));
        for (i, f) in files.iter().enumerate() {
            if let Err(e) = set.add(f) {
                set.skipped.push((f.clone(), format!("{e:#}")));
            }
            if (i + 1) % 10 == 0 || i + 1 == files.len() {
                rep.progress(&format!("    opened {}/{} ({:.0}s)", i + 1, files.len(), rep.elapsed()));
            }
        }
        rep.say(format!("  opened {} archives, {} names in total, {} duplicate names", set.pak_count(), set.entry_count(), set.duplicate_names));
        for (p, why) in &set.skipped {
            rep.say(format!("  SKIPPED {}: {why}", p.display()));
        }
        pakset = Some(set);
        Ok(())
    });
    let Some(mut paks) = pakset else { return false };

    step(&mut rep, "3/6 Weapon files (what makes up a chainsword and a bolt pistol)", |rep| step_weapons(rep, &mut paks));
    step(&mut rep, "4/6 Weapon textures", |rep| step_textures(rep, &mut paks, &opts.out));
    step(&mut rep, "5/6 Weapon data files (.cls)", |rep| step_classes(rep, &mut paks));
    step(&mut rep, "6/6 Sounds", |rep| step_sounds(rep, &mut paks, &opts.out));

    rep.section("Done");
    rep.say(format!("  finished in {:.0}s. Send the file  probe-report.txt  (and the textures folder if you like) back.", rep.elapsed()));
    true
}

fn template_folder(name: &str) -> String {
    name.split('/').nth(1).unwrap_or("").to_string()
}

fn step_weapons(rep: &mut Report, paks: &mut PakSet) -> Result<()> {
    for kw in WEAPON_WORDS {
        let names = paks.find(|k| k.starts_with("tpl/") && k.contains(kw));
        let mut folders: BTreeMap<String, usize> = BTreeMap::new();
        for n in &names {
            *folders.entry(template_folder(n)).or_default() += 1;
        }
        rep.say(format!("  {kw}: {} template files in {} template folders", names.len(), folders.len()));
        for (f, c) in folders.iter().take(30) {
            rep.detail(format!("      {f}  ({c} files)"));
        }
    }
    for kw in ["chainsword", "bolt_pistol"] {
        let names = paks.find(|k| k.starts_with("tpl/") && k.contains(kw));
        let Some(folder) = names.iter().map(|n| template_folder(n)).min_by_key(|f| (f.len(), f.clone())) else { continue };
        rep.say(format!("  files of  tpl/{folder}:"));
        for n in names.iter().filter(|n| template_folder(n) == folder) {
            match paks.info(n) {
                Ok(i) => rep.say(format!("      {}  {}  ({})", n.rsplit('/').next().unwrap_or(n), human(i.size), i.pak.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())),
                Err(e) => rep.detail(format!("      {n}: {e:#}")),
            }
        }
    }
    Ok(())
}

fn step_textures(rep: &mut Report, paks: &mut PakSet, out: &Path) -> Result<()> {
    let dir = out.join("textures");
    std::fs::create_dir_all(&dir)?;
    let mut gallery: Vec<(String, String)> = Vec::new();
    let mut shown_raw = false;
    for kw in ["wpn_chainsword", "wpn_bolt_pistol"] {
        let mut res = paks.find(|k| k.ends_with(".pct.resource") && k.contains(kw));
        if res.is_empty() {
            res = paks.find(|k| k.ends_with(".resource") && k.contains(kw));
        }
        let mips = paks.find(|k| k.ends_with(".pct_mip") && k.contains(kw));
        rep.say(format!("  {kw}: {} texture descriptors, {} .pct_mip data files", res.len(), mips.len()));
        for n in res.iter().take(14) {
            rep.detail(format!("      {n}"));
        }
        // up to 5 different textures: one per name once a trailing _<number> is ignored
        let mut seen = HashSet::new();
        let picks: Vec<&String> = res
            .iter()
            .filter(|n| {
                let stem = n.rsplit('/').next().unwrap_or(n).split('.').next().unwrap_or("");
                let base = stem.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('_').to_string();
                seen.insert(base)
            })
            .take(5)
            .collect();
        for r in picks {
            if !shown_raw {
                shown_raw = true;
                match paks.read_text(r, 1 << 20) {
                    Ok(t) => {
                        rep.say(format!("  descriptor text of {r} (first lines):"));
                        for l in t.lines().take(60) {
                            rep.say(format!("      | {l}"));
                        }
                    }
                    Err(e) => rep.say(format!("  cannot read {r}: {e:#}")),
                }
            }
            match texture::load_top(paks, r) {
                Err(e) => rep.say(format!("  texture {r}: {e:#}")),
                Ok(t) => match t.decode() {
                    Err(e) => rep.say(format!("  texture {r}: {} {}x{} could not be decoded: {e:#}", format_name(t.desc.format), t.width, t.height)),
                    Ok(img) => {
                        let m = img.mean_rgba();
                        rep.say(format!(
                            "  texture {}: {} {}x{} (mip {} of {}), average colour rgba({},{},{},{}) from {}",
                            r.rsplit('/').next().unwrap_or(r),
                            format_name(t.desc.format),
                            t.width,
                            t.height,
                            t.level,
                            t.desc.n_mip_map,
                            m[0],
                            m[1],
                            m[2],
                            m[3],
                            t.file.rsplit('/').next().unwrap_or(&t.file)
                        ));
                        let stem = safe_name(r.rsplit('/').next().unwrap_or(r).trim_end_matches(".resource").trim_end_matches(".pct"));
                        let small = img.shrink_to(768);
                        if let Ok(png) = small.to_png() {
                            let file = format!("{stem}.png");
                            if std::fs::write(dir.join(&file), png).is_ok() {
                                gallery.push((file, format!("{r} - {} {}x{}", format_name(t.desc.format), t.width, t.height)));
                            }
                        }
                        if gallery.len() <= 2 {
                            if let Ok(dds) = texture::dds_bytes(t.desc.format, t.width, t.height, &t.data) {
                                let _ = std::fs::write(dir.join(format!("{stem}.dds")), dds);
                            }
                        }
                    }
                },
            }
        }
    }
    let mut html = String::from("<!doctype html><meta charset=utf-8><title>Ashen Marine - Space Marine 2 texture preview</title><body style='background:#1b1b1b;color:#ddd;font-family:sans-serif'><h1>Space Marine 2 weapon textures (read from your own install)</h1>");
    for (file, caption) in &gallery {
        html += &format!("<figure style='display:inline-block;margin:8px'><img src='{file}' style='max-width:384px;image-rendering:pixelated'><figcaption style='font-size:12px;max-width:384px'>{caption}</figcaption></figure>");
    }
    std::fs::write(dir.join("index.html"), html)?;
    rep.say(format!("  wrote {} preview pictures to {}", gallery.len(), dir.display()));
    Ok(())
}

fn step_classes(rep: &mut Report, paks: &mut PakSet) -> Result<()> {
    let all_cls = paks.find(|k| k.ends_with(".cls"));
    rep.say(format!("  {} .cls files in total", all_cls.len()));
    for kw in ["chainsword", "bolt_pistol"] {
        let hits: Vec<&String> = all_cls.iter().filter(|n| n.to_ascii_lowercase().contains(kw)).collect();
        rep.say(format!("  .cls files with '{kw}' in the NAME: {}", hits.len()));
        for n in hits.iter().take(12) {
            rep.detail(format!("      {n}"));
        }
        for n in hits.iter().take(2) {
            match paks.read_text(n, 4 << 20) {
                Ok(t) => {
                    rep.say(format!("  first lines of {n}:"));
                    for l in t.lines().take(90) {
                        rep.say(format!("      | {l}"));
                    }
                }
                Err(e) => rep.say(format!("  cannot read {n}: {e:#}")),
            }
        }
    }
    // every other .cls that mentions the weapons (sound ids, stats and so on live in other files)
    let mut mentions = 0usize;
    let mut shown = 0usize;
    let mut files_with = 0usize;
    for n in &all_cls {
        let Ok(text) = paks.read_text(n, 8 << 20) else { continue };
        let low = text.to_ascii_lowercase();
        if !(low.contains("chainsword") || low.contains("bolt_pistol")) {
            continue;
        }
        files_with += 1;
        for (i, l) in text.lines().enumerate() {
            let ll = l.to_ascii_lowercase();
            if ll.contains("chainsword") || ll.contains("bolt_pistol") {
                mentions += 1;
                if shown < 45 {
                    shown += 1;
                    rep.say(format!("  mention {n}:{}: {}", i + 1, l.trim()));
                }
            }
        }
    }
    rep.say(format!("  {mentions} lines in {files_with} .cls files mention a chainsword or a bolt pistol"));
    Ok(())
}

/// Identifier-like words of a text (letters, digits, underscore), 5..=80 characters.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| (5..=80).contains(&w.len()) && w.bytes().any(|b| b.is_ascii_alphabetic()))
}

fn candidate_event_names() -> Vec<String> {
    let prefixes = ["", "play_", "stop_", "sfx_", "wpn_", "play_wpn_", "stop_wpn_", "play_sfx_", "play_sfx_wpn_", "pause_", "resume_"];
    let weapons = ["chainsword", "chain_sword", "wpn_chainsword", "bolt_pistol", "boltpistol", "wpn_bolt_pistol", "bolt_rifle", "bolter", "bolt_carbine", "melee", "pistol", "chainsword_01", "bolt_pistol_01"];
    let actions = [
        "", "swing", "swing_light", "swing_heavy", "attack", "attack_light", "attack_heavy", "light", "heavy", "hit", "impact", "impact_flesh", "idle", "rev", "revving", "start", "stop", "loop", "fire", "fire_single", "shot", "shoot", "single", "burst", "auto", "reload", "reload_start", "reload_end", "equip",
        "unequip", "draw", "holster", "parry", "block", "charge", "overheat", "ads", "dry", "empty", "click", "combo", "slash", "sweep", "stab", "execution", "ammo_pickup",
    ];
    let mut v = Vec::new();
    for p in prefixes {
        for w in weapons {
            for a in actions {
                if a.is_empty() {
                    v.push(format!("{p}{w}"));
                } else {
                    v.push(format!("{p}{w}_{a}"));
                    v.push(format!("{p}{a}_{w}"));
                }
            }
        }
    }
    v
}

fn step_sounds(rep: &mut Report, paks: &mut PakSet, out: &Path) -> Result<()> {
    // --- banks
    let bnks = paks.find(|k| k.ends_with(".bnk"));
    rep.say(format!("  {} .bnk sound banks", bnks.len()));
    for n in bnks.iter().take(60) {
        if let Ok(i) = paks.info(n) {
            rep.detail(format!("      {n}  {}", human(i.size)));
        }
    }
    let mut banks: Vec<(String, Bank)> = Vec::new();
    let mut budget: u64 = 600 << 20;
    for n in &bnks {
        let Ok(i) = paks.info(n) else { continue };
        if i.size > budget || i.size > 200 << 20 {
            rep.detail(format!("  (skipping big bank {n}, {})", human(i.size)));
            continue;
        }
        budget -= i.size;
        match paks.read(n, 200 << 20).and_then(Bank::parse) {
            Ok(b) => banks.push((n.clone(), b)),
            Err(e) => rep.detail(format!("  bank {n}: {e:#}")),
        }
    }
    rep.say(format!("  parsed {} banks", banks.len()));
    let wpn = banks.iter().position(|(n, _)| n.to_ascii_lowercase().ends_with("/wpn.bnk")).or_else(|| banks.iter().position(|(n, _)| n.to_ascii_lowercase().contains("wpn")));
    if let Some(i) = wpn {
        let (n, b) = &banks[i];
        rep.say(format!("  weapon bank {n}: Wwise bank version {} (id {:#x}), {} bytes", b.version, b.bank_id, b.data.len()));
        let chunks: Vec<String> = b.chunks.iter().map(|c| format!("{}:{}", c.tag, c.range.len())).collect();
        rep.say(format!("      chunks: {}", chunks.join(" ")));
        let kinds: Vec<String> = b.kind_counts().iter().map(|(k, c)| format!("{} x{c}", bnk::kind_name(*k))).collect();
        rep.say(format!("      objects: {}", kinds.join(", ")));
        let (mut embedded, mut streamed) = (0, 0);
        for o in b.objects.iter().filter(|o| o.kind == bnk::kind::SOUND) {
            if let Some(s) = b.sound(o) {
                if s.stream_type == 0 { embedded += 1 } else { streamed += 1 }
            }
        }
        rep.say(format!("      sounds: {embedded} stored in the bank, {streamed} streamed from separate files; {} media entries in DIDX", b.media.len()));
    } else {
        rep.say("  no weapon bank (name containing 'wpn') among the parsed banks");
    }

    // --- event names: guessed names and every word found in the text data, hashed the way Wwise does
    let mut events: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, (_, b)) in banks.iter().enumerate() {
        for id in b.event_ids() {
            events.entry(id).or_default().push(i);
        }
    }
    rep.say(format!("  {} distinct event ids across the parsed banks", events.len()));
    let mut named: BTreeMap<String, u32> = BTreeMap::new();
    for name in candidate_event_names() {
        let h = bnk::fnv1_lower(&name);
        if events.contains_key(&h) {
            named.insert(name, h);
        }
    }
    rep.say(format!("  guessed weapon event names that exist: {}", named.len()));
    for (n, h) in named.iter().take(40) {
        rep.say(format!("      {n}  ({h:#010x})"));
    }
    let mut from_text = 0usize;
    let mut text_hits: BTreeMap<String, u32> = BTreeMap::new();
    let mut consider = |w: &str| {
        let h = bnk::fnv1_lower(w);
        if events.contains_key(&h) {
            text_hits.entry(w.to_ascii_lowercase()).or_insert(h);
        }
    };
    for n in paks.find(|k| k.ends_with(".cls") || k.ends_with(".asset") || k.ends_with(".txt") || k.ends_with(".sfx")).iter().take(20000) {
        if let Ok(t) = paks.read_text(n, 2 << 20) {
            for w in words(&t) {
                from_text += 1;
                consider(w);
            }
        }
    }
    for n in paks.find(|_| true) {
        for part in n.split(['/', '.']) {
            if part.len() >= 5 {
                consider(part);
            }
        }
    }
    rep.say(format!("  event names recovered from words in game text and file names: {} (from {from_text} words)", text_hits.len()));
    let weaponish: Vec<(&String, &u32)> = text_hits.iter().filter(|(n, _)| ["chainsword", "chain_sword", "bolt", "pistol", "wpn", "melee"].iter().any(|k| n.contains(k))).collect();
    for (n, h) in weaponish.iter().take(70) {
        let in_banks: Vec<&str> = events[h].iter().map(|&i| banks[i].0.rsplit('/').next().unwrap_or("")).collect();
        rep.say(format!("      {n}  ({h:#010x}) in {}", in_banks.join(",")));
    }

    // --- streamed audio files live in zips inside the sound paks
    let zips = paks.find(|k| k.ends_with(".zip"));
    rep.say(format!("  {} zip files inside the paks", zips.len()));
    let mut nested: Vec<(String, NestedZip)> = Vec::new();
    for n in &zips {
        match paks.info(n) {
            Ok(i) => rep.detail(format!("      {n}  {}  {}", human(i.size), if i.stored { "stored" } else { "COMPRESSED" })),
            Err(_) => continue,
        }
        match paks.open_nested(n) {
            Ok(z) => nested.push((n.clone(), z)),
            Err(e) => rep.detail(format!("      cannot open {n}: {e:#}")),
        }
    }
    rep.say(format!("  opened {} of them in place", nested.len()));
    let mut by_id: HashMap<u64, (usize, usize)> = HashMap::new();
    let mut ext_count: BTreeMap<String, usize> = BTreeMap::new();
    for (zi, (zn, z)) in nested.iter().enumerate() {
        let mut sample = Vec::new();
        for ei in 0..z.len() {
            let Some(name) = z.name_for_index(ei) else { continue };
            let file = name.rsplit('/').next().unwrap_or(name);
            let (stem, ext) = file.rsplit_once('.').unwrap_or((file, ""));
            *ext_count.entry(ext.to_ascii_lowercase()).or_default() += 1;
            if let Ok(id) = stem.parse::<u64>() {
                by_id.entry(id).or_insert((zi, ei));
            }
            if sample.len() < 6 {
                sample.push(name.to_string());
            }
        }
        rep.detail(format!("      {zn}: {} entries, e.g. {}", z.len(), sample.join(" | ")));
    }
    rep.say(format!("  file types inside those zips: {:?}", ext_count));
    rep.say(format!("  {} entries are named by a number (wem media ids)", by_id.len()));

    // --- the weapon bank's streamed sounds: are they there, and in which codec?
    let mut codec_hist: BTreeMap<String, usize> = BTreeMap::new();
    let mut found = 0usize;
    let mut wanted = 0usize;
    let mut detail_lines = 0usize;
    if let Some(i) = wpn {
        let b = &banks[i].1;
        let mut ids: Vec<u32> = b.objects.iter().filter(|o| o.kind == bnk::kind::SOUND).filter_map(|o| b.sound(o)).filter(|s| s.stream_type != 0).map(|s| s.media_id).collect();
        ids.sort_unstable();
        ids.dedup();
        wanted = ids.len();
        for id in ids {
            let Some(&(zi, ei)) = by_id.get(&(id as u64)) else { continue };
            found += 1;
            let mut head = vec![0u8; 8192];
            let n = {
                let mut f = match nested[zi].1.by_index(ei) {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                let mut got = 0;
                while got < head.len() {
                    match f.read(&mut head[got..]) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => got += k,
                    }
                }
                got
            };
            head.truncate(n);
            match WemInfo::parse(&head) {
                Ok(w) => {
                    *codec_hist.entry(format!("{} (0x{:04X})", wem::codec_name(w.format_tag), w.format_tag)).or_default() += 1;
                    if detail_lines < 8 {
                        detail_lines += 1;
                        let ch: Vec<String> = w.chunks.iter().map(|(t, l)| format!("{t}:{l}")).collect();
                        rep.say(format!("      wem {id}: {} ch {} Hz, ~{:.2}s, chunks {}", w.channels, w.sample_rate, w.approx_seconds(), ch.join(" ")));
                        rep.detail(format!("        fmt extra bytes: {:02x?}", &w.fmt_extra[..w.fmt_extra.len().min(48)]));
                    }
                }
                Err(e) => {
                    *codec_hist.entry(format!("unreadable header ({e})")).or_default() += 1;
                    if detail_lines < 8 {
                        detail_lines += 1;
                        rep.say(format!("      media {id}: first bytes {:02x?}", &head[..head.len().min(32)]));
                    }
                }
            }
        }
    }
    rep.say(format!("  weapon bank streamed sounds found in the zips: {found} of {wanted}"));
    for (c, n) in &codec_hist {
        rep.say(format!("      codec {c}: {n} files"));
    }

    // --- one concrete try: decode what the first named weapon event plays
    let dir = out.join("sounds");
    std::fs::create_dir_all(&dir)?;
    let mut written = 0;
    let mut done_events: HashSet<u32> = HashSet::new();
    'outer: for (name, h) in named.iter().chain(weaponish.iter().map(|(n, h)| (*n, *h))) {
        if !done_events.insert(*h) {
            continue;
        }
        for &bi in events.get(h).into_iter().flatten() {
            let b = &banks[bi].1;
            for s in b.event_sounds(*h).into_iter().take(2) {
                let bytes: Option<Vec<u8>> = if s.stream_type == 0 {
                    b.embedded_media(s.media_id).map(|x| x.to_vec())
                } else {
                    by_id.get(&(s.media_id as u64)).and_then(|&(zi, ei)| {
                        let mut v = Vec::new();
                        nested[zi].1.by_index(ei).ok()?.read_to_end(&mut v).ok()?;
                        Some(v)
                    })
                };
                let Some(bytes) = bytes else { continue };
                let file = format!("{}_{}", safe_name(name), s.media_id);
                let _ = std::fs::write(dir.join(format!("{file}.wem")), &bytes);
                match wem::decode(&bytes) {
                    Ok(pcm) => {
                        let _ = std::fs::write(dir.join(format!("{file}.wav")), wem::wav_bytes(&pcm));
                        rep.say(format!("  event {name}: media {} decoded to {file}.wav ({:.2}s)", s.media_id, pcm.seconds()));
                    }
                    Err(e) => rep.say(format!("  event {name}: media {} saved as {file}.wem ({e:#})", s.media_id)),
                }
                written += 1;
                if written >= 6 {
                    break 'outer;
                }
            }
        }
    }
    rep.say(format!("  saved {written} sample sounds to {}", dir.display()));
    Ok(())
}
