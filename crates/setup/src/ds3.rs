//! The Dark Souls III side of the setup program: `ds3-probe` and `ds3-prepare`.
//!
//! Dark Souls III keeps its files inside encrypted archives (`Game/Data0.bhd` + `Data0.bdt` and so on), so there is no
//! loose `item.msgbnd.dcx` to copy. The weapons of the mashup are built from other weapons and would show their old
//! names (Shortsword, Avelyn, Standard Bolt). ModEngine2 loads a loose file from the mod folder instead of the archived
//! one, so this reads the game's own item text out of the archives (read-only, with `ashen-ds3data`), changes the names
//! and descriptions of the test weapons in a copy in memory and, only if every check passes, writes that copy under the name
//! the game asks for: `<mod>/msg/engus/item_dlc2.msgbnd.dcx` for a fully updated game (the game's own folder for its English
//! text is `engUS`; it asks for it in lower case), and the same for `item_dlc1` and `item` if the archives hold those.
//!
//! * `probe` writes a report of what is in the install (key fingerprints, archives, where the interesting files are,
//!   a dry run of the name change). It writes nothing but the report.
//! * `prepare` does the same and writes the override and its manifest, or - when anything is wrong - writes nothing new
//!   and removes an override it wrote earlier (a stale file must not stay in the game).
//!
//! The game's folder is only ever read. Reports show key fingerprints, never keys, and name paths relative to the game
//! folder or the program's folder, so they never carry the player's user name.
mod models;
pub use models::{models, ModelsOpts, ModelsOutcome, MODELS_MANIFEST_FILE, MODELS_REMOVE_REPORT_FILE, MODELS_REPORT_FILE};
use crate::report::{panic_text, Report};
use crate::{find_ds3, human};
use anyhow::{anyhow, bail, Result};
use ashen_common::VERSION;
use ashen_ds3data::archive::{Archive, ArchiveError};
use ashen_ds3data::bhd5::Entry;
use ashen_ds3data::bnd4::{self, Bnd4};
use ashen_ds3data::dcx::{self, DcxInfo};
use ashen_ds3data::discover::{self, Bundle, Limits, Search};
use ashen_ds3data::flver::Flver;
use ashen_ds3data::fmg::{FmgError, FmgFile};
use ashen_ds3data::hash::path_hash;
use ashen_ds3data::install::{exe_path, find_game_dir, look_at_bhd, BhdLook, Ds3Install, ExeInfo, PlainHeader};
use ashen_ds3data::keys::{load_pem_file, RsaPublicKey};
use ashen_ds3data::msgpatch::{patch_item_msgbnd_detailed, ItemEdit, PatchError};
use ashen_ds3data::tpf::Tpf;
use ashen_ds3data::{sha256_hex, snippet};
use serde_json::Value;
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

/// The report both commands write.
pub const REPORT_FILE: &str = "ds3-report.txt";
/// The manifest next to the overrides.
pub const MANIFEST_FILE: &str = "ashenmarine-msg.json";
/// The archive paths of the English item text, in the order a fully updated game prefers them. The folder is `engUS`: kit 0.8
/// looked for `ENGLISH` and found nothing. Kits 0.5 to 0.9 wrote only a changed `item.msgbnd.dcx`, but the game never asks for
/// that one: its hook log shows it asking ModEngine2 for `msg\engus\item_dlc2.msgbnd.dcx`, `menu_dlc2`, `ngword` and
/// `msg\na\sellregion` (and the archives of that install have no `item.msgbnd.dcx` at all), so the names never changed.
/// `item_dlc1` and `item` are what an install with less downloadable content would ask for; every one of the three that the
/// archives hold gets the new names. (The hash of a path ignores the case.)
pub const ENGLISH_ITEM_PATHS: [&str; 3] = ["/msg/engUS/item_dlc2.msgbnd.dcx", "/msg/engUS/item_dlc1.msgbnd.dcx", "/msg/engUS/item.msgbnd.dcx"];
/// Where the changed copy of an archive path goes, relative to the mod folder (always with `/`, and in lower case, the way the
/// game asks for it): `/msg/engUS/item_dlc2.msgbnd.dcx` -> `msg/engus/item_dlc2.msgbnd.dcx`.
pub fn override_rel(archive_path: &str) -> String {
    archive_path.trim_start_matches('/').to_lowercase()
}
/// The folder of the English text, as the report calls it.
pub const ENGLISH_FOLDER: &str = "engUS";

/// The language folders of `msg` that are looked for (the PC build's names; the path table shows which exist).
pub const LANGUAGES: [&str; 16] = [
    "engUS", "engGB", "frafr", "deude", "itait", "jpnjp", "korkr", "polpl", "porbr", "rusru", "spaes", "spaar", "zhocn", "zhotw", "thath", "turtr",
];
/// A language folder of `msg` in plain words.
pub fn language_name(folder: &str) -> &str {
    match folder {
        "engUS" => "English",
        "engGB" => "English (UK)",
        "frafr" => "French",
        "deude" => "German",
        "itait" => "Italian",
        "jpnjp" => "Japanese",
        "korkr" => "Korean",
        "polpl" => "Polish",
        "porbr" => "Portuguese (Brazil)",
        "rusru" => "Russian",
        "spaes" => "Spanish",
        "spaar" => "Spanish (Latin America)",
        "zhocn" => "Chinese (simplified)",
        "zhotw" => "Chinese (traditional)",
        "thath" => "Thai",
        "turtr" => "Turkish",
        other => other,
    }
}
/// The text containers of a language folder that the path table lists: the item text (names and descriptions; the one the mod
/// changes) and the menu text, each in its three versions (base game, first and second downloadable content).
const TEXT_FILES: [&str; 6] = ["item_dlc2", "item_dlc1", "item", "menu_dlc2", "menu_dlc1", "menu"];
/// The item text containers of a language folder, in the order a fully updated game prefers them.
const ITEM_FILES: [&str; 3] = ["item_dlc2", "item_dlc1", "item"];
/// Which of the English item text paths the archives hold, in the order of [`ENGLISH_ITEM_PATHS`].
fn english_item_paths_present(install: &Ds3Install) -> Vec<&'static str> {
    ENGLISH_ITEM_PATHS.iter().copied().filter(|p| !install.lookup(p).is_empty()).collect()
}
/// Other names the English item text might have, tried when the expected ones are not in the archives.
const ALTERNATIVE_ITEM_PATHS: [&str; 9] = [
    "/msg/ENGLISH/item.msgbnd.dcx",
    "/msg/ENG/item.msgbnd.dcx",
    "/msg/EN/item.msgbnd.dcx",
    "/msg/na/item.msgbnd.dcx",
    "/msg/engUS/item.msgbnd",
    "/msg/engUS/item.msgbnd.dcx.bak",
    "/msg/item.msgbnd.dcx",
    "/msg/engUS/item_patch.msgbnd.dcx",
    "/msg/engUS/itemname.msgbnd.dcx",
];
const EXTRA_PATHS: [&str; 13] = [
    // the text files the game itself was seen asking for (hook log of kit 0.8): they show in which archive the text lives
    "/msg/engUS/ngword.msgbnd.dcx",
    "/msg/na/sellregion.msgbnd.dcx",
    "/regulation.bin",
    "/param/gameparam/gameparam.parambnd.dcx",
    "/param/gameparam/gameparam_dlc1.parambnd.dcx",
    "/chr/c0000.chrbnd.dcx",
    "/chr/c0000.anibnd.dcx",
    "/event/common.emevd.dcx",
    "/menu/hi/00_solo.tpf.dcx",
    "/facegen/facegen.fgbnd.dcx",
    "/other/graphicsconfig.xml",
    "/map/mapstudio/m30_00_00_00.msb.dcx",
    "/parts/wp_a_0100.partsbnd.dcx",
];
/// The weapon models the mod will want to swap later.
const MODEL_PATHS: [&str; 5] = [
    "/parts/wp_a_0200.partsbnd.dcx",
    "/parts/wp_a_0200_l.partsbnd.dcx",
    "/parts/wp_a_1404.partsbnd.dcx",
    "/parts/wp_a_1409.partsbnd.dcx",
    "/parts/wp_a_1419.partsbnd.dcx",
];
/// The two of them that are listed inside (groundwork for the model swap).
const MODELS_TO_LIST: [&str; 2] = ["/parts/wp_a_0200.partsbnd.dcx", "/parts/wp_a_1409.partsbnd.dcx"];
/// The ids whose texts the report shows.
const SNIPPET_IDS: [u32; 5] = [2_000_000, 14_090_000, 404_000, 14_040_000, 14_190_000];

const MAX_MSG_FILE: u64 = 64 << 20;
const MAX_MSG_DECODED: u64 = 256 << 20;
const MAX_MODEL_FILE: u64 = 256 << 20;

// ------------------------------------------------------------------------------------------------ the edits

/// One name change and the descriptions that go with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditSpec {
    pub id: u32,
    /// The name the game has now; anything else and nothing is written.
    pub expect: &'static str,
    pub new_name: &'static str,
    pub short: &'static str,
    pub long: &'static str,
}

/// The test weapons: the name each one has in Dark Souls III and the one it gets in the mashup.
pub const EDITS: [EditSpec; 3] = [
    EditSpec {
        id: 2_000_000,
        expect: "Shortsword",
        new_name: "Chainsword",
        short: "A roaring chain-toothed blade of the Adeptus Astartes.",
        long: "A heavy blade rimmed with whirring teeth.\nIt does not cut so much as chew its way through armour and bone.\nThe snarl of its motor is the last thing many foes ever hear.",
    },
    EditSpec {
        id: 14_090_000,
        expect: "Avelyn",
        new_name: "Bolt Pistol",
        short: "A sidearm that fires three bolts in quick succession.",
        long: "A compact sidearm of the Adeptus Astartes.\nOne pull of the trigger sends three explosive bolts downrange in quick succession.\nIt kicks hard, but few things are left standing after a close burst.",
    },
    EditSpec {
        id: 404_000,
        expect: "Standard Bolt",
        new_name: "Bolt Rounds",
        short: "Mass-reactive rounds that burst after striking a target.",
        long: "Small rocket-driven rounds with mass-reactive tips.\nEach one bites into its target before bursting from within.\nLoaded into bolt pistols and the heavier guns of the Astartes.",
    },
];

/// The edits as the patcher takes them.
pub fn item_edits() -> Vec<ItemEdit<'static>> {
    EDITS.iter().map(|e| ItemEdit { id: e.id, expect_name: e.expect, new_name: e.new_name, short_text: Some(e.short), long_text: Some(e.long) }).collect()
}

// ------------------------------------------------------------------------------------------------ options

pub struct ProbeOpts {
    /// The Dark Souls III folder (`--ds3`, or `game-folder.txt`); `None`: ask Steam.
    pub ds3: Option<PathBuf>,
    /// A PEM file with archive keys (`--keys`).
    pub keys: Option<PathBuf>,
    /// The folder the program lives in (`{data}`): `cache/ds3-keys.pem` is looked for here.
    pub data: PathBuf,
    /// The folder the report goes to.
    pub out: PathBuf,
}

pub struct PrepareOpts {
    pub ds3: Option<PathBuf>,
    pub keys: Option<PathBuf>,
    pub data: PathBuf,
    /// The folder the report goes to.
    pub out: PathBuf,
    /// The ModEngine2 mod folder (`{data}/mod`).
    pub mod_dir: PathBuf,
}

/// Where the report of a run goes.
pub fn report_path(out: &Path) -> PathBuf {
    out.join(REPORT_FILE)
}

/// How a run of `prepare` ended.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The override and its manifest were written.
    pub written: bool,
    /// A stale override of an earlier run was deleted.
    pub removed_stale: bool,
    /// Edits applied.
    pub edits: usize,
    /// Why nothing was written (plain words).
    pub reason: Option<String>,
}

impl Outcome {
    /// The exit code of the program is 0 for this.
    pub fn ok(&self) -> bool {
        self.written
    }
}

// ------------------------------------------------------------------------------------------------ small helpers

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string())
}

/// `p` relative to `base` when it is below it, else just its last part (never the folders above: those may hold a user name).
fn shown(p: &Path, base: &Path) -> String {
    match p.strip_prefix(base) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.display().to_string(),
        _ => name_of(p),
    }
}

fn json_text(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// The last part of a path inside a container (`N:\FDP\...\WeaponName.fmg` -> `WeaponName.fmg`).
fn leaf(name: &str) -> &str {
    name.rsplit(['\\', '/']).next().unwrap_or(name)
}

/// Not an error: the test kit has not collected what it needs yet (the very first Prepare, before the game ever ran with the
/// kit). Told as "NOT READY YET" instead of "PROBLEM".
#[derive(Debug)]
struct NotYet(String);

impl std::fmt::Display for NotYet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotYet {}

/// Runs one stage of a run. A problem (an error or a crash) is told in plain words and returned as text.
fn stage<T>(rep: &mut Report, title: &str, f: impl FnOnce(&mut Report) -> Result<T>) -> Result<T, String> {
    rep.section(title);
    match catch_unwind(AssertUnwindSafe(|| f(rep))) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => {
            let why = format!("{e:#}");
            let label = if e.downcast_ref::<NotYet>().is_some() { "NOT READY YET" } else { "PROBLEM" };
            rep.say_wrapped("  ", &format!("{label}: {why}"));
            Err(why)
        }
        Err(p) => {
            let why = format!("this part of the program crashed ({}). Please send me the report.", panic_text(&*p));
            rep.say_wrapped("  ", &format!("PROBLEM: {why}"));
            Err(why)
        }
    }
}

/// Lines that go to the console and the report (`loud`) or to the report only.
struct Out<'a> {
    rep: &'a mut Report,
    loud: bool,
}

impl Out<'_> {
    fn line(&mut self, text: impl AsRef<str>) {
        if self.loud {
            self.rep.say(text);
        } else {
            self.rep.detail(text);
        }
    }

    fn wrapped(&mut self, indent: &str, text: &str) {
        if self.loud {
            self.rep.say_wrapped(indent, text);
        } else {
            self.rep.detail_wrapped(indent, text);
        }
    }
}

// ------------------------------------------------------------------------------------------------ steps both commands share

/// Step "finding the game".
fn find_step(rep: &mut Report, given: Option<&Path>) -> Result<PathBuf> {
    let root = find_ds3(given)?;
    rep.say(format!("  Dark Souls III folder: {}", name_of(&root)));
    rep.say(format!("  Steam build id: {}", crate::ds3_build_id(&root).as_deref().unwrap_or("(unknown)")));
    Ok(root)
}

/// What the program file, the player and the test kit's in-game collector gave for reading the archives.
struct Sources {
    game_dir: PathBuf,
    exe: ExeInfo,
    /// Keys from `--keys` and `cache/ds3-keys.pem`.
    extra: Vec<RsaPublicKey>,
    /// Plain tables of contents the running game held (`cache/bhd5/*.bin`).
    headers: Vec<PlainHeader>,
}

/// Step "program file and keys": the folder with the program file, what it holds, the extra keys the player gave and what
/// the test kit collected from the running game.
fn keys_step(rep: &mut Report, root: &Path, keys_arg: Option<&Path>, data: &Path) -> Result<Sources> {
    let game_dir = find_game_dir(root).map_err(|_| {
        anyhow!("{} does not contain Game\\DarkSoulsIII.exe. Is that really the Dark Souls III folder? If the game is installed, Steam can check it: right click the game, Properties, Installed Files, Verify integrity of game files", name_of(root))
    })?;
    let exe_file = exe_path(&game_dir);
    rep.progress("  reading the program file (a few seconds)");
    let exe = ExeInfo::scan(&exe_file).map_err(|e| anyhow!("{e}"))?;
    rep.say(format!("  program file: {}\\{}, {}, SHA-256 {}", name_of(&game_dir), name_of(&exe_file), human(exe.size), exe.sha256));
    let prints = |keys: &[RsaPublicKey]| keys.iter().map(|k| k.fingerprint()).collect::<Vec<_>>().join(", ");
    if exe.keys.is_empty() {
        rep.say(format!("  archive keys found in the program file: none ({} key text blocks looked at, {} of them damaged; no key in any other shape either)", exe.pem_blocks, exe.pem_rejected));
    } else {
        rep.say(format!("  archive keys found in the program file: {} ({}){}", exe.keys.len(), prints(&exe.keys), if exe.other_forms > 0 { format!(", {} of them not as plain PEM text", exe.other_forms) } else { String::new() }));
    }
    let mut extra: Vec<RsaPublicKey> = Vec::new();
    if let Some(p) = keys_arg {
        match load_pem_file(p) {
            Ok(keys) => {
                rep.say(format!("  keys from the key file {}: {} ({})", name_of(p), keys.len(), prints(&keys)));
                extra.extend(keys);
            }
            Err(e) => rep.say_wrapped("  ", &format!("PROBLEM: the key file {} cannot be used: {e}", name_of(p))),
        }
    }
    let cache = data.join("cache").join("ds3-keys.pem");
    if cache.is_file() {
        match load_pem_file(&cache) {
            Ok(keys) => {
                rep.say(format!("  keys from cache\\ds3-keys.pem: {} ({})", keys.len(), prints(&keys)));
                extra.extend(keys);
            }
            Err(e) => rep.say_wrapped("  ", &format!("PROBLEM: cache\\ds3-keys.pem cannot be used: {e}")),
        }
    } else {
        rep.detail("  keys from cache\\ds3-keys.pem: no such file");
    }
    // everything the running game showed (kit 0.9): the archives that no real key has opened are tried with these, too
    let seen = data.join("cache").join("keys-seen.pem");
    if seen.is_file() {
        match load_pem_file(&seen) {
            Ok(keys) => {
                let before = extra.len();
                for k in keys {
                    if !extra.contains(&k) && !exe.keys.contains(&k) {
                        extra.push(k);
                    }
                }
                rep.say(format!("  keys the running game showed (cache\\keys-seen.pem): {} more", extra.len() - before));
            }
            Err(e) => rep.say_wrapped("  ", &format!("PROBLEM: cache\\keys-seen.pem cannot be used: {e}")),
        }
    }
    let headers = PlainHeader::load_dir(&data.join("cache").join("bhd5"));
    if headers.is_empty() {
        rep.say("  tables of contents saved from the running game (cache\\bhd5): none yet");
    } else {
        rep.say(format!("  tables of contents saved from the running game (cache\\bhd5): {} ({})", headers.len(), headers.iter().map(|h| h.label.as_str()).collect::<Vec<_>>().join(", ")));
    }
    Ok(Sources { game_dir, exe, extra, headers })
}

/// The plain-words explanation for "no archive could be opened".
fn no_archive_opened(install: &Ds3Install, headers: usize) -> String {
    if install.keys().is_empty() && headers == 0 {
        "the test kit has not collected what it needs to read Dark Souls III's archives yet (the program file holds no archive key). It collects it from the running game: start the game once with Play-AshenMarine.bat and quit it again; this step then runs by itself, or run Prepare-AshenMarine.bat again".to_string()
    } else if install.keys().is_empty() {
        format!("{headers} table(s) of contents were saved from the running game but none of them fits an archive of this install (was the game updated since?). Delete the folder ashenmarine\\cache\\bhd5, start the game once with Play-AshenMarine.bat and quit it again")
    } else {
        format!(
            "none of the {} archive keys that were found opens Dark Souls III's archives (key fingerprints: {}). Please send me the report so I can see why",
            install.keys().len(),
            install.keys().iter().map(|k| k.fingerprint()).collect::<Vec<_>>().join(", ")
        )
    }
}

/// Step "archives": opens every `.bhd` and lists them; fails when none can be used.
fn archives_step(rep: &mut Report, src: Sources) -> Result<Ds3Install> {
    let saved = src.headers.len();
    let install = Ds3Install::open_with_sources(src.game_dir, src.exe, &src.extra, &src.headers, &mut |m| rep.progress(&format!("  {m}"))).map_err(|e| anyhow!("{e}"))?;
    if install.archives().is_empty() {
        bail!("the Game folder has no archive files (Data0.bhd and so on). Is Dark Souls III fully installed? Steam can check it: right click the game, Properties, Installed Files, Verify integrity of game files");
    }
    for slot in install.archives() {
        let bdt = slot.bdt_size.map_or("no .bdt".to_string(), |s| format!("{} .bdt", human(s)));
        match &slot.archive {
            Ok(a) => {
                let magic = if a.bdt_magic() == *b"BDF4" { "BDF4".to_string() } else { format!("UNEXPECTED start {:02x?}", a.bdt_magic()) };
                let (agree, total) = a.bucket_agreement();
                rep.say(format!(
                    "  {:<8} {} .bhd, {}, salt {} characters, {} buckets, {} files; {bdt} ({magic})",
                    slot.name,
                    human(slot.bhd_size),
                    a.source(),
                    a.header().salt().len(),
                    a.header().bucket_count(),
                    a.entries().len()
                ));
                // a table that was read correctly is a hash table: every file sits in the bucket its hash points to
                rep.detail(format!("           check: {agree} of {total} file hashes sit in the bucket hash % {} points to", a.header().bucket_count()));
                if a.trailing_bhd_bytes() != 0 {
                    rep.say(format!("           note: {} bytes at the end of the .bhd are not a whole encrypted block (ignored)", a.trailing_bhd_bytes()));
                }
                if a.files_outside_bdt() != 0 {
                    rep.say(format!("           WARNING: {} of the files lie outside the .bdt (damaged or incomplete download); they cannot be read", a.files_outside_bdt()));
                }
            }
            Err(ArchiveError::NoKeyMatched { tried: 0 }) => rep.say(format!("  {:<8} {} .bhd, no key to try; {bdt} [{}]", slot.name, human(slot.bhd_size), slot.hint)),
            Err(ArchiveError::NoKeyMatched { tried }) => rep.say(format!("  {:<8} {} .bhd, no key matched ({tried} tried); {bdt} [{}]", slot.name, human(slot.bhd_size), slot.hint)),
            Err(e) => rep.say(format!("  {:<8} {} .bhd, cannot be opened: {e}; {bdt} [{}]", slot.name, human(slot.bhd_size), slot.hint)),
        }
    }
    // what else is in the folder (names and sizes only): tells at a glance whether this is the game and which parts it has
    if let Ok(listing) = std::fs::read_dir(install.game_dir()) {
        let mut files: Vec<(String, u64)> = listing
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((e.file_name().to_string_lossy().to_string(), if meta.is_dir() { 0 } else { meta.len() }))
            })
            .collect();
        files.sort();
        let shown: Vec<String> = files.iter().take(60).map(|(name, size)| if *size == 0 { format!("{name}/") } else { format!("{name} ({})", human(*size)) }).collect();
        rep.detail_wrapped("  ", &format!("files in the {} folder: {}{}", name_of(install.game_dir()), shown.join(", "), if files.len() > 60 { ", ..." } else { "" }));
    }
    describe_unopened(rep, &install);
    let opened = install.open_archives().count();
    if opened == 0 {
        let why = no_archive_opened(&install, saved);
        if install.keys().is_empty() && saved == 0 {
            return Err(NotYet(why).into());
        }
        bail!("{why}");
    }
    if opened < install.archives().len() {
        rep.say_wrapped("  ", &format!("NOTE: {} of {} archives could not be opened; files inside them cannot be found.", install.archives().len() - opened, install.archives().len()));
    }
    Ok(install)
}

/// Hex in lines of 32 bytes, each with its offset.
fn hex_lines(bytes: &[u8]) -> Vec<String> {
    bytes.chunks(32).enumerate().map(|(i, c)| format!("{:04x}: {}", i * 32, ashen_ds3data::hex(c))).collect()
}

/// What an archive that no key opened looks like from the outside: its first and last bytes (the whole file when it is small),
/// and which of the keys that are known turn its first block into a plain block. The rules that hold for the other archives
/// may not hold for these (`Data0.bhd` is not a whole number of encrypted blocks; the downloadable content may be laid out
/// differently), so the report carries what is needed to work out the rules.
fn describe_unopened(rep: &mut Report, install: &Ds3Install) {
    for slot in install.archives().iter().filter(|s| s.archive.is_err()) {
        let look = match look_at_bhd(&slot.bhd_path, install.keys()) {
            Ok(look) => look,
            Err(e) => {
                rep.detail(format!("  {}: its .bhd cannot be looked at ({e})", slot.name));
                continue;
            }
        };
        rep.detail(format!("  {}: {} bytes; the first {}: {}", slot.name, look.size, look.head.len(), ashen_ds3data::hex(&look.head)));
        rep.detail(format!("  {}: the last {}: {}", slot.name, look.tail.len(), ashen_ds3data::hex(&look.tail)));
        if let Some(whole) = &look.whole {
            rep.detail(format!("  {}: the whole file, 32 bytes to a line:", slot.name));
            for line in hex_lines(whole) {
                rep.detail(format!("      {line}"));
            }
        }
        if look.keys_tried == 0 {
            rep.say(format!("  {:<8} too short for a key to be tried on it", slot.name));
        } else if look.plain_by.is_empty() {
            rep.say(format!("  {:<8} {} keys were tried on its first block: none turns it into a plain block (so none of them is its key, or it is not stored in blocks)", slot.name, look.keys_tried));
        } else {
            for (fingerprint, plain) in &look.plain_by {
                let readable = if BhdLook::readable_start(plain) { format!(" = \"{}\"", String::from_utf8_lossy(&plain[..4])) } else { String::new() };
                rep.say(format!(
                    "  {:<8} key {fingerprint} turns its first block into a plain block (one wrong key in about 128 does that by chance); it starts {}{readable}",
                    slot.name,
                    ashen_ds3data::hex(&plain[..plain.len().min(16)])
                ));
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ reading files out of the archives

/// A file out of an archive that reads as DCX holding a BND4.
struct Found {
    archive: String,
    stored: u32,
    unpadded: Option<u64>,
    hit_count: usize,
    /// Why the other files with the same path hash were not used.
    rejected: Vec<String>,
    dcx_len: usize,
    dcx_info: DcxInfo,
    decoded: Vec<u8>,
    bnd: Bnd4,
}

/// One file of an archive, read as DCX holding a BND4.
fn load_found(archive: &Archive, entry: &Entry, hit_count: usize, max_read: u64) -> Result<Found, String> {
    let bytes = archive.read_limited(entry, max_read).map_err(|e| format!("cannot be read: {e}"))?;
    let (decoded, info) = dcx::decode_limited(&bytes, MAX_MSG_DECODED).map_err(|e| format!("is not usable DCX: {e}"))?;
    let bnd = Bnd4::parse(&decoded).map_err(|e| format!("is DCX but not a usable BND4: {e} [{}]", Bnd4::describe_header(&decoded)))?;
    Ok(Found { archive: archive.name().to_string(), stored: entry.padded_size, unpadded: entry.unpadded(), hit_count, rejected: Vec::new(), dcx_len: bytes.len(), dcx_info: info, decoded, bnd })
}

/// Several files can share a path hash; the first one that reads as DCX holding a BND4 is used.
fn read_valid(install: &Ds3Install, path: &str, max_read: u64) -> Result<Found, String> {
    let hits = install.lookup(path);
    if hits.is_empty() {
        return Err(format!("{path} is not in any archive"));
    }
    let mut rejected = Vec::new();
    for (i, hit) in hits.iter().enumerate() {
        let who = hit.archive.name().to_string();
        match load_found(hit.archive, hit.entry, hits.len(), max_read) {
            Ok(mut found) => {
                found.rejected = std::mem::take(&mut rejected);
                for (j, other) in hits.iter().enumerate() {
                    if j > i {
                        found.rejected.push(format!("{} also has a file with this path hash (not used, the first usable one was taken)", other.archive.name()));
                    }
                }
                return Ok(found);
            }
            Err(why) => rejected.push(format!("{who}: the file with this path hash {why}")),
        }
    }
    Err(rejected.join("; "))
}

fn dcx_line(found: &Found) -> String {
    format!(
        "found in {}: {} bytes ({}); DCX variant {}; {} bytes after decoding, SHA-256 of that {}",
        found.archive,
        found.dcx_len,
        found.unpadded.map_or(format!("entry of {} bytes with padding, size after decryption not given", found.stored), |u| format!("entry of {} bytes with padding, {u} after decryption", found.stored)),
        found.dcx_info.variant_name(),
        found.decoded.len(),
        sha256_hex(&found.decoded)
    )
}

/// The BND4 summary: format, layout, and every file with its table summary.
fn describe_container(out: &mut Out, found: &Found, show_snippets: bool) {
    let b = &found.bnd;
    out.line(format!("  container: BND4 version \"{}\", format {:?} (stored as {:#04x}), {} names, extended {}, {} files", b.version, b.format, b.format_raw, if b.unicode { "Unicode" } else { "ASCII" }, b.extended, b.files.len()));
    if let Some(t) = &b.hash_table {
        out.line(format!("  hash table: {} buckets at {:#x}, {}", t.bucket_count, t.offset, if t.consistent { "consistent" } else { "NOT CONSISTENT with the headers" }));
    }
    let gaps: Vec<String> = b.layout.gaps.iter().take(12).map(|g| g.to_string()).collect();
    out.line(format!(
        "  layout: alignment {:#x}, gaps before the files [{}{}], {} bytes after the last file: {}",
        b.layout.alignment,
        gaps.join(", "),
        if b.layout.gaps.len() > 12 { ", ..." } else { "" },
        b.layout.tail_len,
        if b.layout.verified { "as expected".to_string() } else { format!("NOT AS EXPECTED ({})", b.layout.issues.join("; ")) }
    ));
    if let Some(first) = b.files.iter().find_map(|f| f.name.as_deref()) {
        let prefix = first.strip_suffix(leaf(first)).unwrap_or("");
        if !prefix.is_empty() {
            out.line(format!("  file names look like  {prefix}<file>"));
        }
    }
    let mut tables: Vec<(usize, FmgFile)> = Vec::new();
    let mut unreadable_shown = 0;
    for f in &b.files {
        let bytes = b.file_bytes(&found.decoded, f.index).unwrap_or(&[]);
        let what = match FmgFile::parse(bytes) {
            Ok(t) => {
                let range = t.id_range().map_or("empty".to_string(), |(lo, hi)| format!("ids {lo}..{hi}"));
                let s = format!("text table, {} entries ({} with text), {range}", t.len(), t.text_count());
                tables.push((f.index, t));
                s
            }
            Err(FmgError::NotFmg) if !f.name.as_deref().is_some_and(|n| n.to_lowercase().ends_with(".fmg")) => "not a text table".to_string(),
            Err(e) => {
                // a table that should have read but did not: its structure helps to see why
                unreadable_shown += 1;
                if unreadable_shown <= 3 {
                    out.line(format!("    (table #{} did not read: {e}; {})", f.index, FmgFile::describe_header(bytes)));
                }
                format!("NOT READABLE as a text table ({e})")
            }
        };
        out.line(format!(
            "    #{:<3} id {:<6} {:<28} {:>9} B{}  {what}",
            f.index,
            f.id.map_or("-".to_string(), |i| i.to_string()),
            f.name.as_deref().map_or("(no name)", leaf),
            f.stored_size,
            if f.is_compressed() { " (compressed)" } else { "" }
        ));
    }
    if show_snippets {
        out.line("  texts at the ids the mod changes (first 40 characters):");
        for id in SNIPPET_IDS {
            let found_in: Vec<String> = tables
                .iter()
                .filter_map(|(index, t)| t.get(id).map(|text| format!("{} \"{}\"", b.files.get(*index).and_then(|f| f.name.as_deref()).map_or("?", leaf), snippet(text, 40))))
                .collect();
            out.line(format!("    id {id}: {}", if found_in.is_empty() { "no table has a text here".to_string() } else { found_in.join("; ") }));
        }
    }
}

// ------------------------------------------------------------------------------------------------ the dry run

/// One check of the dry run.
struct DryRun {
    /// Every check that had to pass did.
    ok: bool,
    /// The first thing that went wrong, in plain words.
    reason: Option<String>,
    /// The new DCX file, when the edits could be made and checked.
    new_file: Option<Vec<u8>>,
    applied: Vec<(u32, String, String)>,
}

fn explain_patch_error(e: &PatchError) -> String {
    match e {
        PatchError::NameMismatch { id, expected, found } => {
            let have: Vec<String> = found.iter().map(|(table, text)| format!("{table} says \"{text}\"")).collect();
            format!("Dark Souls III's item text no longer has \"{expected}\" at id {id} ({}). The game may have been updated", have.join("; "))
        }
        PatchError::IdAbsent { id } => {
            let expected = EDITS.iter().find(|e| e.id == *id).map_or("?", |e| e.expect);
            format!("Dark Souls III's item text has no entry at id {id}, where \"{expected}\" was expected. The game may have been updated")
        }
        PatchError::NoTables => "none of the files inside Dark Souls III's item text could be read as text tables".to_string(),
        PatchError::Bnd4(e) => format!("Dark Souls III's item text is laid out in a way this program does not trust ({e})"),
        other => format!("{other}"),
    }
}

/// Makes the edits on a copy in memory and checks everything that can be checked. Nothing is written.
fn dry_run(out: &mut Out, found: &Found, edits: &[ItemEdit]) -> DryRun {
    let mut run = DryRun { ok: true, reason: None, new_file: None, applied: Vec::new() };
    let note = |out: &mut Out, run: &mut DryRun, passed: bool, name: &str, detail: String| {
        out.line(format!("    [{}] {name}{}", if passed { "ok" } else { "FAILED" }, if detail.is_empty() { String::new() } else { format!(": {detail}") }));
        if !passed {
            run.ok = false;
            if run.reason.is_none() {
                run.reason = Some(format!("a safety check failed ({name}{})", if detail.is_empty() { String::new() } else { format!(": {detail}") }));
            }
        }
    };
    out.line("  dry run of the name change (on a copy in memory, nothing is written):");

    // the compression layer: write it again and read it back
    match dcx::encode(&found.decoded, &found.dcx_info).map_err(|e| e.to_string()).and_then(|re| dcx::decode_limited(&re, MAX_MSG_DECODED).map_err(|e| e.to_string())) {
        Ok((back, _)) if back == found.decoded => note(out, &mut run, true, "DCX written again and read back", format!("identical bytes ({} bytes)", back.len())),
        Ok(_) => note(out, &mut run, false, "DCX written again and read back", "the bytes differ".to_string()),
        Err(e) => note(out, &mut run, false, "DCX written again and read back", e),
    }
    // the container layer
    let layout = &found.bnd.layout;
    if !layout.verified {
        run.reason = Some(format!("the files inside Dark Souls III's item text are not stored the way this program expects ({}), so it does not dare to change them", layout.issues.join("; ")));
    }
    note(out, &mut run, layout.verified, "BND4 files are stored the expected way", if layout.verified { format!("alignment {:#x}, zero padding", layout.alignment) } else { layout.issues.join("; ") });
    let (mut tested, mut skipped, mut bad) = (0usize, 0usize, 0usize);
    for f in &found.bnd.files {
        if f.is_compressed() || f.stored_size == 0 {
            skipped += 1;
            continue;
        }
        let same = found.bnd.file_bytes(&found.decoded, f.index).and_then(|bytes| bnd4::replace_file(&found.decoded, &found.bnd, f.index, bytes).ok());
        if same.as_deref() == Some(&found.decoded[..]) {
            tested += 1;
        } else {
            bad += 1;
        }
    }
    note(out, &mut run, bad == 0 && tested > 0, "BND4 rewrite with each file's own bytes gives identical bytes", format!("{tested} files tested, {skipped} empty or compressed ones skipped, {bad} differed"));
    // the text table writer, informational: the real game's tables are not byte-identical to what this program writes (kit 0.9
    // saw 0 of 47), so the first differences are in the report: they say how the game's own layout differs
    let (mut same, mut total) = (0usize, 0usize);
    let mut differing: Vec<String> = Vec::new();
    for f in &found.bnd.files {
        if let Some(bytes) = found.bnd.file_bytes(&found.decoded, f.index) {
            if let Ok(t) = FmgFile::parse(bytes) {
                total += 1;
                let again = t.to_bytes();
                if again == bytes {
                    same += 1;
                } else if differing.len() < 3 {
                    let head = |b: &[u8]| ashen_ds3data::hex(&b[..b.len().min(ashen_ds3data::fmg::HEADER_LEN)]);
                    differing.push(format!(
                        "file {} {}: the game's table is {} bytes, this program's writer makes {}; first difference at {}; header of the game's table {}; of this program's {}",
                        f.index,
                        f.name.as_deref().map_or("(no name)", leaf),
                        bytes.len(),
                        again.len(),
                        first_difference(&again, bytes),
                        head(bytes),
                        head(&again)
                    ));
                }
            }
        }
    }
    out.line(format!("    [info] text tables written again by this program's writer are byte-identical to the game's: {}", if total == 0 { "no tables".to_string() } else if same == total { format!("yes ({same} of {total})") } else { format!("no ({same} of {total}); the tables are read back and compared as text instead. The name change does not use that writer: it puts the new strings at the end of a table and leaves the rest of it as the game has it") }));
    for d in &differing {
        out.rep.detail_wrapped("      ", d);
    }
    if !run.ok {
        return run;
    }

    // the edits
    let outcome = match patch_item_msgbnd_detailed(&found.decoded, edits) {
        Ok(o) => o,
        Err(e) => {
            let why = explain_patch_error(&e);
            out.line("    [FAILED] the edits could not be made:");
            out.wrapped("      ", &why);
            run.ok = false;
            run.reason = Some(why);
            return run;
        }
    };
    note(out, &mut run, true, "the edits", format!("{} edits, {} tables rewritten", edits.len(), outcome.changed_files.len()));
    for line in &outcome.log {
        out.rep.detail(format!("      {line}"));
    }
    // read the result back independently of the patcher
    let verdict = verify_patched(found, &outcome.bytes, &outcome.changed_files, edits);
    note(out, &mut run, verdict.is_ok(), "the result read back: new names present, every other file of the container byte-identical", verdict.err().unwrap_or_default());
    // wrap it up again and read that back, too
    let wrapped = dcx::encode(&outcome.bytes, &found.dcx_info).map_err(|e| e.to_string()).and_then(|file| {
        let (inner, _) = dcx::decode_limited(&file, MAX_MSG_DECODED).map_err(|e| e.to_string())?;
        if inner != outcome.bytes {
            return Err("the DCX file does not decode to the patched container".to_string());
        }
        let b = Bnd4::parse(&inner).map_err(|e| e.to_string())?;
        if !b.layout.verified {
            return Err("the patched container is not laid out as expected".to_string());
        }
        Ok(file)
    });
    match &wrapped {
        Ok(file) => note(out, &mut run, true, "the new DCX file decodes to the patched container", format!("{} bytes (the game's was {})", file.len(), found.dcx_len)),
        Err(e) => note(out, &mut run, false, "the new DCX file decodes to the patched container", e.clone()),
    }
    if run.ok {
        run.applied = edits.iter().map(|e| (e.id, e.expect_name.to_string(), e.new_name.to_string())).collect();
        run.new_file = wrapped.ok();
    }
    run
}

/// Parses the patched container again and checks it against the original, using only the public readers.
fn verify_patched(found: &Found, patched: &[u8], changed: &[usize], edits: &[ItemEdit]) -> Result<(), String> {
    let after = Bnd4::parse(patched).map_err(|e| e.to_string())?;
    if after.files.len() != found.bnd.files.len() {
        return Err("the number of files changed".to_string());
    }
    for f in &found.bnd.files {
        let (old, new) = (found.bnd.file_bytes(&found.decoded, f.index), after.file_bytes(patched, f.index));
        let Some(new_f) = after.files.get(f.index) else {
            return Err(format!("file {} is missing from the result", f.index));
        };
        if (f.id, &f.name, f.flags) != (new_f.id, &new_f.name, new_f.flags) {
            return Err(format!("file {} changed its id, name or flags", f.index));
        }
        if !changed.contains(&f.index) {
            if old != new {
                return Err(format!("file {} changed although it should not have", f.index));
            }
            continue;
        }
        let before = old.and_then(|b| FmgFile::parse(b).ok()).ok_or_else(|| format!("file {} was not a text table", f.index))?;
        let now = new.and_then(|b| FmgFile::parse(b).ok()).ok_or_else(|| format!("file {} no longer reads as a text table", f.index))?;
        if before.len() != now.len() {
            return Err(format!("file {} has another number of entries", f.index));
        }
        for (id, text) in before.iter() {
            let edited = edits.iter().any(|e| e.id == id);
            if !edited && (!now.contains(id) || now.get(id) != text) {
                return Err(format!("file {} changed the text at id {id}, which is not one of the edits", f.index));
            }
        }
    }
    for e in edits {
        let in_name_table = found.bnd.files.iter().filter(|f| changed.contains(&f.index)).any(|f| {
            let before = found.bnd.file_bytes(&found.decoded, f.index).and_then(|b| FmgFile::parse(b).ok());
            let now = after.file_bytes(patched, f.index).and_then(|b| FmgFile::parse(b).ok());
            matches!((before, now), (Some(b), Some(n)) if b.get(e.id) == Some(e.expect_name) && n.get(e.id) == Some(e.new_name))
        });
        if !in_name_table {
            return Err(format!("the new name for id {} is not in the result", e.id));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------ the override on disk

/// Is `rel` (from the manifest) a plain relative path that stays below the mod folder?
fn safe_relative(rel: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = rel.split(['/', '\\']).collect();
    let ok = !rel.is_empty() && !rel.starts_with(['/', '\\']) && parts.iter().all(|p| !p.is_empty() && *p != "." && *p != ".." && !p.contains(':'));
    ok.then_some(parts)
}

/// The files an earlier run wrote, as its manifest lists them (`written` is one path in the manifest of kits 0.5 to 0.9 and a
/// list of paths since kit 0.10). `None`: the manifest cannot be read or lists something that is not below the mod folder.
fn manifest_files(text: &str) -> Option<Vec<String>> {
    let v = serde_json::from_str::<Value>(text).ok()?;
    let listed: Vec<String> = match &v["written"] {
        Value::String(one) => vec![one.clone()],
        Value::Array(many) => many.iter().map(|m| m.as_str().map(str::to_string)).collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    (!listed.is_empty() && listed.iter().all(|rel| safe_relative(rel).is_some())).then_some(listed)
}

/// Deletes the overrides an earlier run wrote (the manifest says which files they are) and the manifest. Returns whether they
/// are gone (or there were none). Files that are not listed in a readable manifest are never touched.
fn remove_stale_override(rep: &mut Report, mod_dir: &Path) -> bool {
    let manifest = mod_dir.join(MANIFEST_FILE);
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        // no manifest: the files an earlier kit may have written are named, not deleted
        let any = ENGLISH_ITEM_PATHS.iter().any(|p| rel_path(mod_dir, &override_rel(p)).exists());
        if any {
            rep.say_wrapped("  ", "NOTE: the mod folder has an item text file but no readable manifest for it, so it is not known to come from this program and it was left alone.");
        }
        return true;
    };
    let Some(listed) = manifest_files(&text) else {
        rep.say_wrapped("  ", "NOTE: the manifest of an earlier run cannot be read, so the files it describes were left alone.");
        return true;
    };
    let mut all_gone = true;
    for rel in &listed {
        let path = rel_path(mod_dir, rel);
        match std::fs::remove_file(&path) {
            Ok(()) => {
                rep.say_wrapped("  ", &format!("Removed the item text file of an earlier run ({}), so the game does not keep a possibly out-of-date copy.", shown(&path, mod_dir)));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                rep.say_wrapped("  ", &format!("WARNING: the item text file of an earlier run ({}) could not be deleted ({e}). Is Dark Souls III running? Close it and run this again, or delete that file by hand: the game may keep showing the changed names.", shown(&path, mod_dir)));
                all_gone = false;
            }
        }
    }
    if all_gone {
        let _ = std::fs::remove_file(&manifest);
    }
    all_gone
}

/// `rel` (with `/`) below the mod folder.
fn rel_path(mod_dir: &Path, rel: &str) -> PathBuf {
    rel.split(['/', '\\']).fold(mod_dir.to_path_buf(), |p, part| p.join(part))
}

/// Writes through a temporary file in the same folder (flushed to disk) and a rename.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_file_name(format!("{}.tmp", name_of(path)));
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// What one changed copy was made from.
struct Source<'a> {
    /// The archive's table of contents (`Data1.bhd`).
    archive: &'a str,
    /// The path in the archives (`/msg/engUS/item_dlc2.msgbnd.dcx`).
    path: &'a str,
    sha256: &'a str,
}

/// `ashenmarine-msg.json` (format 2), with the keys in the documented order: one entry of `source` for each file in `written`.
fn manifest_json(sources: &[Source], applied: &[(u32, String, String)]) -> String {
    let edits: Vec<String> = applied.iter().map(|(id, old, new)| format!("{{\"id\":{id},\"old\":{},\"new\":{}}}", json_text(old), json_text(new))).collect();
    let sources_json: Vec<String> = sources.iter().map(|s| format!("{{\"archive\":{},\"path\":{},\"sha256_of_decoded_original\":{}}}", json_text(s.archive), json_text(s.path), json_text(s.sha256))).collect();
    let written: Vec<String> = sources.iter().map(|s| json_text(&override_rel(s.path))).collect();
    format!(
        "{{\"format\":2,\"tool\":{},\"source\":[{}],\"edits\":[{}],\"written\":[{}]}}",
        json_text(&format!("ashenmarine-setup {VERSION}")),
        sources_json.join(","),
        edits.join(","),
        written.join(",")
    )
}

// ------------------------------------------------------------------------------------------------ finding text by content

/// The names a text bundle might have in the archives (language folder times file name), to put a name to a hash.
fn known_bundle_names() -> Vec<String> {
    let mut names = Vec::new();
    for lang in LANGUAGES {
        for file in ["item", "menu", "item_dlc1", "item_dlc2", "menu_dlc1", "menu_dlc2"] {
            names.push(format!("/msg/{lang}/{file}.msgbnd.dcx"));
        }
    }
    names.extend(ALTERNATIVE_ITEM_PATHS.iter().map(|p| p.to_string()));
    names
}

/// One line about a text bundle found by its content.
fn bundle_line(b: &Bundle, names: &[String]) -> String {
    let called = names.iter().find(|n| path_hash(n) == b.hash).map_or(String::new(), |n| format!("  = {n}"));
    let inside: Vec<&str> = b.names.iter().map(|n| leaf(n)).take(6).collect();
    format!(
        "{:<8} hash {:08x}  {} B stored{}  {}  {} files ({} text tables): {}{}{called}",
        b.archive,
        b.hash,
        b.stored,
        b.decoded_size.map_or(String::new(), |d| format!(", {d} B decoded")),
        b.dcx_variant.as_deref().unwrap_or("no DCX"),
        b.file_count,
        b.tables,
        inside.join(", "),
        if b.names.len() > 6 { ", ..." } else { "" }
    )
}

/// Looks at the start of every file of every opened archive for containers of text tables, and writes what it saw.
fn bundle_search(rep: &mut Report, install: &Ds3Install) -> Search {
    let archives: Vec<&Archive> = install.open_archives().collect();
    rep.say_wrapped("  ", "Looking at the start of every file in the archives (a few KB each, nothing is copied) for containers of text tables - this finds the item text even if its name is not what this program expects. It takes from a few seconds to a minute or two.");
    let search = discover::find_text_bundles(&archives, &Limits::default(), &mut |m| rep.progress(&format!("  {m}")));
    for st in &search.archives {
        rep.say(format!(
            "  {:<8} {} files: {} looked at ({} too small or too big, {} unreadable); DCX {} (+{} of another kind, {} damaged); containers {}; containers of text tables {}",
            st.name, st.files, st.looked_at, st.skipped_by_size, st.unreadable, st.dcx, st.other_dcx, st.damaged_dcx, st.bnd4, st.bundles
        ));
        let mut kinds: Vec<(&String, &usize)> = st.kinds.iter().collect();
        kinds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let shown: Vec<String> = kinds.iter().take(14).map(|(k, n)| format!("{k} {n}")).collect();
        rep.detail_wrapped("           ", &format!("what the files start with: {}{}", shown.join(", "), if kinds.len() > 14 { ", ..." } else { "" }));
    }
    if let Some(why) = &search.stopped {
        rep.say_wrapped("  ", &format!("NOTE: the search stopped early: {why}. What follows is what was seen up to then."));
    }
    rep.say(format!("  {} containers of text tables found in {:.1} s", search.bundles.len(), search.elapsed.as_secs_f32()));
    let names = known_bundle_names();
    for (i, b) in search.bundles.iter().enumerate() {
        let line = format!("    {}", bundle_line(b, &names));
        if i < 12 {
            rep.say(line);
        } else {
            rep.detail(line);
        }
    }
    if search.bundles.len() > 12 {
        rep.say(format!("    ... and {} more (all of them are in the report)", search.bundles.len() - 12));
    }
    search
}

/// Of the text bundles found by content, the one that is Dark Souls III's English item text: it has "Shortsword" at id
/// 2000000 and the other texts the edits expect (the same check that protects the real run), whatever its name.
fn find_item_text_by_content(rep: &mut Report, install: &Ds3Install, search: &Search) -> Option<Found> {
    let edits = item_edits();
    let known: Vec<u32> = ENGLISH_ITEM_PATHS.iter().map(|p| path_hash(p)).collect();
    // the ones with a name the game asks for first, then the biggest (the item text is one of the bigger bundles)
    let mut order: Vec<&Bundle> = search.bundles.iter().collect();
    order.sort_by_key(|b| (!known.contains(&b.hash), std::cmp::Reverse(b.stored)));
    let mut tried = 0;
    for b in order.into_iter().take(80) {
        let Some(archive) = install.open_archives().find(|a| a.name() == b.archive) else { continue };
        let Some(entry) = archive.entries().get(b.entry_index) else { continue };
        tried += 1;
        let Ok(found) = load_found(archive, entry, 1, MAX_MSG_FILE) else { continue };
        if patch_item_msgbnd_detailed(&found.decoded, &edits).is_ok() {
            rep.say_wrapped(
                "  ",
                &format!(
                    "The English item text is the container in {} with the path hash {:08x}{}.",
                    b.archive,
                    b.hash,
                    match ENGLISH_ITEM_PATHS.iter().find(|p| path_hash(p) == b.hash) {
                        Some(name) => format!(" (the hash of {})", name.trim_start_matches('/')),
                        None => ", which is NOT the hash of any name the game asks for (item_dlc2, item_dlc1 or item); it was recognised by its text (\"Shortsword\", \"Avelyn\" and \"Standard Bolt\" at the ids the mod changes)".to_string(),
                    }
                ),
            );
            return Some(found);
        }
    }
    rep.say_wrapped("  ", &format!("None of the {tried} containers of text tables that were tried has the English item text (\"Shortsword\" at id 2000000 and so on)."));
    None
}

// ------------------------------------------------------------------------------------------------ ds3-probe

fn step(rep: &mut Report, title: &str, f: impl FnOnce(&mut Report) -> Result<()>) {
    let _ = stage(rep, title, f);
}

/// The path table: which of the interesting files are in which archive.
fn path_table(rep: &mut Report, install: &Ds3Install) {
    let mut paths: Vec<String> = Vec::new();
    for lang in LANGUAGES {
        for file in TEXT_FILES {
            paths.push(format!("/msg/{lang}/{file}.msgbnd.dcx"));
        }
    }
    paths.extend(EXTRA_PATHS.iter().map(|p| p.to_string()));
    paths.extend(MODEL_PATHS.iter().map(|p| p.to_string()));
    let mut found = 0;
    for path in &paths {
        let hits = install.lookup(path);
        if hits.is_empty() {
            rep.detail(format!("  {path:<42} hash {:08x}  not found", path_hash(path)));
            continue;
        }
        found += 1;
        let sizes: Vec<String> = hits
            .iter()
            .map(|h| format!("{}: {} B stored, {}", h.archive.name(), h.entry.padded_size, h.entry.unpadded().map_or("unpadded size not given".to_string(), |u| format!("{u} B unpadded"))))
            .collect();
        rep.say(format!("  {path:<42} hash {:08x}  {}", path_hash(path), sizes.join("; also ")));
    }
    rep.say(format!("  {found} of {} paths exist in the archives (the others are listed in the report as not found)", paths.len()));
    if found == 0 {
        rep.say_wrapped("  ", "None of the paths exist. The folder names or the hash may differ from what this program assumes; the report has the hash of every path it tried.");
    }
    if english_item_paths_present(install).is_empty() {
        // the language folder may be called something else: try a few other spellings
        rep.say("  The English item text was not found under the expected names. Other spellings:");
        for alt in ALTERNATIVE_ITEM_PATHS {
            let hits = install.lookup(alt);
            let state = if hits.is_empty() { "not found".to_string() } else { format!("FOUND in {}", hits.iter().map(|h| h.archive_name().to_string()).collect::<Vec<_>>().join(", ")) };
            rep.say(format!("    {alt:<40} hash {:08x}  {state}", path_hash(alt)));
        }
    }
}

/// Step "item text": every English item text container that exists (in its three versions) and the first one of every other
/// language, examined and dry-run.
fn item_text_probe(rep: &mut Report, install: &Ds3Install) {
    let mut any = false;
    let mut loud_done = false;
    for lang in LANGUAGES {
        let mut language_done = false;
        for file in ITEM_FILES {
            let path = format!("/msg/{lang}/{file}.msgbnd.dcx");
            if install.lookup(&path).is_empty() {
                continue;
            }
            // the other languages are only looked at (a dry run of each takes a moment): the first container is enough
            if lang != ENGLISH_FOLDER && std::mem::replace(&mut language_done, true) {
                continue;
            }
            any = true;
            // the console shows the first English container (the one a fully updated game asks for); the report has them all
            let loud = lang == ENGLISH_FOLDER && !loud_done;
            loud_done |= loud;
            let english = lang == ENGLISH_FOLDER;
            let mut out = Out { rep: &mut *rep, loud };
            out.line(format!("  {path}:"));
            match read_valid(install, &path, MAX_MSG_FILE) {
                Err(why) => {
                    out.wrapped("    ", &format!("PROBLEM: {why}"));
                }
                Ok(found) => {
                    out.line(format!("  {}", dcx_line(&found)));
                    for r in &found.rejected {
                        out.wrapped("    ", r);
                    }
                    describe_container(&mut out, &found, true);
                    if english {
                        dry_run(&mut out, &found, &item_edits());
                    } else {
                        out.line("  (the name change is made for English only; this language is just looked at)");
                        // still run it quietly: the report then shows what the other languages hold at these ids
                        let mut quiet = Out { rep: &mut *out.rep, loud: false };
                        dry_run(&mut quiet, &found, &item_edits());
                    }
                }
            }
        }
    }
    if !any {
        rep.say_wrapped("  ", "No item text (item_dlc2, item_dlc1 or item .msgbnd.dcx) was found for any language, so there is nothing to change. See the path table above.");
    }
}

/// Where two byte strings first differ, for a report.
fn first_difference(a: &[u8], b: &[u8]) -> String {
    match a.iter().zip(b.iter()).position(|(x, y)| x != y) {
        Some(i) => {
            let lo = i.saturating_sub(8);
            let show = |s: &[u8]| ashen_ds3data::hex(s.get(lo..(i + 8).min(s.len())).unwrap_or(&[]));
            format!("offset {i:#x}: ours {} / the game's {}", show(a), show(b))
        }
        None => format!("the shorter one ends at {:#x}", a.len().min(b.len())),
    }
}

/// The model and texture files inside a weapon's container: the structure of the FLVER (bones, materials, meshes, how the
/// vertices are stored, with raw samples) and of the TPF, and whether writing them again gives the very same bytes - the check
/// that the writer can be trusted with this game's files. The details go to the report only; a summary line is shown.
fn describe_model_files(rep: &mut Report, found: &Found) {
    for f in &found.bnd.files {
        let Some(name) = f.name.as_deref() else { continue };
        let lower = name.to_lowercase();
        let Some(bytes) = found.bnd.file_bytes(&found.decoded, f.index) else { continue };
        if lower.ends_with(".flver") {
            let mut summary = format!("    {}: {} bytes", leaf(name), bytes.len());
            match Flver::parse(bytes) {
                Err(e) => summary.push_str(&format!("; NOT READABLE as a model: {e}")),
                Ok(model) => {
                    rep.detail(format!("    {} ({} bytes):", leaf(name), bytes.len()));
                    for line in model.describe() {
                        rep.detail(format!("      {line}"));
                    }
                    for i in 0..model.meshes.len() {
                        for line in model.vertex_stats(i) {
                            rep.detail(format!("    {line}"));
                        }
                    }
                    let verdict = match model.write() {
                        Ok(again) if again == bytes => "written again it is byte-identical to the game's file".to_string(),
                        Ok(again) => format!("written again it DIFFERS ({} bytes instead of {}; first difference at {})", again.len(), bytes.len(), first_difference(&again, bytes)),
                        Err(e) => format!("cannot be written again ({e})"),
                    };
                    summary.push_str(&format!("; model with {} meshes, {} materials, {} bones, {} dummies; {verdict}", model.meshes.len(), model.materials.len(), model.nodes.len(), model.dummies.len()));
                }
            }
            rep.say(summary);
        } else if lower.ends_with(".tpf") {
            let mut summary = format!("    {}: {} bytes", leaf(name), bytes.len());
            match Tpf::parse(bytes) {
                Err(e) => summary.push_str(&format!("; NOT READABLE as a texture container: {e}")),
                Ok(tpf) => {
                    for line in tpf.describe() {
                        rep.detail(format!("      {line}"));
                    }
                    let verdict = if tpf.write() == bytes { "written again it is byte-identical".to_string() } else { format!("written again it DIFFERS ({} bytes instead of {}; first difference at {})", tpf.write().len(), bytes.len(), first_difference(&tpf.write(), bytes)) };
                    summary.push_str(&format!("; {} textures [{}]; {verdict}", tpf.textures.len(), tpf.textures.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")));
                }
            }
            rep.say(summary);
        }
    }
}

/// Step "weapon models": DCX variant, the BND4 listing, and the structure of the model and texture files of two of them.
fn models_probe(rep: &mut Report, install: &Ds3Install) {
    for path in MODELS_TO_LIST {
        let mut out = Out { rep: &mut *rep, loud: true };
        out.line(format!("  {path}:"));
        match read_valid(install, path, MAX_MODEL_FILE) {
            Err(why) => out.wrapped("    ", &format!("not usable: {why}")),
            Ok(found) => {
                out.line(format!("  {}", dcx_line(&found)));
                describe_container(&mut out, &found, false);
                describe_model_files(rep, &found);
            }
        }
    }
}

/// `ds3-probe`: a read-only look at the player's Dark Souls III. Returns true when the install could be examined.
pub fn probe(opts: &ProbeOpts) -> bool {
    if let Ok(root) = find_ds3(opts.ds3.as_deref()) {
        if let Ok(game) = find_game_dir(&root) {
            if crate::prepare::is_inside(&opts.out, &game) || crate::prepare::is_inside(&opts.out, &root) {
                println!("Ashen Marine - Dark Souls III probe (version {VERSION})");
                println!("  The report folder is inside your Dark Souls III folder. This program never writes anything into the game folder, so nothing was done.");
                println!("  Please start it again with  --out \"<a folder somewhere else>\"  or move this program out of the game folder.");
                return false;
            }
        }
    }
    let report = report_path(&opts.out);
    let mut rep = Report::create(&report);
    rep.say(format!("Ashen Marine - Dark Souls III probe (version {VERSION})"));
    rep.say("  This only READS your Dark Souls III files. It never changes, moves or uploads anything of the game.");
    rep.say_wrapped("  ", &format!("A report of what it found is written to:  {}", shown(&report, opts.out.parent().unwrap_or(&opts.out))));

    let Ok(root) = stage(&mut rep, "1/7 Finding Dark Souls III", |rep| find_step(rep, opts.ds3.as_deref())) else { return finish_probe(rep, false) };
    let Ok(sources) = stage(&mut rep, "2/7 The program file and the archive keys", |rep| keys_step(rep, &root, opts.keys.as_deref(), &opts.data)) else { return finish_probe(rep, false) };
    let Ok(install) = stage(&mut rep, "3/7 The game archives", |rep| archives_step(rep, sources)) else { return finish_probe(rep, false) };
    step(&mut rep, "4/7 Where the files the mod needs are", |rep| {
        path_table(rep, &install);
        Ok(())
    });
    step(&mut rep, "5/7 The text files, found by what they contain", |rep| {
        let search = bundle_search(rep, &install);
        if english_item_paths_present(&install).is_empty() {
            let _ = find_item_text_by_content(rep, &install, &search);
        }
        Ok(())
    });
    step(&mut rep, "6/7 The item text (names and descriptions of weapons)", |rep| {
        item_text_probe(rep, &install);
        Ok(())
    });
    step(&mut rep, "7/7 Weapon models (groundwork for swapping them later)", |rep| {
        models_probe(rep, &install);
        Ok(())
    });
    finish_probe(rep, true)
}

fn finish_probe(mut rep: Report, ok: bool) -> bool {
    rep.section("Done");
    if ok {
        rep.say(format!("  Finished in {:.0} s. Nothing of the game was changed. Please send me the file  {REPORT_FILE}  if I ask for it.", rep.elapsed()));
    } else {
        rep.say_wrapped("  ", &format!("The probe could not look at the game; the message above says what is wrong. Please send me the file  {REPORT_FILE}  if you cannot fix it yourself."));
    }
    ok
}

// ------------------------------------------------------------------------------------------------ ds3-export-models

/// The report of `ds3-export-models`.
pub const EXPORT_REPORT_FILE: &str = "ds3-export-report.txt";
/// Together the copies are far smaller than this (a weapon container is a few hundred KB to a few MB).
const MAX_EXPORT_TOTAL: u64 = 96 << 20;

pub struct ExportOpts {
    pub ds3: Option<PathBuf>,
    pub keys: Option<PathBuf>,
    pub data: PathBuf,
    /// The folder the copies and the report go to.
    pub out: PathBuf,
}

pub fn export_report_path(out: &Path) -> PathBuf {
    out.join(EXPORT_REPORT_FILE)
}

/// Writes the weapon containers as the game stores them (still DCX) below `out`, one file per container. Returns how many.
fn copy_weapon_containers(rep: &mut Report, install: &Ds3Install, out: &Path) -> usize {
    let mut total = 0u64;
    let mut copied = 0;
    for path in MODEL_PATHS {
        let name = leaf(path);
        let hits = install.lookup(path);
        if hits.is_empty() {
            rep.say(format!("  {name}: not in any archive"));
            continue;
        }
        // several files can share a path hash: the first one that is DCX holding a BND4 is the container
        let mut chosen = None;
        let mut why_not = Vec::new();
        for hit in &hits {
            match hit.archive.read_limited(hit.entry, MAX_MODEL_FILE) {
                Err(e) => why_not.push(format!("{}: cannot be read ({e})", hit.archive.name())),
                Ok(bytes) => match dcx::decode_limited(&bytes, MAX_MSG_DECODED).map_err(|e| e.to_string()).and_then(|(inner, info)| Bnd4::parse(&inner).map(|b| (inner, info, b)).map_err(|e| e.to_string())) {
                    Ok((inner, info, bnd)) => {
                        chosen = Some((hit.archive.name().to_string(), bytes, inner, info, bnd));
                        break;
                    }
                    Err(e) => why_not.push(format!("{}: not a usable container ({e})", hit.archive.name())),
                },
            }
        }
        let Some((archive, bytes, inner, info, bnd)) = chosen else {
            rep.say_wrapped("  ", &format!("{name}: not copied - {}", why_not.join("; ")));
            continue;
        };
        if total + bytes.len() as u64 > MAX_EXPORT_TOTAL {
            rep.say(format!("  {name}: not copied - the copies would be bigger than {}", human(MAX_EXPORT_TOTAL)));
            continue;
        }
        if let Err(e) = std::fs::create_dir_all(out).and_then(|_| std::fs::write(out.join(name), &bytes)) {
            rep.say_wrapped("  ", &format!("{name}: not copied - cannot write it ({e})"));
            continue;
        }
        total += bytes.len() as u64;
        copied += 1;
        let inside: Vec<String> = bnd.files.iter().map(|f| format!("{} ({} B)", f.name.as_deref().map_or("?", leaf), bnd.file_bytes(&inner, f.index).map_or(0, |b| b.len()))).collect();
        rep.say(format!("  {name:<34} {:>10}  from {archive}, {}, sha256 {}", human(bytes.len() as u64), info.variant_name(), &sha256_hex(&bytes)[..16]));
        rep.detail(format!("      inside: {}", inside.join(", ")));
    }
    copied
}

/// A table of contents (`.bhd`) bigger than this is not copied (the real ones that no key opens are 2 KB to 250 KB).
const MAX_HEADER_COPY: u64 = 1 << 20;

/// Copies the small `.bhd` of every archive that could not be opened, and the public keys the collector saw, into
/// `out/headers`: what is needed to work out how those archives are laid out. A `.bhd` is only the index of an archive
/// (hashes, sizes, offsets), never its content. Returns how many files were copied.
fn copy_unopened_headers(rep: &mut Report, install: &Ds3Install, data: &Path, out: &Path) -> usize {
    let dir = out.join("headers");
    let mut copied = 0;
    let mut put = |rep: &mut Report, from: &Path, name: String| {
        match std::fs::read(from) {
            Err(e) => rep.say(format!("  {name}: not copied - cannot be read ({e})")),
            Ok(bytes) => match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(&name), &bytes)) {
                Err(e) => rep.say(format!("  {name}: not copied - cannot write it ({e})")),
                Ok(()) => {
                    copied += 1;
                    rep.say(format!("  {name:<34} {:>10}", human(bytes.len() as u64)));
                }
            },
        }
    };
    let unopened: Vec<&ashen_ds3data::install::ArchiveSlot> = install.archives().iter().filter(|s| s.archive.is_err()).collect();
    if unopened.is_empty() {
        rep.say("  every archive could be opened, so there is nothing to copy");
        return 0;
    }
    for slot in unopened {
        if slot.bhd_size > MAX_HEADER_COPY {
            rep.say(format!("  {}.bhd: not copied - it is {}", slot.name, human(slot.bhd_size)));
            continue;
        }
        put(rep, &slot.bhd_path, format!("{}.bhd", slot.name));
    }
    for name in ["ds3-keys.pem", "keys-seen.pem"] {
        let p = data.join("cache").join(name);
        if p.is_file() {
            put(rep, &p, name.to_string());
        }
    }
    copied
}

/// `ds3-export-models`: OPTIONAL. Copies the five weapon containers the model swap will change into a folder so the player
/// can choose to send them. Returns true when at least one was copied. The game is only read; nothing is uploaded.
pub fn export_models(opts: &ExportOpts) -> bool {
    if let Ok(root) = find_ds3(opts.ds3.as_deref()) {
        if let Ok(game) = find_game_dir(&root) {
            if crate::prepare::is_inside(&opts.out, &game) || crate::prepare::is_inside(&opts.out, &root) {
                println!("Ashen Marine - copy the Dark Souls III weapon model files (version {VERSION})");
                println!("  The output folder is inside your Dark Souls III folder. This program never writes anything into the game folder, so nothing was done.");
                println!("  Please start it again with  --out \"<a folder somewhere else>\"  or move this program out of the game folder.");
                return false;
            }
        }
    }
    let report = export_report_path(&opts.out);
    let mut rep = Report::create(&report);
    rep.say(format!("Ashen Marine - copy the Dark Souls III weapon model files (version {VERSION})"));
    rep.say_wrapped("  ", "This READS five weapon files of your Dark Souls III (the Shortsword and the Avelyn, and the left-hand and related versions the mod may change) and writes plain copies of them into a folder next to this program. It also copies the small table-of-contents files (.bhd, a few KB to a few hundred KB, only the index of an archive) of the archives this program could not open, and the public keys it saw in the running game. Nothing is uploaded and nothing in the game is changed. The copies are only for you to send me, if you want to, so that I can build the model converter and work out how those archives are laid out.");
    rep.say_wrapped("  ", &format!("The copies and a list of them go to:  {}", shown(&opts.out, opts.out.parent().unwrap_or(&opts.out))));
    let Ok(root) = stage(&mut rep, "1/5 Finding Dark Souls III", |rep| find_step(rep, opts.ds3.as_deref())) else { return finish_export(rep, 0) };
    let Ok(sources) = stage(&mut rep, "2/5 The program file and the archive keys", |rep| keys_step(rep, &root, opts.keys.as_deref(), &opts.data)) else { return finish_export(rep, 0) };
    let Ok(install) = stage(&mut rep, "3/5 The game archives", |rep| archives_step(rep, sources)) else { return finish_export(rep, 0) };
    let copied = stage(&mut rep, "4/5 Copying the weapon files", |rep| Ok(copy_weapon_containers(rep, &install, &opts.out))).unwrap_or(0);
    // the small tables of contents that no key opened, and the public keys seen: they show how those archives are laid out
    let headers = stage(&mut rep, "5/5 Copying the tables of contents of the archives that could not be opened (small index files, no content)", |rep| Ok(copy_unopened_headers(rep, &install, &opts.data, &opts.out))).unwrap_or(0);
    finish_export(rep, copied + headers)
}

fn finish_export(mut rep: Report, copied: usize) -> bool {
    rep.section("Done");
    if copied > 0 {
        rep.say_wrapped("  ", &format!("Copied {copied} files in {:.0} s. They are in the folder named above. Nothing of the game was changed.", rep.elapsed()));
    } else {
        rep.say_wrapped("  ", "Nothing was copied; the message above says why. Nothing of the game was changed.");
    }
    copied > 0
}

// ------------------------------------------------------------------------------------------------ ds3-prepare

/// Everything went wrong somewhere: say so, remove a stale override, and leave.
fn give_up(mut rep: Report, mut outcome: Outcome, mod_dir: &Path, reason: String, report: &Path, out_dir: &Path) -> Outcome {
    rep.section("Done");
    rep.say("  Nothing was changed: the item names in the game stay as Dark Souls III has them.");
    rep.say_wrapped("  ", &format!("Why: {reason}."));
    let before = mod_dir.join(MANIFEST_FILE).exists();
    let gone = remove_stale_override(&mut rep, mod_dir);
    outcome.removed_stale = before && gone && !mod_dir.join(MANIFEST_FILE).exists();
    rep.say_wrapped("  ", &format!("If you cannot fix it yourself, please send me the report:  {}", shown(report, out_dir.parent().unwrap_or(out_dir))));
    outcome.reason = Some(reason);
    outcome
}

/// `ds3-prepare`: reads the item text, and only if every check passes writes the changed copies below `<mod>/msg/engus` (the
/// file names the game asks for, see [`ENGLISH_ITEM_PATHS`]) and `<mod>/ashenmarine-msg.json`. On any problem nothing new is
/// written and the overrides of an earlier run are removed.
pub fn prepare(opts: &PrepareOpts) -> Outcome {
    let mut outcome = Outcome::default();
    // nothing may ever be written inside the game's folder
    if let Ok(root) = find_ds3(opts.ds3.as_deref()) {
        let game = find_game_dir(&root).ok();
        for (what, dir) in [("report", &opts.out), ("mod", &opts.mod_dir)] {
            if crate::prepare::is_inside(dir, &root) || game.as_ref().is_some_and(|g| crate::prepare::is_inside(dir, g)) {
                println!("Ashen Marine - preparing your Dark Souls III item names (version {VERSION})");
                println!("  The {what} folder ({}) is inside your Dark Souls III folder. This program never writes anything into the game folder, so nothing was done.", name_of(dir));
                println!("  Please start it again with the other folder chosen by you (--out / --mod), or move this program out of the game folder.");
                outcome.reason = Some("a folder to write to is inside the game folder".to_string());
                return outcome;
            }
        }
    }
    let report = report_path(&opts.out);
    let mut rep = Report::create(&report);
    rep.say(format!("Ashen Marine - preparing your Dark Souls III item names (version {VERSION})"));
    rep.say("  This only READS your Dark Souls III files. It never changes, moves or uploads anything of the game.");
    rep.say_wrapped("  ", "It reads the game's item text, changes the names of the test weapons (Chainsword, Bolt Pistol, Bolt Rounds) in a copy, and puts that copy into the mod folder, where ModEngine2 picks it up instead of the game's own file.");
    rep.say_wrapped("  ", &format!("A report of what it did is written to:  {}", shown(&report, opts.out.parent().unwrap_or(&opts.out))));
    rep.say("  This usually takes a few seconds (a slow disk can take a minute). Please keep this window open.");

    macro_rules! try_stage {
        ($e:expr) => {
            match $e {
                Ok(v) => v,
                Err(why) => return give_up(rep, outcome, &opts.mod_dir, why, &report, &opts.out),
            }
        };
    }
    let root = try_stage!(stage(&mut rep, "1/5 Finding Dark Souls III", |rep| find_step(rep, opts.ds3.as_deref())));
    let sources = try_stage!(stage(&mut rep, "2/5 The program file and the archive keys", |rep| keys_step(rep, &root, opts.keys.as_deref(), &opts.data)));
    let install = try_stage!(stage(&mut rep, "3/5 The game archives", |rep| archives_step(rep, sources)));
    let plan = match stage(&mut rep, "4/5 Reading the item text and checking the name change", |rep| plan_step(rep, &install)) {
        Ok(plan) => plan,
        Err(why) => {
            // one run should tell everything: where the files the mod needs are, in the same report
            step(&mut rep, "Where the files are (for the diagnosis)", |rep| {
                path_table(rep, &install);
                Ok(())
            });
            step(&mut rep, "The item text, if it is found under any language", |rep| {
                item_text_probe(rep, &install);
                Ok(())
            });
            return give_up(rep, outcome, &opts.mod_dir, why, &report, &opts.out);
        }
    };

    // the last stage: write. Everything above passed.
    let written = stage(&mut rep, "5/5 Writing the item text for the mod", |rep| write_step(rep, &opts.mod_dir, &plan));
    if let Err(why) = written {
        // do not leave a half-done state behind
        let _ = std::fs::remove_file(opts.mod_dir.join(MANIFEST_FILE));
        for target in &plan.targets {
            let _ = std::fs::remove_file(rel_path(&opts.mod_dir, &override_rel(target.archive_path)));
        }
        return give_up(rep, outcome, &opts.mod_dir, why, &report, &opts.out);
    }
    outcome.written = true;
    outcome.edits = plan.applied.len();
    rep.section("Done");
    rep.say_wrapped("  ", &format!("Finished in {:.0} s. The test weapons are now called {} in the item text for the mod.", rep.elapsed(), EDITS.iter().map(|e| e.new_name).collect::<Vec<_>>().join(", ")));
    let files: Vec<String> = plan.targets.iter().map(|t| override_rel(t.archive_path).replace('/', "\\")).collect();
    rep.say_wrapped("  ", &format!("Written:  {}  and  {}  (in the mod folder). Dark Souls III itself was not changed. You can close this window.", files.join(",  "), MANIFEST_FILE));
    outcome
}

/// What stage 4 decided to write: one changed copy for each English item text container the archives hold.
struct Plan {
    targets: Vec<Target>,
    /// The same for every target.
    applied: Vec<(u32, String, String)>,
}

struct Target {
    /// The path in the archives (`/msg/engUS/item_dlc2.msgbnd.dcx`); [`override_rel`] gives the file name in the mod folder.
    archive_path: &'static str,
    /// The table of contents it came from (`Data1.bhd`).
    archive_file: String,
    sha256_of_original: String,
    new_file: Vec<u8>,
}

/// Stage 4: finds the English item text, and only returns a plan if every dry run passed.
fn plan_step(rep: &mut Report, install: &Ds3Install) -> Result<Plan> {
    let present = english_item_paths_present(install);
    if present.is_empty() {
        let wanted: Vec<String> = ENGLISH_ITEM_PATHS.iter().map(|p| format!("{} ({:08x})", p.trim_start_matches('/'), path_hash(p))).collect();
        rep.say_wrapped("  ", &format!("The English item text is not in the archives under any of the names the game asks for: no archive has a file whose path hashes to one of {}.", wanted.join(", ")));
        let search = bundle_search(rep, install);
        if find_item_text_by_content(rep, install, &search).is_some() {
            bail!("the English item text is in the archives, but under a path hash that is none of the names the game asks for (item_dlc2.msgbnd.dcx, item_dlc1.msgbnd.dcx, item.msgbnd.dcx), so this program cannot tell under which file name the game wants the changed copy. Please send me the report");
        }
        let others: Vec<&str> = LANGUAGES.iter().skip(1).copied().filter(|l| ITEM_FILES.iter().any(|f| !install.lookup(&format!("/msg/{l}/{f}.msgbnd.dcx")).is_empty())).collect();
        if !others.is_empty() {
            let list: Vec<&str> = others.iter().map(|l| language_name(l)).collect();
            bail!("the item text was found only for {} but not for English, and this program only changes the English text. Is the game set to another language? Run ds3-probe and send me its report", list.join(", "));
        }
        let unopened = install.archives().iter().filter(|s| s.archive.is_err()).map(|s| s.name.clone()).collect::<Vec<_>>();
        if unopened.is_empty() {
            bail!("the item text of the game was not found in any Dark Souls III archive, neither under its name nor by looking at what the files contain. Please send me the report");
        }
        bail!(
            "the item text of the game was not found in the archives that could be opened, neither under its name nor by looking at what the files contain. It is probably in {}, which could not be opened (the lines about the archives above say why). Please send me the report",
            unopened.join(" or ")
        );
    }
    let mut targets: Vec<Target> = Vec::new();
    let mut applied = Vec::new();
    for (i, path) in present.iter().copied().enumerate() {
        let name = path.trim_start_matches('/');
        let found = read_valid(install, path, MAX_MSG_FILE).map_err(|why| anyhow!("the English item text ({name}) could not be used: {why}"))?;
        // the console shows the first one (the one a fully updated game asks for); the report has all of them
        let mut out = Out { rep: &mut *rep, loud: i == 0 };
        out.line(format!("  {name}:"));
        out.line(format!("  {}", dcx_line(&found)));
        if found.hit_count > 1 {
            out.line(format!("  {} archives or entries have this path hash; the first one that reads as a text container was used", found.hit_count));
        }
        for r in &found.rejected {
            out.wrapped("    ", r);
        }
        describe_container(&mut out, &found, true);
        let run = dry_run(&mut out, &found, &item_edits());
        let Some(new_file) = run.new_file.filter(|_| run.ok) else {
            bail!("{name}: {}", run.reason.unwrap_or_else(|| "a safety check failed".to_string()));
        };
        if i > 0 {
            rep.say(format!("  {name}: the same checks passed ({} bytes after the name change)", new_file.len()));
        }
        if applied.is_empty() {
            applied = run.applied;
        }
        targets.push(Target { archive_path: path, archive_file: format!("{}.bhd", found.archive), sha256_of_original: sha256_hex(&found.decoded), new_file });
    }
    if targets.len() > 1 {
        rep.say_wrapped("  ", &format!("{} English item text containers will be changed ({}): a fully updated game asks for the first one; the others are for an install with less downloadable content.", targets.len(), targets.iter().map(|t| t.archive_path.trim_start_matches('/')).collect::<Vec<_>>().join(", ")));
    }
    Ok(Plan { targets, applied })
}

/// Stage 5: files of an earlier run that this plan does not write are removed; then the manifest (so that a later failure
/// still knows what to remove), then the overrides; every file through a temporary file and a rename, and read back.
fn write_step(rep: &mut Report, mod_dir: &Path, plan: &Plan) -> Result<()> {
    let manifest_path = mod_dir.join(MANIFEST_FILE);
    let sources: Vec<Source> = plan.targets.iter().map(|t| Source { archive: &t.archive_file, path: t.archive_path, sha256: &t.sha256_of_original }).collect();
    let manifest = manifest_json(&sources, &plan.applied);
    // files an earlier run wrote that this one does not: kit 0.9 wrote item.msgbnd.dcx, which the game never asks for
    let new_rels: Vec<String> = plan.targets.iter().map(|t| override_rel(t.archive_path)).collect();
    if let Some(old) = std::fs::read_to_string(&manifest_path).ok().and_then(|text| manifest_files(&text)) {
        for rel in old.iter().filter(|r| !new_rels.contains(&r.replace('\\', "/"))) {
            let path = rel_path(mod_dir, rel);
            if std::fs::remove_file(&path).is_ok() {
                rep.say_wrapped("  ", &format!("Removed {}, which an earlier run wrote and this one does not.", shown(&path, mod_dir)));
            }
        }
    }
    write_atomic(&manifest_path, manifest.as_bytes()).map_err(|e| anyhow!("cannot write {} ({e}). Is the folder read-only? You can start this program with  --mod \"<another folder>\"", MANIFEST_FILE))?;
    for (target, rel) in plan.targets.iter().zip(&new_rels) {
        let override_path = rel_path(mod_dir, rel);
        write_atomic(&override_path, &target.new_file).map_err(|e| anyhow!("cannot write {} ({e}). Is Dark Souls III running? Close it and run this again", rel.replace('/', "\\")))?;
        let back = std::fs::read(&override_path).map_err(|e| anyhow!("the file just written cannot be read back ({e})"))?;
        if back != target.new_file {
            bail!("the file just written ({}) does not read back the same", rel.replace('/', "\\"));
        }
        rep.say(format!("  wrote {} ({} bytes)", rel.replace('/', "\\"), target.new_file.len()));
    }
    rep.say(format!("  wrote {} ({} bytes)", MANIFEST_FILE, manifest.len()));
    Ok(())
}

// ------------------------------------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edits_are_what_the_design_says() {
        let table: Vec<(u32, &str, &str)> = EDITS.iter().map(|e| (e.id, e.expect, e.new_name)).collect();
        assert_eq!(table, vec![(2_000_000, "Shortsword", "Chainsword"), (14_090_000, "Avelyn", "Bolt Pistol"), (404_000, "Standard Bolt", "Bolt Rounds")]);
        assert_eq!(EDITS[0].short, "A roaring chain-toothed blade of the Adeptus Astartes.");
        for e in EDITS {
            assert!(e.short.chars().count() < 160 && !e.short.contains('\n'), "{e:?}: a short text is one short line");
            assert_eq!(e.long.lines().count(), 3, "{e:?}: the long text has three lines");
            assert!(!e.long.contains('\r') && !e.long.ends_with('\n'));
            assert!(e.long.chars().count() > 100);
        }
        let edits = item_edits();
        assert_eq!(edits.len(), 3);
        assert_eq!((edits[1].id, edits[1].expect_name, edits[1].new_name, edits[1].short_text, edits[1].long_text), (14_090_000, "Avelyn", "Bolt Pistol", Some(EDITS[1].short), Some(EDITS[1].long)));
        assert!(LANGUAGES.len() == 16 && LANGUAGES[0] == ENGLISH_FOLDER && ENGLISH_FOLDER == "engUS");
        assert_eq!(ENGLISH_ITEM_PATHS, ["/msg/engUS/item_dlc2.msgbnd.dcx", "/msg/engUS/item_dlc1.msgbnd.dcx", "/msg/engUS/item.msgbnd.dcx"]);
    }

    #[test]
    fn the_overrides_are_named_the_way_the_game_asks_for_them() {
        // seen in a real game's hook log (kit 0.9): ModEngine2 was asked about msg\engus\item_dlc2.msgbnd.dcx, and the
        // archives of that install hold a file with the path hash d450d103
        assert_eq!(override_rel("/msg/engUS/item_dlc2.msgbnd.dcx"), "msg/engus/item_dlc2.msgbnd.dcx");
        assert_eq!(override_rel("/msg/engUS/item.msgbnd.dcx"), "msg/engus/item.msgbnd.dcx");
        assert_eq!(path_hash("/msg/engUS/item_dlc2.msgbnd.dcx"), 0xd450_d103, "the English item text of the real game");
        assert_eq!(path_hash("/msg/engUS/item_dlc1.msgbnd.dcx"), 0x6ede_5fc6);
        assert_eq!(path_hash("/msg/engUS/item.msgbnd.dcx"), 0x624f_014f, "the real archives have nothing under this hash: the game never asks for it");
        assert_eq!(path_hash("/msg/engus/item_dlc2.msgbnd.dcx"), path_hash(ENGLISH_ITEM_PATHS[0]), "the hash ignores the case: the game asks for engus, the archive path says engUS");
        assert_ne!(path_hash("/msg/ENGLISH/item.msgbnd.dcx"), path_hash(ENGLISH_ITEM_PATHS[2]), "kit 0.8's guess was a different path");
        for path in ENGLISH_ITEM_PATHS {
            let rel = override_rel(path);
            assert!(safe_relative(&rel).is_some() && rel == rel.to_lowercase() && !rel.starts_with('/'), "{rel}");
        }
    }

    #[test]
    fn the_manifest_has_the_documented_shape() {
        let applied = vec![(2_000_000u32, "Shortsword".to_string(), "Chainsword".to_string()), (404_000, "Standard \"Bolt\"".to_string(), "Bolt Rounds".to_string())];
        let sha = "ab".repeat(32);
        let sources = [Source { archive: "Data1.bhd", path: "/msg/engUS/item_dlc2.msgbnd.dcx", sha256: &sha }, Source { archive: "Data0.bhd", path: "/msg/engUS/item.msgbnd.dcx", sha256: &sha }];
        let text = manifest_json(&sources, &applied);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["format"], 2);
        assert_eq!(v["tool"], format!("ashenmarine-setup {VERSION}"));
        assert_eq!(v["source"][0]["archive"], "Data1.bhd");
        assert_eq!(v["source"][0]["path"], "/msg/engUS/item_dlc2.msgbnd.dcx");
        assert_eq!(v["source"][1]["archive"], "Data0.bhd");
        assert_eq!(v["source"][0]["sha256_of_decoded_original"].as_str().unwrap().len(), 64);
        assert_eq!(v["edits"][0], serde_json::json!({"id": 2000000, "old": "Shortsword", "new": "Chainsword"}));
        assert_eq!(v["edits"][1]["old"], "Standard \"Bolt\"");
        assert_eq!(v["written"], serde_json::json!(["msg/engus/item_dlc2.msgbnd.dcx", "msg/engus/item.msgbnd.dcx"]));
        assert!(text.starts_with("{\"format\":2,\"tool\":\"ashenmarine-setup "), "keys in the documented order: {text}");
        assert!(text.find("\"source\"").unwrap() < text.find("\"edits\"").unwrap() && text.find("\"edits\"").unwrap() < text.find("\"written\"").unwrap());
        assert_eq!(manifest_files(&text), Some(vec!["msg/engus/item_dlc2.msgbnd.dcx".to_string(), "msg/engus/item.msgbnd.dcx".to_string()]));
    }

    #[test]
    fn the_files_of_a_manifest_are_read_in_both_formats_and_only_when_they_stay_below_the_mod_folder() {
        // kits 0.5 to 0.9 wrote one path as text, kit 0.10 writes a list
        assert_eq!(manifest_files("{\"written\":\"msg/engus/item.msgbnd.dcx\"}"), Some(vec!["msg/engus/item.msgbnd.dcx".to_string()]));
        assert_eq!(manifest_files("{\"written\":[\"a/b.dcx\",\"a/c.dcx\"]}"), Some(vec!["a/b.dcx".to_string(), "a/c.dcx".to_string()]));
        for bad in ["{\"written\":[]}", "{\"written\":[\"a/b\",\"../c\"]}", "{\"written\":[\"a/b\",7]}", "{\"written\":7}", "{\"written\":\"../x\"}", "{}", "not json", ""] {
            assert_eq!(manifest_files(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn manifest_paths_must_stay_below_the_mod_folder() {
        assert_eq!(safe_relative("msg/engus/item.msgbnd.dcx"), Some(vec!["msg", "engus", "item.msgbnd.dcx"]));
        assert_eq!(safe_relative("msg\\engUS\\item.msgbnd.dcx"), Some(vec!["msg", "engUS", "item.msgbnd.dcx"]));
        for bad in ["", "/etc/passwd", "\\windows\\x", "../x", "a/../../x", "a//b", "./a", "C:\\x", "a/b:c", "a/", ".."] {
            assert_eq!(safe_relative(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn paths_in_reports_do_not_show_the_folders_above() {
        let base = Path::new("/home/someone/kit");
        assert_eq!(shown(Path::new("/home/someone/kit/ds3-prepare/ds3-report.txt"), base), Path::new("ds3-prepare").join("ds3-report.txt").display().to_string());
        assert_eq!(shown(Path::new("/home/someone/elsewhere/ds3-report.txt"), base), "ds3-report.txt");
        assert_eq!(shown(base, base), "kit");
        assert_eq!(name_of(Path::new("/home/someone/Steam/steamapps/common/DARK SOULS III")), "DARK SOULS III");
        assert_eq!(leaf("N:\\FDP\\data\\INTERROOT_win64\\msg\\ENGLISH\\WeaponName.fmg"), "WeaponName.fmg");
        assert_eq!(leaf("plain.txt"), "plain.txt");
    }

    #[test]
    fn atomic_writes_leave_no_temporary_file_and_replace_what_was_there() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a").join("b").join("file.bin");
        write_atomic(&p, b"first").unwrap();
        write_atomic(&p, b"second, longer").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"second, longer");
        let names: Vec<String> = std::fs::read_dir(p.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["file.bin"]);
        // a folder in the way: an error, and no temporary file left behind
        let q = t.path().join("dir");
        std::fs::create_dir_all(q.join("sub")).unwrap();
        assert!(write_atomic(&q, b"x").is_err());
        assert!(!t.path().join("dir.tmp").exists());
    }

    #[test]
    fn a_stale_override_is_removed_only_when_the_manifest_lists_it() {
        let t = tempfile::tempdir().unwrap();
        let mod_dir = t.path().join("mod");
        let override_path = mod_dir.join("msg").join("engus").join("item_dlc2.msgbnd.dcx");
        let other_path = mod_dir.join("msg").join("engus").join("item.msgbnd.dcx");
        let mut rep = Report::create(&t.path().join("r.txt"));
        let manifest = |files: &[&str]| {
            let sha = "00".repeat(32);
            let sources: Vec<Source> = files.iter().map(|p| Source { archive: "Data1.bhd", path: p, sha256: &sha }).collect();
            manifest_json(&sources, &[])
        };
        // nothing there: fine
        assert!(remove_stale_override(&mut rep, &mod_dir));
        // a file without a manifest is left alone
        std::fs::create_dir_all(override_path.parent().unwrap()).unwrap();
        std::fs::write(&override_path, b"somebody else's file").unwrap();
        assert!(remove_stale_override(&mut rep, &mod_dir));
        assert!(override_path.exists());
        // a manifest that lists it: both go
        std::fs::write(mod_dir.join(MANIFEST_FILE), manifest(&["/msg/engUS/item_dlc2.msgbnd.dcx"])).unwrap();
        assert!(remove_stale_override(&mut rep, &mod_dir));
        assert!(!override_path.exists() && !mod_dir.join(MANIFEST_FILE).exists());
        // a manifest that lists two files: both go, and a third one that it does not list stays
        let third = mod_dir.join("msg").join("engus").join("item_dlc1.msgbnd.dcx");
        for p in [&override_path, &other_path, &third] {
            std::fs::write(p, b"x").unwrap();
        }
        std::fs::write(mod_dir.join(MANIFEST_FILE), manifest(&["/msg/engUS/item_dlc2.msgbnd.dcx", "/msg/engUS/item.msgbnd.dcx"])).unwrap();
        assert!(remove_stale_override(&mut rep, &mod_dir));
        assert!(!override_path.exists() && !other_path.exists() && third.exists() && !mod_dir.join(MANIFEST_FILE).exists());
        // the manifest of kits 0.5 to 0.9 (one path as text) is understood
        std::fs::write(&other_path, b"x").unwrap();
        std::fs::write(mod_dir.join(MANIFEST_FILE), "{\"format\":1,\"written\":\"msg/engus/item.msgbnd.dcx\"}").unwrap();
        assert!(remove_stale_override(&mut rep, &mod_dir));
        assert!(!other_path.exists() && !mod_dir.join(MANIFEST_FILE).exists());
        // a manifest that lists something outside the mod folder, or garbage: nothing is deleted
        let outside = t.path().join("precious.txt");
        std::fs::write(&outside, b"keep").unwrap();
        for text in ["{\"written\":\"../precious.txt\"}", "{\"written\":[\"../precious.txt\"]}", "{\"written\":42}", "not json", "{}"] {
            std::fs::write(mod_dir.join(MANIFEST_FILE), text).unwrap();
            assert!(remove_stale_override(&mut rep, &mod_dir));
            assert!(outside.exists() && mod_dir.join(MANIFEST_FILE).exists(), "{text}");
        }
        // a manifest that lists a file that is already gone is fine
        std::fs::write(mod_dir.join(MANIFEST_FILE), manifest(&["/msg/engUS/item_dlc1.msgbnd.dcx", "/msg/engUS/item.msgbnd.dcx"])).unwrap();
        std::fs::remove_file(&third).unwrap();
        assert!(remove_stale_override(&mut rep, &mod_dir));
        assert!(!mod_dir.join(MANIFEST_FILE).exists());
    }
}
