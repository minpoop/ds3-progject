//! The sound slots the setup tool prepared from the player's own Space Marine 2 install: `sounds/index.json` names each
//! slot and its WAV files ("takes"); this loads them into clips ready for the mixer and picks a take at random without
//! repeating the one before. Pure file reading, so it is tested on any OS.
use crate::mixer::Clip;
use crate::wav;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// One row of the design sheet `sounds.json`: the settings of a slot that are decided by design, not by the files.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetSlot {
    pub id: String,
    pub sm2_event: String,
    pub volume: f32,
    pub looped: bool,
}

/// The slots of the design sheet (embedded at build time, so the sheet stays the source of truth).
pub fn sheet_slots() -> Vec<SheetSlot> {
    #[derive(Deserialize)]
    struct Sheet {
        rows: Vec<Row>,
    }
    #[derive(Deserialize)]
    struct Row {
        id: String,
        sm2_event: String,
        volume: f32,
        looped: bool,
    }
    let sheet: Sheet = serde_json::from_str(include_str!("../../../design/sheets/sounds.json")).expect("design/sheets/sounds.json is valid (checked by tests)");
    sheet.rows.into_iter().map(|r| SheetSlot { id: r.id, sm2_event: r.sm2_event, volume: r.volume, looped: r.looped }).collect()
}

/// `index.json` as the setup tool writes it (format 1).
#[derive(Deserialize)]
struct IndexFile {
    format: u32,
    #[serde(default)]
    sounds: Vec<IndexSlot>,
}

#[derive(Deserialize)]
struct IndexSlot {
    slot: String,
    #[serde(default)]
    event: String,
    #[serde(default, rename = "loop")]
    looped: bool,
    #[serde(default)]
    files: Vec<String>,
}

pub struct Slot {
    pub event: String,
    pub looped: bool,
    pub takes: Vec<Arc<Clip>>,
    last: Option<usize>,
}

#[derive(Default)]
pub struct SoundSet {
    slots: HashMap<String, Slot>,
}

/// What loading found: the sounds that could be read, and a line for everything that could not.
pub struct Loaded {
    pub set: SoundSet,
    pub warnings: Vec<String>,
}

impl SoundSet {
    /// Read `<dir>/index.json` and every WAV it lists (paths are relative to `dir`). A file that cannot be read drops only
    /// that take; a slot without any readable take is left out. `Err` only when the index itself is missing or unusable.
    pub fn load(dir: &Path) -> Result<Loaded, String> {
        let index_path = dir.join("index.json");
        let text = std::fs::read_to_string(&index_path).map_err(|e| format!("cannot read {}: {e}", index_path.display()))?;
        let index: IndexFile = serde_json::from_str(&text).map_err(|e| format!("{} is not a valid sound index: {e}", index_path.display()))?;
        if index.format != 1 {
            return Err(format!("{} has format {} but this version understands format 1", index_path.display(), index.format));
        }
        let mut set = SoundSet::default();
        let mut warnings = Vec::new();
        for s in index.sounds {
            let mut takes = Vec::new();
            for f in &s.files {
                // the index comes from our own tool but a file name is still never allowed to leave the folder
                if f.contains("..") || f.contains('/') || f.contains('\\') || f.contains(':') {
                    warnings.push(format!("{}: ignoring the odd file name {f:?}", s.slot));
                    continue;
                }
                match std::fs::read(dir.join(f)).map_err(|e| e.to_string()).and_then(|b| wav::parse(&b)) {
                    Ok(w) => {
                        let clip = Clip::from_pcm(w.channels, w.sample_rate, &w.samples);
                        if clip.len_frames() == 0 {
                            warnings.push(format!("{}: {f} holds no sound", s.slot));
                        } else {
                            takes.push(Arc::new(clip));
                        }
                    }
                    Err(e) => warnings.push(format!("{}: cannot use {f}: {e}", s.slot)),
                }
            }
            if takes.is_empty() {
                warnings.push(format!("{}: no usable sound, the slot is off", s.slot));
            } else {
                set.slots.insert(s.slot, Slot { event: s.event, looped: s.looped, takes, last: None });
            }
        }
        Ok(Loaded { set, warnings })
    }

    pub fn has(&self, slot: &str) -> bool {
        self.slots.contains_key(slot)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.slots.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    pub fn slot(&self, name: &str) -> Option<&Slot> {
        self.slots.get(name)
    }

    pub fn total_seconds(&self) -> f32 {
        self.slots.values().flat_map(|s| s.takes.iter()).map(|c| c.seconds()).sum()
    }

    /// One take of `slot`, chosen from `random` (any random number), never the one picked last time when there is a choice.
    pub fn pick(&mut self, slot: &str, random: u64) -> Option<Arc<Clip>> {
        let s = self.slots.get_mut(slot)?;
        let n = s.takes.len();
        let i = match (n, s.last) {
            (1, _) => 0,
            (_, None) => (random % n as u64) as usize,
            (_, Some(last)) => {
                let j = (random % (n as u64 - 1)) as usize;
                if j >= last {
                    j + 1
                } else {
                    j
                }
            }
        };
        s.last = Some(i);
        Some(s.takes[i].clone())
    }
}

/// A tiny xorshift generator: sound variety does not need anything better, and it keeps the hook free of a new dependency.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_set(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        let wav = |n: usize, v: i16, ch: u16, rate: u32| wav::build(ch, rate, &vec![v; n * ch as usize]);
        std::fs::write(dir.join("swing_1_1.wav"), wav(4800, 1000, 1, 48_000)).unwrap();
        std::fs::write(dir.join("swing_1_2.wav"), wav(2400, 2000, 2, 48_000)).unwrap();
        std::fs::write(dir.join("swing_1_3.wav"), wav(441, 3000, 2, 44_100)).unwrap();
        std::fs::write(dir.join("idle_1.wav"), wav(441, 500, 2, 44_100)).unwrap();
        std::fs::write(dir.join("broken.wav"), b"not a wav").unwrap();
        let index = r#"{"format":1,"source":"test","sounds":[
            {"slot":"swing_1","event":"e1","loop":false,"files":["swing_1_1.wav","swing_1_2.wav","swing_1_3.wav","broken.wav","missing.wav","../escape.wav"],"seconds":[0.1,0.05,0.01]},
            {"slot":"idle","event":"e2","loop":true,"files":["idle_1.wav"]},
            {"slot":"empty_slot","files":["broken.wav"]}
        ]}"#;
        std::fs::write(dir.join("index.json"), index).unwrap();
    }

    #[test]
    fn loads_clips_resamples_and_reports_what_it_could_not_use() {
        let d = tempfile::tempdir().unwrap();
        write_set(d.path());
        let l = SoundSet::load(d.path()).unwrap();
        assert_eq!(l.set.names(), vec!["idle", "swing_1"]);
        let swing = l.set.slot("swing_1").unwrap();
        assert_eq!(swing.takes.len(), 3);
        assert_eq!(swing.event, "e1");
        assert!(!swing.looped && l.set.slot("idle").unwrap().looped);
        // 4800 frames at 48 kHz = 0.1 s = 4410 frames at 44.1 kHz; mono became stereo
        assert_eq!(swing.takes[0].len_frames(), 4410);
        assert!(swing.takes[0].frames.chunks_exact(2).all(|f| f[0] == f[1]));
        let w = l.warnings.join("\n");
        assert!(w.contains("cannot use broken.wav") && w.contains("cannot use missing.wav") && w.contains("odd file name"), "{w}");
        assert!(w.contains("empty_slot: no usable sound"), "{w}");
        let total = l.set.total_seconds();
        assert!((total - 0.17).abs() < 0.01, "{total}");
    }

    #[test]
    fn an_unusable_index_is_an_error_with_the_path() {
        let d = tempfile::tempdir().unwrap();
        assert!(SoundSet::load(d.path()).err().unwrap().contains("index.json"));
        std::fs::write(d.path().join("index.json"), "{").unwrap();
        assert!(SoundSet::load(d.path()).err().unwrap().contains("not a valid sound index"));
        std::fs::write(d.path().join("index.json"), r#"{"format":2,"sounds":[]}"#).unwrap();
        assert!(SoundSet::load(d.path()).err().unwrap().contains("format 2"));
    }

    #[test]
    fn picks_vary_and_never_repeat_the_previous_take() {
        let d = tempfile::tempdir().unwrap();
        write_set(d.path());
        let mut set = SoundSet::load(d.path()).unwrap().set;
        let mut rng = Rng::new(12345);
        let mut prev: Option<Arc<Clip>> = None;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..300 {
            let c = set.pick("swing_1", rng.next()).unwrap();
            if let Some(p) = &prev {
                assert!(!Arc::ptr_eq(p, &c), "the same take twice in a row");
            }
            seen.insert(Arc::as_ptr(&c) as usize);
            prev = Some(c);
        }
        assert_eq!(seen.len(), 3, "all takes get used");
        // a single take is simply repeated
        let a = set.pick("idle", 1).unwrap();
        let b = set.pick("idle", 2).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(set.pick("nope", 1).is_none());
        assert!(set.has("idle") && !set.has("nope"));
    }

    #[test]
    fn the_design_sheet_slots_parse_and_name_real_slots() {
        let slots = sheet_slots();
        assert!(slots.len() >= 10);
        for s in &slots {
            assert!(s.volume > 0.0 && s.volume <= 1.5, "{}: {}", s.id, s.volume);
            assert!(s.sm2_event.starts_with("wpn_"), "{}: {}", s.id, s.sm2_event);
        }
        for need in ["chainsword_swing_1", "chainsword_swing_4", "chainsword_strong", "chainsword_idle", "boltpistol_fire"] {
            assert!(slots.iter().any(|s| s.id == need), "{need} is in the sheet");
        }
        assert!(slots.iter().find(|s| s.id == "chainsword_idle").unwrap().looped);
    }

    #[test]
    fn the_generator_is_not_stuck() {
        let mut r = Rng::new(0);
        let v: Vec<u64> = (0..5).map(|_| r.next()).collect();
        assert!(v.windows(2).all(|w| w[0] != w[1]));
    }
}
