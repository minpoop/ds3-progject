//! The prepare step: turns the player's own Space Marine 2 sound events into plain `.wav` files for the mod.
//!
//! What the mod wants comes from `design/sheets/sounds.json` (built into the program): every row names a Space Marine 2
//! event. For each row this finds the event in `sounds/desktop/wpn.bnk` and plays it a few times in its head
//! (a random container picks one sound by its weight, a switch container follows its default switch, a layer container
//! plays all of them, and every sound keeps the volume, pitch and delay the sound designers gave it - this needs the
//! bank to be read exactly, see `hirc`; when it cannot be, the older approximate walk `Bank::resolve_take` is used; the
//! random numbers come from a generator seeded by the slot, so the same install always gives the same files). The sound files of such
//! a play are read from the game's `wpn.zip` (or from the bank), decoded, mixed and written as
//! `<out>/sounds/<slot>_<n>.wav`. `index.json` lists what was made and `ready.json`, written last, says the job is done.
//! Game files are only ever read.
use crate::report::{panic_text, wrap, Report};
use crate::{find_sm2, human, open_paks, words};
use anyhow::{anyhow, bail, Context, Result};
use ashen_common::VERSION;
use ashen_sm2::bnk::{self, Bank, SoundInfo};
use ashen_sm2::hirc;
use ashen_sm2::mix::{mix_layers, MAX_SECONDS};
use ashen_sm2::pak::{NestedZip, PakSet};
use ashen_sm2::render::apply_voice;
use ashen_sm2::wem::{self, Pcm};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

pub struct Opts {
    pub sm2: Option<PathBuf>,
    /// the assets folder: the sounds go into `sounds` below it and `ready.json` into it
    pub out: PathBuf,
    /// bring all sounds up together so the loudest one is near full scale (always on in the program; tests switch it off
    /// to see the exact samples)
    pub normalize: bool,
    /// when the bank can be read exactly, make two sets: `sounds` in the way kit 4 did it (every sound at full volume, the
    /// set the owner already judged good) and `sounds-exact` with the volumes, delays and weights of the bank, so the two can
    /// be compared in the game (key F9). Off: one set, made the best way the bank allows.
    pub ab: bool,
}

/// How a run ended.
#[derive(Debug, Default)]
pub struct Outcome {
    /// rows of the slot table
    pub slots: usize,
    /// slots that got at least one sound
    pub prepared: usize,
    /// `.wav` files written
    pub files: usize,
    /// the same for the second set (`sounds-exact`), when there is one
    pub alt_prepared: usize,
    pub alt_files: usize,
    /// `(slot, why)` of every slot that got no sound
    pub failed: Vec<(String, String)>,
    /// `ready.json` was written: the job finished with something to show
    pub ready: bool,
}

impl Outcome {
    /// The exit code of the program is 0 for this.
    pub fn ok(&self) -> bool {
        self.ready
    }
}

pub use ashen_common::soundset::ALT_SET;

/// The weapon sound bank, and the zip next to it that holds the sound files it streams.
const BANK: &str = "sounds/desktop/wpn.bnk";
const ZIP: &str = "sounds/desktop/wpn.zip";
/// When a take turns out to use the same sound files as an earlier one, up to this many other random draws are tried.
const RETRIES: u32 = 20;
/// The most takes a row of the sheet may ask for.
const MAX_TAKES: usize = 16;
/// A weapon sound is a handful of layers. An event that plays more than this at once was most likely read wrongly, and
/// mixing it would only make noise.
const MAX_LAYERS: usize = 32;
const MAX_BANK: u64 = 200 << 20;
const MAX_WEM: u64 = 64 << 20;
/// Limits for the help that is only worked out when an event is missing: the game's own event lists, other banks.
const MAX_LISTS: usize = 8;
const MAX_LIST: u64 = 32 << 20;
const MAX_OTHER_BANK: u64 = 128 << 20;
const OTHER_BANKS_BUDGET: u64 = 512 << 20;

// ------------------------------------------------------------------------------------------------ the slot table

/// The slot table: `design/sheets/sounds.json`, the source of truth for which sounds the mod plays.
const SOUNDS_SHEET: &str = include_str!("../../../design/sheets/sounds.json");

/// One row of the sheet: a sound the mod can play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// the row id; the files are called `<id>_<n>.wav`
    pub id: String,
    /// the Space Marine 2 (Wwise) event whose sounds are rendered for it
    pub event: String,
    /// how many different renderings to make
    pub takes: usize,
    /// played in a loop until stopped
    pub looped: bool,
}

/// The rows of the built-in slot table.
pub fn slots() -> Result<Vec<Slot>> {
    parse_slots(SOUNDS_SHEET)
}

fn parse_slots(json: &str) -> Result<Vec<Slot>> {
    let sheet: Value = serde_json::from_str(json).context("the list of sounds built into this program is damaged")?;
    let rows = sheet["rows"].as_array().ok_or_else(|| anyhow!("the list of sounds built into this program has no rows"))?;
    let mut out: Vec<Slot> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let text = |key: &str| row[key].as_str().filter(|s| !s.trim().is_empty()).map(str::to_string).ok_or_else(|| anyhow!("row {} of the list of sounds has no \"{key}\"", i + 1));
        let id = text("id")?;
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            bail!("the sound id {id:?} cannot be used in a file name");
        }
        if out.iter().any(|s| s.id == id) {
            bail!("the sound id {id:?} appears twice in the list of sounds");
        }
        let takes = row["takes"].as_u64().filter(|t| (1..=MAX_TAKES as u64).contains(t)).ok_or_else(|| anyhow!("sound {id}: \"takes\" must be a whole number from 1 to {MAX_TAKES}"))?;
        let looped = row["looped"].as_bool().ok_or_else(|| anyhow!("sound {id}: \"looped\" must be true or false"))?;
        out.push(Slot { id, event: text("sm2_event")?, takes: takes as usize, looped });
    }
    if out.is_empty() {
        bail!("the list of sounds built into this program is empty");
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------ random numbers

/// splitmix64: tiny and well mixed, and it gives the same numbers for the same seed on every machine.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// An index below `n` (`n` > 0). The slight bias of the remainder does not matter for picking a sound.
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// The seed of a take: the slot name's hash, the take number and the retry number (another draw for the same take) side
/// by side, so no two takes of any slots share one (xor-ing the take into the hash would: names that differ in their
/// last digit differ by a few bits only).
fn take_seed(slot: &str, take: usize, attempt: u32) -> u64 {
    (bnk::fnv1_lower(slot) as u64) << 16 | (take as u64 & 0xFF) << 8 | (attempt as u64 & 0xFF)
}

// ------------------------------------------------------------------------------------------------ the result files

/// One line of `index.json`.
struct IndexEntry {
    slot: String,
    event: String,
    looped: bool,
    /// file names inside the sounds folder
    files: Vec<String>,
    seconds: Vec<f64>,
}

fn json_text(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

fn json_number(x: f64) -> String {
    serde_json::to_string(&x).unwrap_or_else(|_| "0.0".to_string())
}

impl IndexEntry {
    fn json(&self) -> String {
        let files: Vec<String> = self.files.iter().map(|f| json_text(f)).collect();
        let seconds: Vec<String> = self.seconds.iter().map(|s| json_number(*s)).collect();
        format!(
            "{{\"slot\":{},\"event\":{},\"loop\":{},\"files\":[{}],\"seconds\":[{}]}}",
            json_text(&self.slot),
            json_text(&self.event),
            self.looped,
            files.join(","),
            seconds.join(",")
        )
    }
}

/// `sounds/index.json`: what was made. Written by hand so that the keys come in the documented order.
fn index_json(source: &str, entries: &[IndexEntry]) -> String {
    let sounds: Vec<String> = entries.iter().map(IndexEntry::json).collect();
    format!("{{\"format\":1,\"source\":{},\"sounds\":[{}]}}", json_text(source), sounds.join(","))
}

/// `ready.json`: the marker that the whole job finished.
fn ready_json(slots: usize, files: usize, alt: bool) -> String {
    let alt = if alt { format!(",\"alt_sets\":[{}]", json_text(ALT_SET)) } else { String::new() };
    format!("{{\"format\":1,\"tool\":{},\"slots\":{slots},\"files\":{files}{alt}}}", json_text(&format!("ashenmarine-setup {VERSION}")))
}

/// Write through a temporary file and a rename, so a reader never sees half a file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("cannot move {} to {}", tmp.display(), path.display()))
}

/// Is `name` one of the files a run writes for `slot`: `<slot>_<number>.wav`? A slot whose name goes on with more words
/// (chainsword_idle and chainsword_idle_start) has files of its own that must not count.
fn is_take_file(slot: &str, name: &str) -> bool {
    name.strip_prefix(slot).and_then(|r| r.strip_prefix('_')).and_then(|r| r.strip_suffix(".wav")).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Remove what an earlier run left: `ready.json` first (so nothing looks finished from here on), the index, and the
/// `.wav` files of every slot of the table. Files that belong to nothing of ours stay.
fn clear_old_output(out: &Path, sounds: &Path, slots: &[Slot], rep: &mut Report) {
    let mut gone = 0;
    let mut remove = |path: &Path, rep: &mut Report| match std::fs::remove_file(path) {
        Ok(()) => gone += 1,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => rep.say_wrapped("  ", &format!("WARNING: cannot delete the old file {} ({e}). Is the game running? It will be written over if possible.", path.display())),
    };
    remove(&out.join("ready.json"), rep);
    remove(&sounds.join("index.json"), rep);
    if let Ok(dir) = std::fs::read_dir(sounds) {
        let mut stale: Vec<PathBuf> = dir.filter_map(|e| e.ok()).filter(|e| e.file_name().to_str().is_some_and(|n| slots.iter().any(|s| is_take_file(&s.id, n)))).map(|e| e.path()).collect();
        stale.sort();
        for path in stale {
            remove(&path, rep);
        }
    }
    if gone > 0 {
        rep.detail(format!("  removed {gone} files of an earlier run"));
    }
}

// ------------------------------------------------------------------------------------------------ the sources

/// Reads one sound file for a sound object: `(bytes, where they came from)`.
type Reader<'a> = dyn FnMut(&SoundInfo) -> Result<(Vec<u8>, &'static str)> + 'a;

/// The sound files of the weapon bank that are not in the bank itself: the entries of `wpn.zip`, a stored zip inside a pak.
struct Media {
    zip: Option<NestedZip>,
    /// media id -> entry number
    index: HashMap<u32, usize>,
}

impl Media {
    fn none() -> Media {
        Media { zip: None, index: HashMap::new() }
    }

    fn open(paks: &mut PakSet, name: &str) -> Result<Media> {
        let zip = paks.open_nested(name)?;
        let mut index = HashMap::new();
        for i in 0..zip.len() {
            let Some(entry) = zip.name_for_index(i) else { continue };
            let file = entry.rsplit(['/', '\\']).next().unwrap_or(entry);
            if let Some((stem, ext)) = file.rsplit_once('.') {
                if ext.eq_ignore_ascii_case("wem") {
                    if let Ok(id) = stem.parse::<u32>() {
                        index.entry(id).or_insert(i);
                    }
                }
            }
        }
        Ok(Media { zip: Some(zip), index })
    }

    /// A sound stored in the bank comes from the bank; everything else, and what the bank does not hold after all, from the zip.
    fn read(&mut self, bank: &Bank, sound: &SoundInfo) -> Result<(Vec<u8>, &'static str)> {
        if sound.stream_type == 0 {
            if let Some(bytes) = bank.embedded_media(sound.media_id) {
                return Ok((bytes.to_vec(), "the bank"));
            }
        }
        let Some(zip) = self.zip.as_mut() else { bail!("the archive with the sound files ({ZIP}) was not found in the game files") };
        let Some(&at) = self.index.get(&sound.media_id) else { bail!("there is no {}.wem in {ZIP}", sound.media_id) };
        let mut entry = zip.by_index(at).with_context(|| format!("{}.wem cannot be opened in {ZIP}", sound.media_id))?;
        if entry.size() > MAX_WEM {
            bail!("the file is unexpectedly large ({})", human(entry.size()));
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).with_context(|| format!("{}.wem cannot be read from {ZIP}", sound.media_id))?;
        Ok((bytes, "wpn.zip"))
    }
}

/// The pak entry called `exact`, or else the first one whose name ends with `suffix` (the folders may differ a little).
fn find_entry(paks: &PakSet, exact: &str, suffix: &str) -> Option<String> {
    if paks.contains(exact) {
        return Some(exact.to_string());
    }
    paks.find(|k| k.ends_with(suffix)).into_iter().next()
}

fn has_event(bank: &Bank, id: u32) -> bool {
    bank.object(id).is_some_and(|o| o.kind == bnk::kind::EVENT)
}

/// Help for events the weapon bank does not have: names of events that do exist, and other banks that hold the missing ones.
#[derive(Default)]
struct Lookup {
    /// lower-case names of events that exist in the weapon bank
    known: Vec<String>,
    /// missing event id -> the other bank it was found in
    elsewhere: HashMap<u32, String>,
}

impl Lookup {
    fn build(paks: &mut PakSet, bank: &Bank, bank_name: &str, slots: &[Slot], missing: &HashSet<u32>) -> Lookup {
        let events: HashSet<u32> = bank.event_ids().collect();
        // the bank only has hashes; names come from the table itself and from the event lists the game ships
        let mut known: BTreeSet<String> = slots.iter().filter(|s| events.contains(&bnk::fnv1_lower(&s.event))).map(|s| s.event.to_ascii_lowercase()).collect();
        for name in paks.find(|k| k.starts_with("sounds/") && k.ends_with(".events.csv")).into_iter().take(MAX_LISTS) {
            let Ok(text) = paks.read_text(&name, MAX_LIST) else { continue };
            known.extend(words(&text).filter(|w| events.contains(&bnk::fnv1_lower(w))).map(str::to_ascii_lowercase));
        }
        let mut elsewhere = HashMap::new();
        let mut budget = OTHER_BANKS_BUDGET;
        for name in paks.find(|k| k.ends_with(".bnk")) {
            if name.eq_ignore_ascii_case(bank_name) || elsewhere.len() == missing.len() {
                continue;
            }
            let Ok(info) = paks.info(&name) else { continue };
            if info.size > MAX_OTHER_BANK || info.size > budget {
                continue;
            }
            budget -= info.size;
            let Ok(other) = paks.read(&name, MAX_OTHER_BANK).and_then(Bank::parse) else { continue };
            for id in missing.iter().filter(|id| has_event(&other, **id)) {
                elsewhere.entry(*id).or_insert_with(|| name.clone());
            }
        }
        Lookup { known: known.into_iter().collect(), elsewhere }
    }

    /// Up to `n` known event names that share the most words (the parts between underscores) with `wanted`.
    fn closest(&self, wanted: &str, n: usize) -> Vec<&str> {
        let wanted = wanted.to_ascii_lowercase();
        let wanted_words: HashSet<&str> = wanted.split('_').filter(|w| !w.is_empty()).collect();
        let mut scored: Vec<(usize, usize, &str)> = self
            .known
            .iter()
            .filter(|k| **k != wanted)
            .filter_map(|k| {
                let shared = k.split('_').collect::<HashSet<_>>().iter().filter(|w| wanted_words.contains(*w)).count();
                (shared > 0).then(|| (shared, k.len().abs_diff(wanted.len()), k.as_str()))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(b.2)));
        scored.into_iter().take(n).map(|s| s.2).collect()
    }
}

// ------------------------------------------------------------------------------------------------ what one play is made of

/// One sound of one play of an event, with how loud, how high and how late it plays.
#[derive(Debug, Clone)]
struct Voice {
    sound: SoundInfo,
    /// linear gain (from the volume of the sound and of every container above it)
    gain: f32,
    pitch_cents: f32,
    delay_ms: u32,
}

/// How the sounds of one play of an event are worked out.
pub struct Engine {
    /// the bank read exactly; `None` = only the approximate walk is available
    strict: Option<hirc::Parsed>,
}

impl Engine {
    pub fn approximate() -> Engine {
        Engine { strict: None }
    }

    /// Use the exact reading when it understood (nearly) the whole bank, else the approximate walk.
    pub fn choose(parsed: hirc::Parsed) -> Engine {
        let enough = parsed.ok > 0 && parsed.failed * 50 <= parsed.ok;
        Engine { strict: enough.then_some(parsed) }
    }

    pub fn is_exact(&self) -> bool {
        self.strict.is_some()
    }

    /// The voices of one play of `event`, and whether they come from the exact reading.
    fn draw(&self, bank: &Bank, event: u32, rng: &mut dyn FnMut(usize) -> usize) -> (Vec<Voice>, bool) {
        if let Some(p) = &self.strict {
            let voices = hirc::resolve_event(bank, &p.nodes, event, rng);
            if !voices.is_empty() {
                let voices = voices
                    .into_iter()
                    .map(|v| Voice {
                        sound: SoundInfo { plugin: v.source.plugin, stream_type: v.source.stream_type, media_id: v.source.media_id, in_memory_size: 0 },
                        gain: v.gain,
                        pitch_cents: v.pitch_cents,
                        delay_ms: v.delay_ms,
                    })
                    .collect();
                return (voices, true);
            }
        }
        let sounds = bank.resolve_take(event, &mut |n| rng(n));
        (sounds.into_iter().map(|sound| Voice { sound, gain: 1.0, pitch_cents: 0.0, delay_ms: 0 }).collect(), false)
    }
}

// ------------------------------------------------------------------------------------------------ making one slot

fn channels_text(channels: u16) -> String {
    match channels {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        n => format!("{n} channels"),
    }
}

fn id_list(ids: &[u32]) -> String {
    ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" ")
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// A sound file read and decoded.
fn load_layer(sound: &SoundInfo, read: &mut Reader) -> Result<(Pcm, &'static str)> {
    let (bytes, from) = read(sound)?;
    let pcm = catch_unwind(AssertUnwindSafe(|| wem::decode(&bytes))).unwrap_or_else(|p| Err(anyhow!("the decoder crashed ({})", panic_text(&*p)))).context("it could not be decoded")?;
    if pcm.samples.is_empty() {
        bail!("it holds no sound");
    }
    Ok((pcm, from))
}

/// One take, mixed.
struct Mixed {
    pcm: Pcm,
    layers: usize,
    /// seconds of the longest layer (a mix longer than `MAX_SECONDS` is cut)
    longest: f64,
}

/// Read, decode and mix the sounds of one play of the event. A file that cannot be used leaves out only its own layer;
/// the take fails when no layer is left.
fn build_take(voices: &[Voice], read: &mut Reader, rep: &mut Report) -> Result<Mixed> {
    if voices.is_empty() {
        bail!("this play of the event reaches no sound that this tool can follow");
    }
    if voices.len() > MAX_LAYERS {
        bail!("this play of the event uses {} sound files at the same time, more than the {MAX_LAYERS} this tool will mix (the sound bank may be laid out differently than expected)", voices.len());
    }
    let mut layers: Vec<Pcm> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for v in voices {
        let id = v.sound.media_id;
        match load_layer(&v.sound, read) {
            Ok((pcm, from)) => {
                let peak = pcm.peak();
                let how = if v.gain != 1.0 || v.pitch_cents != 0.0 || v.delay_ms != 0 {
                    format!(", played at {:+.1} dB{}{}", 20.0 * v.gain.max(1e-6).log10(), if v.pitch_cents != 0.0 { format!(", pitch {:+.0} cents", v.pitch_cents) } else { String::new() }, if v.delay_ms != 0 { format!(", starting {} ms late", v.delay_ms) } else { String::new() })
                } else {
                    String::new()
                };
                rep.detail(format!("    media {id}: {:.2} s, {}, {} Hz, loudest sample {peak} ({:.0}% of full scale), read from {from}{how}", pcm.seconds(), channels_text(pcm.channels), pcm.sample_rate, peak as f64 * 100.0 / 32767.0));
                layers.push(apply_voice(&pcm, v.gain, v.pitch_cents, v.delay_ms));
            }
            Err(e) => {
                rep.detail_wrapped("    ", &format!("media {id}: LEFT OUT, {e:#} (the sound object uses plugin {:#010x})", v.sound.plugin));
                problems.push(format!("media {id}: {e:#}"));
            }
        }
    }
    let longest = layers.iter().map(|l| l.seconds()).fold(0.0, f64::max);
    let layer_count = layers.len();
    match mix_layers(&layers) {
        Some(pcm) => Ok(Mixed { pcm, layers: layer_count, longest }),
        None => {
            let more = problems.len().saturating_sub(3);
            let shown = problems.iter().take(3).cloned().collect::<Vec<_>>().join("; ");
            bail!("none of its {} sound files could be used ({shown}{})", voices.len(), if more > 0 { format!("; and {more} more") } else { String::new() });
        }
    }
}

/// Everything for one slot: draw the takes, build and write their files, tell the report. Returns the index entry, or why
/// the slot got no sound.
fn make_slot(slot: &Slot, bank: &Bank, engine: &Engine, read: &mut Reader, dir: &Path, rep: &mut Report) -> Result<IndexEntry> {
    let event = bnk::fnv1_lower(&slot.event);
    let mut seen: Vec<Vec<u32>> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut seconds: Vec<f64> = Vec::new();
    let mut last_problem: Option<String> = None;
    'takes: for take in 0..slot.takes {
        for attempt in 0..=RETRIES {
            let mut rng = SplitMix64(take_seed(&slot.id, take, attempt));
            let (voices, exact) = engine.draw(bank, event, &mut |n| rng.below(n));
            let mut media: Vec<u32> = voices.iter().map(|v| v.sound.media_id).collect();
            media.sort_unstable();
            media.dedup();
            if seen.contains(&media) {
                continue; // the same sound files as an earlier take: draw again
            }
            seen.push(media.clone());
            let n = files.len() + 1;
            rep.detail_wrapped("  ", &format!("variation {n}: sound files {} ({})", if media.is_empty() { "(none)".to_string() } else { id_list(&media) }, if exact { "volumes, delays and choices read exactly from the bank" } else { "approximate walk: every sound at full volume" }));
            match build_take(&voices, read, rep) {
                Ok(mixed) => {
                    let file = format!("{}_{n}.wav", slot.id);
                    std::fs::write(dir.join(&file), wem::wav_bytes(&mixed.pcm)).with_context(|| format!("could not save {file} in {}", dir.display()))?;
                    let cut = if mixed.longest > MAX_SECONDS as f64 { format!(", cut at {MAX_SECONDS} s with a short fade-out") } else { String::new() };
                    rep.detail(format!(
                        "    -> {file}: {:.2} s, {}, {} Hz, loudest sample {} (mix of {} layer{}{cut})",
                        mixed.pcm.seconds(),
                        channels_text(mixed.pcm.channels),
                        mixed.pcm.sample_rate,
                        mixed.pcm.peak(),
                        mixed.layers,
                        if mixed.layers == 1 { "" } else { "s" }
                    ));
                    seconds.push(round2(mixed.pcm.seconds()));
                    files.push(file);
                    continue 'takes;
                }
                Err(e) => {
                    rep.detail_wrapped("    ", &format!("this variation cannot be used: {e:#}"));
                    last_problem = Some(format!("{e:#}"));
                }
            }
        }
        break; // no new variation turned up in all those draws: the event has no more of them
    }
    if files.is_empty() {
        bail!("{}", last_problem.unwrap_or_else(|| "the event plays no sound that this tool can follow".to_string()));
    }
    if files.len() < slot.takes {
        rep.detail_wrapped("  ", &format!("The game has only {} different version{} of this sound, so {} file{} made instead of {}.", files.len(), if files.len() == 1 { "" } else { "s" }, files.len(), if files.len() == 1 { " was" } else { "s were" }, slot.takes));
    }
    Ok(IndexEntry { slot: slot.id.clone(), event: slot.event.clone(), looped: slot.looped, files, seconds })
}

// ------------------------------------------------------------------------------------------------ the whole job

/// Runs one stage. A problem (an error or a crash) is told in plain words and gives `None`.
fn stage<T>(rep: &mut Report, title: &str, f: impl FnOnce(&mut Report) -> Result<T>) -> Option<T> {
    rep.section(title);
    match catch_unwind(AssertUnwindSafe(|| f(rep))) {
        Ok(Ok(v)) => Some(v),
        Ok(Err(e)) => {
            rep.say_wrapped("  ", &format!("PROBLEM: {e:#}"));
            None
        }
        Err(p) => {
            rep.say_wrapped("  ", &format!("PROBLEM: this part of the program crashed ({}). Please send me the report.", panic_text(&*p)));
            None
        }
    }
}

/// Is `path` (which may not exist yet) the same as `folder` or below it? The nearest folder that exists decides.
pub(crate) fn is_inside(path: &Path, folder: &Path) -> bool {
    let Ok(folder) = folder.canonicalize() else { return false };
    let mut p = path.to_path_buf();
    loop {
        if let Ok(real) = p.canonicalize() {
            return real.starts_with(&folder);
        }
        if !p.pop() {
            return false;
        }
    }
}

/// Where the report goes: the folder `prepare-sm2` next to the assets folder.
pub fn report_path(out: &Path) -> PathBuf {
    out.parent().unwrap_or(out).join("prepare-sm2").join("prepare-report.txt")
}

/// The job could not be done: say so and what to do about it.
fn give_up(mut rep: Report, outcome: Outcome, report: &Path) -> Outcome {
    rep.section("Done");
    rep.say("  Nothing was prepared this time. The message above says what is wrong.");
    rep.say_wrapped("  ", &format!("If you cannot fix it yourself, please send me the report:  {}", report.display()));
    outcome
}

pub fn run(opts: &Opts) -> Outcome {
    let report = report_path(&opts.out);
    let sounds_dir = opts.out.join("sounds");
    let mut outcome = Outcome::default();

    // Space Marine 2 is looked for first, before anything is created: nothing may be written inside its folder
    let found = find_sm2(opts.sm2.as_deref());
    if let Ok((root, _)) = &found {
        if is_inside(&opts.out, root) || is_inside(&report, root) {
            println!("Ashen Marine - preparing your Space Marine 2 sounds (version {VERSION})");
            for line in wrap(&format!("The folder for the converted sounds ({}) is inside your Space Marine 2 folder ({}). This program never writes anything into the game folder, so nothing was done. Please start it again with  --out \"<a folder somewhere else>\"  or move this program out of the game folder.", opts.out.display(), root.display()), 100) {
                println!("  {line}");
            }
            return outcome;
        }
    }
    let mut rep = Report::create(&report);

    rep.say(format!("Ashen Marine - preparing your Space Marine 2 sounds (version {VERSION})"));
    rep.say("  This only READS your Space Marine 2 files. It never changes, moves or uploads anything of the game.");
    rep.say_wrapped("  ", &format!("It makes plain sound files (.wav) for the mod and puts them in:  {}", sounds_dir.display()));
    rep.say_wrapped("  ", &format!("A report of what it did is written to:  {}", report.display()));
    rep.say("  This usually takes less than a minute (a slow disk can take a few minutes). Please keep this window open.");

    let Some((root, build)) = stage(&mut rep, "1/4 Finding Space Marine 2", |rep| {
        let found = found?;
        rep.say_wrapped("  ", &format!("found it:  {}", found.0.display()));
        rep.say(format!("  Steam build id: {}", found.1.as_deref().unwrap_or("unknown")));
        Ok(found)
    }) else {
        return give_up(rep, outcome, &report);
    };

    let Some(mut paks) = stage(&mut rep, "2/4 Opening the game files (read-only)", |rep| {
        open_paks(rep, &root).map_err(|e| {
            anyhow!("{e:#}. Is that really the Space Marine 2 folder (it should contain a folder called client_pc)? If the game is installed, Steam can check it: right click the game, Properties, Installed Files, Verify integrity of game files")
        })
    }) else {
        return give_up(rep, outcome, &report);
    };

    let Some((slots, bank_name, bank, mut media, engine)) = stage(&mut rep, "3/4 Reading the weapon sound list", |rep| {
        let slots = slots()?;
        let Some(bank_name) = find_entry(&paks, BANK, "/wpn.bnk") else {
            bail!("this Space Marine 2 has no weapon sound bank ({BANK}). Is the game fully installed and up to date? Steam can check it: right click the game, Properties, Installed Files, Verify integrity of game files")
        };
        let bank = Bank::parse(paks.read(&bank_name, MAX_BANK)?).with_context(|| format!("{bank_name} could not be read"))?;
        rep.say(format!("  weapon sound bank:  {bank_name} ({} sound events)", bank.event_ids().count()));
        rep.detail(format!("  bank version {}, {} objects", bank.version, bank.objects.len()));
        let media = match find_entry(&paks, ZIP, "/wpn.zip") {
            None => {
                rep.say_wrapped("  ", &format!("PROBLEM: the archive with the sound files ({ZIP}) was not found. Only sounds stored in the bank itself can be made."));
                Media::none()
            }
            Some(name) => match Media::open(&mut paks, &name) {
                Ok(m) => {
                    rep.say(format!("  sound files:        {name} ({} files)", m.index.len()));
                    m
                }
                Err(e) => {
                    rep.say_wrapped("  ", &format!("PROBLEM: {name} cannot be opened ({e:#}). Only sounds stored in the bank itself can be made."));
                    Media::none()
                }
            },
        };
        let wanted: usize = slots.iter().map(|s| s.takes).sum();
        rep.say(format!("  the mod wants {} sounds, up to {wanted} files", slots.len()));
        // how exactly can this bank be read? (the report tells me whether my picture of the format is right)
        let parsed = hirc::parse_bank(&bank);
        rep.detail(format!("  reading the bank exactly: {}", parsed.summary()));
        for (kind, id, why) in &parsed.failures {
            rep.detail_wrapped("    ", &format!("object {id} ({}) was not understood: {why}", bnk::kind_name(*kind)));
        }
        let engine = Engine::choose(parsed);
        rep.detail(if engine.is_exact() {
            "  using the exact reading: volumes, delays, weights and switches come from the bank"
        } else {
            "  the bank could not be read exactly, so an approximate walk is used (every sound at full volume)"
        });
        Ok((slots, bank_name, bank, media, engine))
    }) else {
        return give_up(rep, outcome, &report);
    };
    outcome.slots = slots.len();

    rep.section("4/4 Making the sounds");
    if let Err(e) = std::fs::create_dir_all(&sounds_dir) {
        rep.say_wrapped("  ", &format!("PROBLEM: cannot create the folder {} ({e}). Is that place read-only? You can start this program with  --out \"<another folder>\"  to choose where the sounds go.", sounds_dir.display()));
        return give_up(rep, outcome, &report);
    }
    // events the bank lacks: look for similar names and for other banks once, before anything is written
    let missing: HashSet<u32> = slots.iter().map(|s| bnk::fnv1_lower(&s.event)).filter(|id| !has_event(&bank, *id)).collect();
    let lookup = if missing.is_empty() { Lookup::default() } else { Lookup::build(&mut paks, &bank, &bank_name, &slots, &missing) };
    clear_old_output(&opts.out, &sounds_dir, &slots, &mut rep);
    // a second set of an earlier run must not outlive this one: the game would find its index and play it
    clear_old_output(&opts.out, &opts.out.join(ALT_SET), &slots, &mut rep);

    // The set the owner already judged good is made the way kit 4 made it (every sound at full volume). When the bank can be read
    // exactly and both sets are wanted, the exact reading goes into a second folder for comparison.
    let (engine, alt_engine) = if opts.ab && engine.is_exact() { (Engine::approximate(), Some(engine)) } else { (engine, None) };
    let source = format!("Space Marine 2 {}", build.as_deref().unwrap_or("unknown"));
    let mut read = |s: &SoundInfo| media.read(&bank, s);
    let mut set = render_set(&slots, &bank, &bank_name, &lookup, &engine, &mut read, &sounds_dir, &mut rep, None);
    outcome.prepared = set.entries.len();
    outcome.files = set.entries.iter().map(|e| e.files.len()).sum();
    outcome.failed = std::mem::take(&mut set.failed);

    rep.section("Done");
    rep.say(format!("prepared {} of {} slots, {} files", outcome.prepared, outcome.slots, outcome.files));
    if !outcome.failed.is_empty() {
        rep.say("These sounds could NOT be prepared (the mod stays silent for them):");
        for (slot, why) in &outcome.failed {
            rep.say_item("  - ", &format!("{slot}: {why}"));
        }
    }
    if set.entries.is_empty() {
        rep.say("  Nothing could be prepared, so the mod has no Space Marine 2 sounds yet.");
        rep.say_wrapped("  ", &format!("Please send me the report so I can see why:  {}", report.display()));
        return outcome;
    }
    if opts.normalize {
        normalize_loudness(&sounds_dir, &set.entries, &mut rep);
    }
    // the second set, when there is one: a failure here never spoils the first
    let mut alt_ready = false;
    if let Some(alt) = &alt_engine {
        let alt_dir = opts.out.join(ALT_SET);
        rep.section("The second set: the bank read exactly");
        rep.say_wrapped("  ", "These are the same sounds with the volumes, delays and weights the game's own sound bank gives them. In the game, key F9 switches between this set and the first one.");
        let made = std::fs::create_dir_all(&alt_dir).map_err(|e| anyhow!("cannot create {}: {e}", alt_dir.display())).map(|_| {
            clear_old_output(&opts.out, &alt_dir, &slots, &mut rep);
            let alt_set = render_set(&slots, &bank, &bank_name, &lookup, alt, &mut read, &alt_dir, &mut rep, Some("exact"));
            if opts.normalize && !alt_set.entries.is_empty() {
                normalize_loudness(&alt_dir, &alt_set.entries, &mut rep);
            }
            alt_set
        });
        match made {
            Ok(alt_set) if !alt_set.entries.is_empty() => match write_atomic(&alt_dir.join("index.json"), index_json(&source, &alt_set.entries).as_bytes()) {
                Ok(()) => {
                    outcome.alt_prepared = alt_set.entries.len();
                    outcome.alt_files = alt_set.entries.iter().map(|e| e.files.len()).sum();
                    alt_ready = true;
                    rep.say(format!("  second set: {} of {} slots, {} files", outcome.alt_prepared, outcome.slots, outcome.alt_files));
                }
                Err(e) => rep.say_wrapped("  ", &format!("The second set could not be finished ({e:#}); the first set is not affected.")),
            },
            Ok(_) => rep.say_wrapped("  ", "The second set came out empty; the first set is not affected."),
            Err(e) => rep.say_wrapped("  ", &format!("The second set could not be made ({e:#}); the first set is not affected.")),
        }
        if !alt_ready {
            // no half-made second set may be left for the game to find
            let _ = std::fs::remove_file(alt_dir.join("index.json"));
        }
    }
    // the index first, the marker last: ready.json only exists when everything above finished
    let written = write_atomic(&sounds_dir.join("index.json"), index_json(&source, &set.entries).as_bytes()).and_then(|_| write_atomic(&opts.out.join("ready.json"), ready_json(outcome.prepared, outcome.files, alt_ready).as_bytes()));
    if let Err(e) = written {
        rep.say_wrapped("  ", &format!("PROBLEM: {e:#}. The sounds are not marked as ready."));
        return outcome;
    }
    outcome.ready = true;
    let took = rep.elapsed();
    rep.say_wrapped("  ", &format!("Finished in {took:.0} s. The sounds are in:  {}", sounds_dir.display()));
    if outcome.failed.is_empty() {
        rep.say("  Everything is ready. You can close this window.");
    } else {
        rep.say_wrapped("  ", &format!("The mod works without the missing ones; it just stays silent for them. If you would like me to look into why, please send me the report:  {}", report.display()));
    }
    outcome
}

/// What rendering every slot of the table into one folder gave.
struct SetResult {
    entries: Vec<IndexEntry>,
    /// `(slot, why)` of every slot that got no sound
    failed: Vec<(String, String)>,
}

/// Render every slot with `engine` into `dir`. `label` is `Some` for the second set (its lines in the report are shorter).
#[allow(clippy::too_many_arguments)]
fn render_set(slots: &[Slot], bank: &Bank, bank_name: &str, lookup: &Lookup, engine: &Engine, read: &mut Reader, dir: &Path, rep: &mut Report, label: Option<&str>) -> SetResult {
    let mut set = SetResult { entries: Vec::new(), failed: Vec::new() };
    let tag = label.map(|l| format!(" [{l}]")).unwrap_or_default();
    for (i, slot) in slots.iter().enumerate() {
        let event = bnk::fnv1_lower(&slot.event);
        rep.detail("");
        rep.detail(format!("---- slot {} of {}: {}{tag} ----", i + 1, slots.len(), slot.id));
        if label.is_none() {
            rep.detail(format!("  Space Marine 2 event {}  (id {event} = {event:#010x}); {} variation{} wanted, loop: {}", slot.event, slot.takes, if slot.takes == 1 { "" } else { "s" }, if slot.looped { "yes" } else { "no" }));
        }
        let made = if has_event(bank, event) {
            if label.is_none() {
                rep.detail(format!("  The event is in {bank_name}. What it reaches:"));
                for line in bank.event_tree(event).lines() {
                    rep.detail(format!("    {line}"));
                }
            }
            catch_unwind(AssertUnwindSafe(|| make_slot(slot, bank, engine, read, dir, rep))).unwrap_or_else(|p| Err(anyhow!("this sound crashed the program ({})", panic_text(&*p))))
        } else {
            Err(explain_missing(slot, bank, bank_name, lookup, rep))
        };
        match made {
            Ok(entry) => {
                let secs: Vec<String> = entry.seconds.iter().map(|s| format!("{s:.2} s")).collect();
                rep.say(format!("  [{:>2}/{}]{tag} {} ... {} variation{} ({})", i + 1, slots.len(), slot.id, entry.files.len(), if entry.files.len() == 1 { "" } else { "s" }, secs.join(", ")));
                set.entries.push(entry);
            }
            Err(e) => {
                let why = format!("{e:#}");
                rep.say(format!("  [{:>2}/{}]{tag} {} ... NO SOUND", i + 1, slots.len(), slot.id));
                rep.say_wrapped("          ", &why);
                // whatever was written for this slot before it failed does not belong in the result
                clear_slot(dir, &slot.id);
                set.failed.push((slot.id.clone(), why));
            }
        }
    }
    set
}

/// The loudest sample of all the sounds should be near full scale: sounds made from quiet game levels are all brought up by
/// the same factor, so how loud they are compared with each other stays as the game has it.
fn normalize_loudness(sounds: &Path, entries: &[IndexEntry], rep: &mut Report) {
    const TARGET: f64 = 0.9 * 32767.0;
    const MAX_BOOST: f64 = 16.0;
    let files: Vec<PathBuf> = entries.iter().flat_map(|e| e.files.iter().map(|f| sounds.join(f))).collect();
    let mut loudest = 0i32;
    let mut decoded: Vec<(PathBuf, Pcm)> = Vec::new();
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(pcm) = wem::decode(&bytes) else { continue };
        loudest = loudest.max(pcm.peak() as i32);
        decoded.push((path, pcm));
    }
    if loudest == 0 {
        return;
    }
    let factor = (TARGET / loudest as f64).min(MAX_BOOST);
    if factor < 1.1 {
        rep.detail(format!("  loudness: the loudest sample is already at {:.0}% of full scale; nothing was changed", loudest as f64 * 100.0 / 32767.0));
        return;
    }
    let mut changed = 0;
    for (path, pcm) in &decoded {
        let louder = Pcm { channels: pcm.channels, sample_rate: pcm.sample_rate, samples: pcm.samples.iter().map(|s| (*s as f64 * factor).round().clamp(-32768.0, 32767.0) as i16).collect() };
        if std::fs::write(path, wem::wav_bytes(&louder)).is_ok() {
            changed += 1;
        }
    }
    rep.say_wrapped("  ", &format!("All sounds were made {:+.1} dB louder together (the loudest one was at {:.0}% of full scale); {changed} files. How loud they are compared with each other is unchanged.", 20.0 * factor.log10(), loudest as f64 * 100.0 / 32767.0));
}

/// Say why an event is missing from the weapon bank, and what is known that might help; returns the one-line reason.
fn explain_missing(slot: &Slot, bank: &Bank, bank_name: &str, lookup: &Lookup, rep: &mut Report) -> anyhow::Error {
    let event = bnk::fnv1_lower(&slot.event);
    rep.detail(format!("  The event is NOT in {bank_name}, which holds {} events.", bank.event_ids().count()));
    let other = lookup.elsewhere.get(&event);
    if let Some(other) = other {
        rep.detail_wrapped("  ", &format!("It does exist in another sound bank, {other}, but this tool only reads {bank_name}."));
    }
    let close = lookup.closest(&slot.event, 5);
    if close.is_empty() {
        rep.detail("  No other event names are known to compare it with.");
    } else {
        rep.detail_wrapped("  ", &format!("The closest event names that do exist in {bank_name}: {}", close.join(", ")));
    }
    match other {
        Some(other) => anyhow!("Space Marine 2 has no event called \"{}\" in {bank_name}; it is in {other}, which this tool does not read yet", slot.event),
        None => anyhow!("Space Marine 2 has no event called \"{}\" in {bank_name}", slot.event),
    }
}

/// Delete the `.wav` files of one slot (after it failed half way).
fn clear_slot(sounds: &Path, slot: &str) {
    if let Ok(dir) = std::fs::read_dir(sounds) {
        for e in dir.filter_map(|e| e.ok()) {
            if e.file_name().to_str().is_some_and(|n| is_take_file(slot, n)) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ashen_sm2::bnk::{kind, testbank::Builder, ACTION_PLAY};
    use ashen_sm2::wem::test_wem;

    /// A plain 16-bit wem of `frames` identical frames.
    fn pcm_wem(rate: u32, frame: &[i16], frames: usize) -> Vec<u8> {
        let data: Vec<u8> = (0..frames).flat_map(|_| frame.iter().flat_map(|s| s.to_le_bytes())).collect();
        test_wem(1, frame.len() as u16, rate, 16, &[], &data)
    }

    fn slot(takes: usize) -> Slot {
        Slot { id: "test_slot".to_string(), event: "test_event".to_string(), takes, looped: false }
    }

    /// A bank whose event `test_event` plays object 100 (made by `f`); sound `n` (1 to 4) has media `10 + n`.
    fn bank(f: impl FnOnce(&mut Builder)) -> Bank {
        let mut b = Builder::new(150);
        for n in 1..=4 {
            b.sound(n, 10 + n, 1);
        }
        f(&mut b);
        b.action(900, ACTION_PLAY, 100).event(bnk::fnv1_lower("test_event"), &[900]);
        Bank::parse(b.build()).unwrap()
    }

    /// Media 11..=14 hold 1000, 2000, 3000 and 100 in every frame (0.01 s, 0.02 s, 0.03 s and 0.015 s at 44.1 kHz); the
    /// ones in `broken` cannot be read.
    fn reader(broken: &'static [u32]) -> impl FnMut(&SoundInfo) -> Result<(Vec<u8>, &'static str)> {
        move |s: &SoundInfo| {
            if broken.contains(&s.media_id) {
                bail!("it is not there");
            }
            let (value, frames) = match s.media_id {
                11 => (1000, 441),
                12 => (2000, 882),
                13 => (3000, 1323),
                14 => (100, 661),
                other => bail!("the test has no media {other}"),
            };
            Ok((pcm_wem(44100, &[value], frames), "the test"))
        }
    }

    /// Run `make_slot` in a temp folder: its result, the first sample of every file it lists, and the report text.
    fn make(slot: &Slot, bank: &Bank, broken: &'static [u32]) -> (Result<IndexEntry>, Vec<i16>, String) {
        let t = tempfile::tempdir().unwrap();
        let mut rep = Report::create(&t.path().join("report.txt"));
        let mut read = reader(broken);
        let res = make_slot(slot, bank, &Engine::approximate(), &mut read, t.path(), &mut rep);
        let firsts = res.as_ref().map(|e| e.files.iter().map(|f| wem::decode(&std::fs::read(t.path().join(f)).unwrap()).unwrap().samples[0]).collect()).unwrap_or_default();
        (res, firsts, std::fs::read_to_string(t.path().join("report.txt")).unwrap())
    }

    #[test]
    fn the_built_in_slot_table_parses_and_has_what_the_tests_build_on() {
        let slots = slots().unwrap();
        assert!(slots.len() >= 3);
        for s in &slots {
            assert!(!s.id.is_empty() && !s.event.is_empty() && (1..=MAX_TAKES).contains(&s.takes), "{s:?}");
        }
        let row = |id: &str| slots.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("design/sheets/sounds.json has no row {id}: the fake install of the tests builds on it"));
        assert_eq!((row("chainsword_swing_1").event.as_str(), row("chainsword_swing_1").looped), ("wpn_melee_chswd_light_1hit", false));
        assert_eq!((row("chainsword_idle").event.as_str(), row("chainsword_idle").looped), ("wpn_melee_chswd_idle_loop", true));
        assert_eq!((row("boltpistol_fire").event.as_str(), row("boltpistol_fire").looped), ("wpn_firearm_shoot_2d_bolt_pistol", false));
        assert!(row("chainsword_swing_1").takes > 1 && row("boltpistol_fire").takes > 1);
    }

    #[test]
    fn a_damaged_slot_table_is_refused_with_a_reason() {
        let row = |extra: &str| format!("{{\"rows\": [{{\"id\": \"a\", \"sm2_event\": \"e\", \"takes\": 2, \"looped\": false{extra}}}]}}");
        assert_eq!(parse_slots(&row(", \"volume\": 0.5")).unwrap(), vec![Slot { id: "a".into(), event: "e".into(), takes: 2, looped: false }], "other columns are ignored");
        for (json, why) in [
            ("{", "damaged"),
            ("{\"rows\": 3}", "no rows"),
            ("{\"rows\": []}", "empty"),
            ("{\"rows\": [{\"id\": \"a\", \"takes\": 1, \"looped\": false}]}", "sm2_event"),
            ("{\"rows\": [{\"id\": \"../a\", \"sm2_event\": \"e\", \"takes\": 1, \"looped\": false}]}", "file name"),
            ("{\"rows\": [{\"id\": \"a\", \"sm2_event\": \"e\", \"takes\": 0, \"looped\": false}]}", "takes"),
            ("{\"rows\": [{\"id\": \"a\", \"sm2_event\": \"e\", \"takes\": 99, \"looped\": false}]}", "takes"),
            ("{\"rows\": [{\"id\": \"a\", \"sm2_event\": \"e\", \"takes\": 1}]}", "looped"),
            ("{\"rows\": [{\"id\": \"a\", \"sm2_event\": \"e\", \"takes\": 1, \"looped\": false}, {\"id\": \"a\", \"sm2_event\": \"f\", \"takes\": 1, \"looped\": false}]}", "twice"),
        ] {
            let err = format!("{:#}", parse_slots(json).unwrap_err());
            assert!(err.contains(why), "{json}: {err}");
        }
    }

    #[test]
    fn take_files_are_told_apart_even_when_one_slot_name_starts_with_another() {
        assert!(is_take_file("chainsword_idle", "chainsword_idle_1.wav"));
        assert!(is_take_file("chainsword_swing_1", "chainsword_swing_1_12.wav"));
        assert!(!is_take_file("chainsword_idle", "chainsword_idle_start_1.wav"), "that file belongs to chainsword_idle_start");
        assert!(is_take_file("chainsword_idle_start", "chainsword_idle_start_1.wav"));
        assert!(!is_take_file("chainsword_swing_1", "chainsword_swing_12_1.wav"));
        assert!(!is_take_file("chainsword_swing_1", "chainsword_swing_1.wav"));
        assert!(!is_take_file("chainsword_swing_1", "chainsword_swing_1_2.wav.bak"));
        assert!(!is_take_file("chainsword_swing_1", "chainsword_swing_1_.wav"));
        assert!(!is_take_file("chainsword_swing_1", "index.json"));
    }

    #[test]
    fn the_generator_repeats_for_a_seed_and_the_seeds_differ() {
        let draw = |seed| {
            let mut r = SplitMix64(seed);
            (0..8).map(|_| r.below(1000)).collect::<Vec<_>>()
        };
        assert_eq!(draw(5), draw(5));
        assert_ne!(draw(5), draw(6));
        assert!(draw(5).iter().all(|&v| v < 1000));
        assert_eq!(SplitMix64(0).next_u64(), 0xE220_A839_7B1D_CDAF, "the published first output of splitmix64 for seed 0");
        let seeds: HashSet<u64> = ["a", "b", "chainsword_swing_1"].iter().flat_map(|s| (0..3).flat_map(move |t| (0..=RETRIES).map(move |a| take_seed(s, t, a)))).collect();
        assert_eq!(seeds.len(), 3 * 3 * (RETRIES as usize + 1), "every slot, take and retry has its own seed");
        assert_eq!(take_seed("a", 0, 0), (bnk::fnv1_lower("a") as u64) << 16);
        assert_eq!(take_seed("a", 2, 3) - take_seed("a", 0, 0), 2 * 256 + 3);
    }

    #[test]
    fn the_json_files_have_the_documented_shape() {
        let entries = vec![
            IndexEntry { slot: "chainsword_swing_1".into(), event: "wpn_melee_chswd_light_1hit".into(), looped: false, files: vec!["chainsword_swing_1_1.wav".into(), "chainsword_swing_1_2.wav".into()], seconds: vec![0.81, 0.77] },
            IndexEntry { slot: "chainsword_idle".into(), event: "wpn_melee_chswd_idle_loop".into(), looped: true, files: vec!["chainsword_idle_1.wav".into()], seconds: vec![2.0] },
        ];
        assert_eq!(
            index_json("Space Marine 2 25098992", &entries),
            concat!(
                "{\"format\":1,\"source\":\"Space Marine 2 25098992\",\"sounds\":[",
                "{\"slot\":\"chainsword_swing_1\",\"event\":\"wpn_melee_chswd_light_1hit\",\"loop\":false,\"files\":[\"chainsword_swing_1_1.wav\",\"chainsword_swing_1_2.wav\"],\"seconds\":[0.81,0.77]},",
                "{\"slot\":\"chainsword_idle\",\"event\":\"wpn_melee_chswd_idle_loop\",\"loop\":true,\"files\":[\"chainsword_idle_1.wav\"],\"seconds\":[2.0]}]}"
            )
        );
        assert_eq!(ready_json(13, 27, false), format!("{{\"format\":1,\"tool\":\"ashenmarine-setup {VERSION}\",\"slots\":13,\"files\":27}}"));
        assert_eq!(ready_json(13, 27, true), format!("{{\"format\":1,\"tool\":\"ashenmarine-setup {VERSION}\",\"slots\":13,\"files\":27,\"alt_sets\":[\"sounds-exact\"]}}"));
        // strings are escaped like any JSON string
        let v: Value = serde_json::from_str(&index_json("odd \"name\" \\", &[])).unwrap();
        assert_eq!(v["source"], "odd \"name\" \\");
    }

    #[test]
    fn the_report_goes_to_a_folder_next_to_the_assets() {
        assert_eq!(report_path(Path::new("/kit/ashenmarine/assets")), Path::new("/kit/ashenmarine/prepare-sm2/prepare-report.txt"));
        assert_eq!(report_path(Path::new("/kit/ashenmarine/assets/")), Path::new("/kit/ashenmarine/prepare-sm2/prepare-report.txt"));
        assert_eq!(report_path(Path::new("assets")), Path::new("prepare-sm2/prepare-report.txt"));
    }

    #[test]
    fn a_path_below_a_folder_is_recognised_even_when_it_does_not_exist_yet() {
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("Space Marine 2");
        std::fs::create_dir_all(&game).unwrap();
        assert!(is_inside(&game, &game));
        assert!(is_inside(&game.join("assets"), &game));
        assert!(is_inside(&game.join("a").join("b").join("c"), &game));
        assert!(is_inside(&game.join("a").join("..").join("assets"), &game));
        assert!(!is_inside(&t.path().join("Space Marine 2 sounds").join("assets"), &game), "only the same start of a name");
        assert!(!is_inside(&t.path().join("assets"), &game));
        assert!(!is_inside(&game.join("..").join("assets"), &game), "a way out of the folder");
        assert!(!is_inside(&game, &t.path().join("nothing")), "a folder that is not there contains nothing");
    }

    #[test]
    fn the_closest_names_share_the_most_words() {
        let lookup = Lookup {
            known: ["wpn_melee_chswd_light_1hit", "wpn_melee_chswd_light_2hit", "wpn_melee_chswd_idle_loop", "wpn_firearm_shoot_2d_bolt_pistol", "music_menu", "wpn_melee_hammer_light_1hit", "wpn_melee_chswd_slash_1hit"].map(String::from).to_vec(),
            elsewhere: HashMap::new(),
        };
        let close = lookup.closest("wpn_melee_chswd_light_3hit", 5);
        assert_eq!(close[..2], ["wpn_melee_chswd_light_1hit", "wpn_melee_chswd_light_2hit"], "{close:?}");
        assert_eq!(close.len(), 5);
        assert!(!close.contains(&"music_menu"), "no shared word, no suggestion");
        assert!(!lookup.closest("wpn_melee_chswd_light_1hit", 9).contains(&"wpn_melee_chswd_light_1hit"), "never the wanted name itself");
        assert!(Lookup::default().closest("anything_at_all", 5).is_empty());
    }

    #[test]
    fn takes_differ_and_a_layer_is_added_to_each() {
        // a random container over three sounds, layered with a fourth
        let bank = bank(|b| {
            b.container(kind::RAN_SEQ, 50, &[1, 2, 3]).container(kind::LAYER, 100, &[50, 4]);
        });
        let (res, firsts, report) = make(&slot(3), &bank, &[]);
        let entry = res.unwrap();
        assert_eq!(entry.files, ["test_slot_1.wav", "test_slot_2.wav", "test_slot_3.wav"]);
        let mut sorted = firsts.clone();
        sorted.sort();
        assert_eq!(sorted, [1100, 2100, 3100], "each take is one of the three plus the extra layer");
        assert_eq!(entry.seconds.len(), 3);
        for (first, secs) in firsts.iter().zip(&entry.seconds) {
            // the longer of the picked sound and the extra layer (661 frames) decides the length
            let frames = match first {
                1100 => 661.0,
                2100 => 882.0,
                _ => 1323.0,
            };
            assert!((secs - round2(frames / 44100.0)).abs() < 1e-9, "{first}: {secs}");
        }
        assert!(report.contains("variation 1: sound files") && report.contains("media 14: 0.01 s, mono, 44100 Hz, loudest sample 100"), "{report}");
        assert!(report.contains("-> test_slot_1.wav") && report.contains("mix of 2 layers"), "{report}");
        // the same slot gives the same takes again (a seeded generator, not chance)
        let (again, firsts_again, _) = make(&slot(3), &bank, &[]);
        assert_eq!((again.unwrap().files, firsts_again), (entry.files, firsts));
    }

    #[test]
    fn an_event_with_fewer_variants_gives_fewer_takes() {
        let two = bank(|b| {
            b.container(kind::RAN_SEQ, 100, &[1, 2]);
        });
        let (res, firsts, report) = make(&slot(5), &two, &[]);
        assert_eq!(res.unwrap().files, ["test_slot_1.wav", "test_slot_2.wav"], "no gaps in the numbering");
        let mut sorted = firsts;
        sorted.sort();
        assert_eq!(sorted, [1000, 2000]);
        assert!(report.contains("only 2 different versions of this sound, so 2 files were made instead of 5"), "{report}");
        // one sound is one variation, however many takes are wanted
        let one = bank(|b| {
            b.container(kind::ACTOR_MIXER, 100, &[3]);
        });
        let (res, _, report) = make(&slot(3), &one, &[]);
        assert_eq!(res.unwrap().files, ["test_slot_1.wav"]);
        assert!(report.contains("only 1 different version of this sound, so 1 file was made instead of 3"), "{report}");
    }

    #[test]
    fn a_file_that_fails_leaves_out_its_layer_and_a_variation_without_files_is_skipped() {
        // a layer of sounds 1 and 2 where 2 cannot be read: the take is just sound 1
        let layered = bank(|b| {
            b.container(kind::LAYER, 100, &[1, 2]);
        });
        let (res, firsts, report) = make(&slot(1), &layered, &[12]);
        assert_eq!(res.unwrap().files, ["test_slot_1.wav"]);
        assert_eq!(firsts, [1000]);
        assert!(report.contains("media 12: LEFT OUT, it is not there"), "{report}");
        assert!(report.contains("mix of 1 layer)"), "{report}");

        // random: the variation that is sound 2 has nothing usable, so other draws are made instead
        let random = bank(|b| {
            b.container(kind::RAN_SEQ, 100, &[1, 2, 3]);
        });
        let (res, firsts, report) = make(&slot(3), &random, &[12]);
        assert_eq!(res.unwrap().files.len(), 2, "{report}");
        let mut sorted = firsts;
        sorted.sort();
        assert_eq!(sorted, [1000, 3000]);
    }

    #[test]
    fn a_slot_without_a_single_usable_file_fails_with_the_reason() {
        let layered = bank(|b| {
            b.container(kind::LAYER, 100, &[1, 2]);
        });
        let (res, _, report) = make(&slot(2), &layered, &[11, 12]);
        let why = res.err().expect("nothing to make").to_string();
        assert!(why.contains("none of its 2 sound files could be used") && why.contains("media 11: it is not there"), "{why}");
        assert!(report.contains("this variation cannot be used"), "{report}");
        // an event that plays nothing this tool can follow
        let nothing = bank(|b| {
            b.container(kind::MUSIC_SEGMENT, 100, &[1]);
        });
        let why = make(&slot(1), &nothing, &[]).0.err().unwrap().to_string();
        assert!(why.contains("reaches no sound"), "{why}");
        // a file nobody can decode
        let t = tempfile::tempdir().unwrap();
        let mut rep = Report::create(&t.path().join("r.txt"));
        let mut garbage = |_: &SoundInfo| -> Result<(Vec<u8>, &'static str)> { Ok((b"not a wem at all".to_vec(), "the test")) };
        let single = bank(|b| {
            b.container(kind::LAYER, 100, &[1]);
        });
        let why = make_slot(&slot(1), &single, &Engine::approximate(), &mut garbage, t.path(), &mut rep).err().unwrap().to_string();
        assert!(why.contains("it could not be decoded"), "{why}");
    }

    #[test]
    fn an_event_that_plays_a_crowd_of_sounds_is_refused() {
        let mut b = Builder::new(150);
        let ids: Vec<u32> = (1..=MAX_LAYERS as u32 + 1).collect();
        for id in &ids {
            b.sound(*id, 100 + id, 1);
        }
        b.container(kind::ACTOR_MIXER, 100_000, &ids).action(900, ACTION_PLAY, 100_000).event(bnk::fnv1_lower("test_event"), &[900]);
        let bank = Bank::parse(b.build()).unwrap();
        let t = tempfile::tempdir().unwrap();
        let mut rep = Report::create(&t.path().join("r.txt"));
        let mut never = |_: &SoundInfo| -> Result<(Vec<u8>, &'static str)> { panic!("nothing may be read for a refused event") };
        let why = make_slot(&slot(3), &bank, &Engine::approximate(), &mut never, t.path(), &mut rep).err().unwrap().to_string();
        assert!(why.contains("33 sound files at the same time") && why.contains("more than the 32"), "{why}");
        assert!(std::fs::read_dir(t.path()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".wav")));
    }

    #[test]
    fn a_mix_longer_than_the_limit_is_cut_and_says_so() {
        let b = bank(|b| {
            b.container(kind::LAYER, 100, &[1]);
        });
        let t = tempfile::tempdir().unwrap();
        let mut rep = Report::create(&t.path().join("r.txt"));
        let mut long = |_: &SoundInfo| -> Result<(Vec<u8>, &'static str)> { Ok((pcm_wem(1000, &[500], 9000), "the test")) };
        let entry = make_slot(&slot(1), &b, &Engine::approximate(), &mut long, t.path(), &mut rep).unwrap();
        assert_eq!(entry.seconds, [8.0]);
        let report = std::fs::read_to_string(t.path().join("r.txt")).unwrap();
        assert!(report.contains("cut at 8 s with a short fade-out"), "{report}");
    }

    #[test]
    fn old_output_is_removed_for_the_slots_of_the_table_only() {
        let t = tempfile::tempdir().unwrap();
        let out = t.path();
        let sounds = out.join("sounds");
        std::fs::create_dir_all(&sounds).unwrap();
        for f in ["chainsword_idle_1.wav", "chainsword_idle_start_1.wav", "chainsword_idle_start_2.wav", "other_1.wav", "index.json", "notes.txt", "chainsword_idle_2.wav.bak"] {
            std::fs::write(sounds.join(f), b"x").unwrap();
        }
        std::fs::write(out.join("ready.json"), b"{}").unwrap();
        let table = vec![Slot { id: "chainsword_idle".into(), event: "e".into(), takes: 1, looped: true }];
        let mut rep = Report::create(&out.join("r.txt"));
        clear_old_output(out, &sounds, &table, &mut rep);
        let mut left: Vec<String> = std::fs::read_dir(&sounds).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left, ["chainsword_idle_2.wav.bak", "chainsword_idle_start_1.wav", "chainsword_idle_start_2.wav", "notes.txt", "other_1.wav"]);
        assert!(!out.join("ready.json").exists());
    }

    /// Media 11..=14 as in `reader`, but a bank built with the exact encoders, so the exact reading applies.
    fn exact_bank(objs: Vec<(u8, Vec<u8>)>) -> Bank {
        let mut all = objs;
        all.push(hirc::testenc::action(900, ACTION_PLAY, 100));
        all.push(hirc::testenc::event(bnk::fnv1_lower("test_event"), &[900]));
        Bank::parse(hirc::testenc::bank(all)).unwrap()
    }

    #[test]
    fn the_exact_reading_applies_volume_delay_and_switch_choices() {
        use hirc::testenc as enc;
        // a layer: sound 13 at full volume, sound 12 half as loud (-6.02 dB) and 10 ms late
        let bank = exact_bank(vec![
            enc::obj(kind::SOUND, 1, &enc::sound(100, 13, 0.0, 0)),
            enc::obj(kind::SOUND, 2, &enc::sound(100, 12, -6.0206, 10)),
            enc::obj(kind::LAYER, 100, &enc::layer(0, &[1, 2], 0.0)),
        ]);
        let engine = Engine::choose(hirc::parse_bank(&bank));
        assert!(engine.is_exact());
        let t = tempfile::tempdir().unwrap();
        let mut rep = Report::create(&t.path().join("report.txt"));
        let mut read = reader(&[]);
        let entry = make_slot(&slot(1), &bank, &engine, &mut read, t.path(), &mut rep).unwrap();
        let pcm = wem::decode(&std::fs::read(t.path().join(&entry.files[0])).unwrap()).unwrap();
        assert_eq!(pcm.samples[0], 3000, "only the first layer at the start");
        assert!((pcm.samples[441] as i32 - 4000).abs() <= 1, "the second layer comes in 10 ms later at half level: {}", pcm.samples[441]);
        let report = std::fs::read_to_string(t.path().join("report.txt")).unwrap();
        assert!(report.contains("read exactly from the bank") && report.contains("played at -6.0 dB") && report.contains("starting 10 ms late"), "{report}");

        // a switch container plays what is assigned to its default switch
        let bank = exact_bank(vec![
            enc::obj(kind::SOUND, 1, &enc::sound(100, 11, 0.0, 0)),
            enc::obj(kind::SOUND, 2, &enc::sound(100, 12, 0.0, 0)),
            enc::obj(kind::SWITCH, 100, &enc::switch(0, 22, &[1, 2], &[(21, vec![1]), (22, vec![2])])),
        ]);
        let engine = Engine::choose(hirc::parse_bank(&bank));
        let t = tempfile::tempdir().unwrap();
        let mut rep = Report::create(&t.path().join("report.txt"));
        let mut read = reader(&[]);
        let entry = make_slot(&slot(3), &bank, &engine, &mut read, t.path(), &mut rep).unwrap();
        assert_eq!(entry.files.len(), 1, "the only branch that can play is the default switch's");
        assert_eq!(wem::decode(&std::fs::read(t.path().join(&entry.files[0])).unwrap()).unwrap().samples[0], 2000);
    }

    #[test]
    fn a_bank_that_is_not_understood_exactly_uses_the_approximate_walk() {
        // the synthetic banks of the other tests do not follow the real layout: the exact reader refuses them
        let approx = bank(|b| {
            b.container(kind::LAYER, 100, &[1, 2]);
        });
        let parsed = hirc::parse_bank(&approx);
        assert!(parsed.failed > 0 || parsed.ok == 0);
        let engine = Engine::choose(parsed);
        assert!(!engine.is_exact());
        let t = tempfile::tempdir().unwrap();
        let mut rep = Report::create(&t.path().join("report.txt"));
        let mut read = reader(&[]);
        let entry = make_slot(&slot(1), &approx, &engine, &mut read, t.path(), &mut rep).unwrap();
        assert_eq!(wem::decode(&std::fs::read(t.path().join(&entry.files[0])).unwrap()).unwrap().samples[0], 3000);
        assert!(std::fs::read_to_string(t.path().join("report.txt")).unwrap().contains("approximate walk"));
        // a few failures among very many understood objects are tolerated, many are not
        let mut p = hirc::parse_bank(&exact_bank(vec![hirc::testenc::obj(kind::SOUND, 1, &hirc::testenc::sound(0, 11, 0.0, 0))]));
        p.ok = 100;
        p.failed = 2;
        assert!(Engine::choose(p).is_exact());
        let mut p = hirc::parse_bank(&exact_bank(vec![hirc::testenc::obj(kind::SOUND, 1, &hirc::testenc::sound(0, 11, 0.0, 0))]));
        p.ok = 100;
        p.failed = 3;
        assert!(!Engine::choose(p).is_exact());
    }

    #[test]
    fn the_loudest_sound_is_brought_up_to_near_full_scale_and_the_others_with_it() {
        let t = tempfile::tempdir().unwrap();
        let mk = |name: &str, v: i16| {
            std::fs::write(t.path().join(name), wem::wav_bytes(&Pcm { channels: 1, sample_rate: 48_000, samples: vec![v; 100] })).unwrap();
        };
        mk("a_1.wav", 3000);
        mk("b_1.wav", 1500);
        let entries = vec![
            IndexEntry { slot: "a".into(), event: "e".into(), looped: false, files: vec!["a_1.wav".into()], seconds: vec![0.0] },
            IndexEntry { slot: "b".into(), event: "e".into(), looped: false, files: vec!["b_1.wav".into()], seconds: vec![0.0] },
        ];
        let mut rep = Report::create(&t.path().join("r.txt"));
        normalize_loudness(t.path(), &entries, &mut rep);
        let a = wem::decode(&std::fs::read(t.path().join("a_1.wav")).unwrap()).unwrap().samples[0] as i32;
        let b = wem::decode(&std::fs::read(t.path().join("b_1.wav")).unwrap()).unwrap().samples[0] as i32;
        assert!((a - 29_490).abs() <= 2, "{a}");
        assert!((a - 2 * b).abs() <= 2, "the relation between the two is kept: {a} {b}");
        // already loud: untouched
        let before = std::fs::read(t.path().join("a_1.wav")).unwrap();
        normalize_loudness(t.path(), &entries, &mut rep);
        assert_eq!(std::fs::read(t.path().join("a_1.wav")).unwrap(), before);
        // silence is left alone (no division by zero)
        mk("a_1.wav", 0);
        mk("b_1.wav", 0);
        normalize_loudness(t.path(), &entries, &mut rep);
    }
}
