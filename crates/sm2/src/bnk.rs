//! Wwise sound banks (`BKHD`, `DIDX`, `DATA`, `HIRC`, ...). Read-only and tolerant: this walks the chunks and the
//! object table, and understands just enough of the event -> action -> sound chain to find which media files an event
//! plays. The layout follows the public Wwise bank format as implemented by rewwise (MIT OR Apache-2.0) and wwiser.
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// Wwise names events by the FNV-1 (not 1a) 32-bit hash of the lower-cased name.
pub fn fnv1_lower(name: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h = h.wrapping_mul(16777619);
        h ^= b.to_ascii_lowercase() as u32;
    }
    h
}

pub mod kind {
    pub const SETTINGS: u8 = 1;
    pub const SOUND: u8 = 2;
    pub const ACTION: u8 = 3;
    pub const EVENT: u8 = 4;
    pub const RAN_SEQ: u8 = 5;
    pub const SWITCH: u8 = 6;
    pub const ACTOR_MIXER: u8 = 7;
    pub const BUS: u8 = 8;
    pub const LAYER: u8 = 9;
    pub const MUSIC_SEGMENT: u8 = 10;
    pub const MUSIC_TRACK: u8 = 11;
    pub const MUSIC_SWITCH: u8 = 12;
    pub const MUSIC_RAN_SEQ: u8 = 13;
}

/// Wwise action type "Play" (action 4, scope game object). Stop, pause, set-state and every other action do not start a
/// sound, so `resolve_take` and `event_tree` leave them out.
pub const ACTION_PLAY: u16 = 0x0403;

/// How many container levels below an action's target `resolve_take` follows; deeper than that is a damaged bank.
pub const MAX_DEPTH: usize = 32;
/// The most objects one `resolve_take` call looks at (a second guard against banks that fan out without end).
const MAX_VISITS: usize = 20_000;

/// `event_tree` shows at most this many levels and about this many lines, then says how much more there is.
const TREE_MAX_DEPTH: usize = 12;
const TREE_MAX_LINES: usize = 400;
/// `event_tree` stops walking after this many objects, even though it only prints `TREE_MAX_LINES` of them.
const TREE_MAX_VISITS: usize = 20_000;

pub fn kind_name(k: u8) -> &'static str {
    match k {
        1 => "settings",
        2 => "sound",
        3 => "action",
        4 => "event",
        5 => "random/sequence container",
        6 => "switch container",
        7 => "actor-mixer",
        8 => "bus",
        9 => "layer container",
        10 => "music segment",
        11 => "music track",
        12 => "music switch",
        13 => "music playlist",
        14 => "attenuation",
        15 => "dialogue event",
        16 => "fx share set",
        17 => "fx custom",
        18 => "aux bus",
        19 => "lfo",
        20 => "envelope",
        21 => "audio device",
        22 => "time modulator",
        _ => "other",
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub tag: String,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaEntry {
    pub id: u32,
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone)]
pub struct HircObject {
    pub kind: u8,
    pub id: u32,
    /// the object's bytes after its id, as a range of the bank file
    pub body: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundInfo {
    pub plugin: u32,
    /// 0 = inside the bank, 1 = streamed from a separate .wem, 2 = prefetched + streamed
    pub stream_type: u8,
    pub media_id: u32,
    pub in_memory_size: u32,
}

#[derive(Debug, Clone)]
pub struct Bank {
    pub data: Vec<u8>,
    pub version: u32,
    pub bank_id: u32,
    pub chunks: Vec<Chunk>,
    pub media: Vec<MediaEntry>,
    media_data: Range<usize>,
    pub objects: Vec<HircObject>,
    by_id: HashMap<u32, usize>,
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

impl Bank {
    pub fn parse(data: Vec<u8>) -> Result<Bank> {
        if data.len() < 8 || &data[..4] != b"BKHD" {
            bail!("not a Wwise bank (no BKHD)");
        }
        let mut chunks = Vec::new();
        let mut at = 0usize;
        while at + 8 <= data.len() {
            let tag = String::from_utf8_lossy(&data[at..at + 4]).to_string();
            let len = u32le(&data, at + 4).unwrap() as usize;
            let start = at + 8;
            let end = start.checked_add(len).filter(|&e| e <= data.len());
            let Some(end) = end else {
                bail!("chunk {tag} at {at} runs past the end of the file ({len} bytes)");
            };
            chunks.push(Chunk { tag, range: start..end });
            at = end;
        }
        let bkhd = chunks[0].range.clone();
        let version = u32le(&data, bkhd.start).unwrap_or(0);
        let bank_id = u32le(&data, bkhd.start + 4).unwrap_or(0);

        let mut media = Vec::new();
        let mut media_data = 0..0;
        let mut objects = Vec::new();
        for c in &chunks {
            match c.tag.as_str() {
                "DIDX" => {
                    for e in data[c.range.clone()].chunks_exact(12) {
                        media.push(MediaEntry {
                            id: u32le(e, 0).unwrap(),
                            offset: u32le(e, 4).unwrap(),
                            size: u32le(e, 8).unwrap(),
                        });
                    }
                }
                "DATA" => media_data = c.range.clone(),
                "HIRC" => {
                    let count = u32le(&data, c.range.start).unwrap_or(0) as usize;
                    let mut p = c.range.start + 4;
                    for _ in 0..count {
                        if p + 5 > c.range.end {
                            break;
                        }
                        let k = data[p];
                        let len = u32le(&data, p + 1).unwrap() as usize;
                        let body_start = p + 5;
                        let body_end = body_start + len;
                        if len < 4 || body_end > c.range.end {
                            break;
                        }
                        objects.push(HircObject { kind: k, id: u32le(&data, body_start).unwrap(), body: body_start + 4..body_end });
                        p = body_end;
                    }
                }
                _ => {}
            }
        }
        let by_id = objects.iter().enumerate().map(|(i, o)| (o.id, i)).collect();
        Ok(Bank { data, version, bank_id, chunks, media, media_data, objects, by_id })
    }

    pub fn object(&self, id: u32) -> Option<&HircObject> {
        self.by_id.get(&id).map(|&i| &self.objects[i])
    }

    pub fn body(&self, o: &HircObject) -> &[u8] {
        &self.data[o.body.clone()]
    }

    pub fn kind_counts(&self) -> Vec<(u8, usize)> {
        let mut m: HashMap<u8, usize> = HashMap::new();
        for o in &self.objects {
            *m.entry(o.kind).or_default() += 1;
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort();
        v
    }

    /// The bytes of an embedded media file (a wem stored inside the bank), if the bank has it.
    pub fn embedded_media(&self, id: u32) -> Option<&[u8]> {
        let e = self.media.iter().find(|e| e.id == id)?;
        let start = self.media_data.start.checked_add(e.offset as usize)?;
        self.data.get(start..start.checked_add(e.size as usize)?)
    }

    pub fn event_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.objects.iter().filter(|o| o.kind == kind::EVENT).map(|o| o.id)
    }

    /// The actions an event runs.
    pub fn event_actions(&self, event: &HircObject) -> Vec<u32> {
        let b = self.body(event);
        // the action count is a plain u8 up to bank version 122 (u32 before that) and a Wwise var-int after
        let (count, mut p) = if self.version <= 122 {
            (u32le(b, 0).unwrap_or(0) as usize, 4)
        } else {
            let mut v = 0usize;
            let mut p = 0;
            while let Some(&byte) = b.get(p) {
                p += 1;
                v = (v << 7) | (byte & 0x7F) as usize;
                if byte & 0x80 == 0 {
                    break;
                }
            }
            (v, p)
        };
        let mut out = Vec::new();
        for _ in 0..count.min(4096) {
            match u32le(b, p) {
                Some(id) => out.push(id),
                None => break,
            }
            p += 4;
        }
        out
    }

    /// `(action type, target object id)` of an action object.
    pub fn action(&self, o: &HircObject) -> Option<(u16, u32)> {
        let b = self.body(o);
        Some((u16le(b, 0)?, u32le(b, 2)?))
    }

    pub fn sound(&self, o: &HircObject) -> Option<SoundInfo> {
        let b = self.body(o);
        Some(SoundInfo { plugin: u32le(b, 0)?, stream_type: *b.get(4)?, media_id: u32le(b, 5)?, in_memory_size: u32le(b, 9)? })
    }

    /// Children of a container-like object, found by looking for `u32 n` followed by `n` ids of objects that exist
    /// in this bank (the position is behind a long, version-dependent parameter block, so it is located by shape).
    pub fn children(&self, o: &HircObject) -> Vec<u32> {
        let b = self.body(o);
        let mut best: Option<(usize, usize)> = None; // (count, position)
        let mut p = 0;
        while p + 8 <= b.len() {
            if let Some(n) = u32le(b, p) {
                let n = n as usize;
                if (1..=2048).contains(&n) && p + 4 + 4 * n <= b.len() {
                    let ok = (0..n).all(|i| {
                        let id = u32le(b, p + 4 + 4 * i).unwrap();
                        id != o.id
                            && self.object(id).is_some_and(|c| {
                                matches!(
                                    c.kind,
                                    kind::SOUND | kind::RAN_SEQ | kind::SWITCH | kind::ACTOR_MIXER | kind::LAYER | kind::MUSIC_SEGMENT | kind::MUSIC_TRACK | kind::MUSIC_SWITCH | kind::MUSIC_RAN_SEQ
                                )
                            })
                    });
                    if ok && best.is_none_or(|(bn, _)| n > bn) {
                        best = Some((n, p));
                    }
                }
            }
            p += 1;
        }
        match best {
            Some((n, p)) => (0..n).map(|i| u32le(b, p + 4 + 4 * i).unwrap()).collect(),
            None => Vec::new(),
        }
    }

    /// Every sound reachable from an event (event -> actions -> targets -> containers -> sounds), all of them at once.
    /// That is the whole family of the event, not what one playback plays: see `resolve_take` for that.
    pub fn event_sounds(&self, event_id: u32) -> Vec<SoundInfo> {
        let mut out = Vec::new();
        let Some(ev) = self.object(event_id) else { return out };
        let mut seen = HashSet::new();
        let mut stack: Vec<u32> = Vec::new();
        for a in self.event_actions(ev) {
            if let Some(ao) = self.object(a) {
                if let Some((_, target)) = self.action(ao) {
                    stack.push(target);
                }
            }
        }
        while let Some(id) = stack.pop() {
            if !seen.insert(id) || seen.len() > 5000 {
                continue;
            }
            let Some(o) = self.object(id) else { continue };
            match o.kind {
                kind::SOUND => {
                    if let Some(s) = self.sound(o) {
                        out.push(s);
                    }
                }
                kind::RAN_SEQ | kind::SWITCH | kind::ACTOR_MIXER | kind::LAYER => stack.extend(self.children(o)),
                _ => {}
            }
        }
        out.sort_by_key(|s| s.media_id);
        out.dedup();
        out
    }

    /// Map of media id -> events that reach it (only events whose sounds could be followed).
    pub fn media_to_events(&self) -> HashMap<u32, Vec<u32>> {
        let mut m: HashMap<u32, Vec<u32>> = HashMap::new();
        for ev in self.event_ids().collect::<Vec<_>>() {
            for s in self.event_sounds(ev) {
                m.entry(s.media_id).or_default().push(ev);
            }
        }
        m
    }

    /// The sounds that play TOGETHER when the event is played once. Only play actions ([`ACTION_PLAY`]) count. Their
    /// target is followed down: a sound plays itself; a random/sequence container plays ONE child, chosen with
    /// `rng(n)` (an index below `n`; the caller supplies the generator, so the same seed gives the same take); a switch
    /// container also plays one child, chosen the same way (its switch groups are not read, so this is a guess); a
    /// layer container and an actor-mixer play ALL their children; any other object plays nothing. Cycles are cut and
    /// the walk gives up after [`MAX_DEPTH`] levels, so a damaged bank cannot send it round forever. An event that is
    /// not in the bank gives an empty list.
    pub fn resolve_take(&self, event_id: u32, rng: &mut impl FnMut(usize) -> usize) -> Vec<SoundInfo> {
        let mut out = Vec::new();
        let Some(ev) = self.object(event_id).filter(|o| o.kind == kind::EVENT) else { return out };
        let mut visits = 0;
        for a in self.event_actions(ev) {
            let Some((ty, target)) = self.object(a).filter(|o| o.kind == kind::ACTION).and_then(|o| self.action(o)) else { continue };
            if ty == ACTION_PLAY {
                self.resolve_into(target, &mut Vec::new(), &mut visits, &mut *rng, &mut out);
            }
        }
        out
    }

    /// One step of `resolve_take`. `path` holds the containers above `id` (it finds cycles and measures the depth).
    fn resolve_into(&self, id: u32, path: &mut Vec<u32>, visits: &mut usize, rng: &mut dyn FnMut(usize) -> usize, out: &mut Vec<SoundInfo>) {
        if path.len() > MAX_DEPTH || path.contains(&id) || *visits >= MAX_VISITS {
            return;
        }
        *visits += 1;
        let Some(o) = self.object(id) else { return };
        match o.kind {
            kind::SOUND => out.extend(self.sound(o)),
            kind::RAN_SEQ | kind::SWITCH => {
                let kids = self.children(o);
                if kids.is_empty() {
                    return;
                }
                // a generator that answers outside 0..n must not be able to crash the walk
                let pick = rng(kids.len()) % kids.len();
                path.push(id);
                self.resolve_into(kids[pick], path, visits, rng, out);
                path.pop();
            }
            kind::LAYER | kind::ACTOR_MIXER => {
                path.push(id);
                for c in self.children(o) {
                    self.resolve_into(c, path, visits, rng, out);
                }
                path.pop();
            }
            _ => {}
        }
    }

    /// An indented text tree of what the event reaches, for reports: one line per object (its kind, id and, for a
    /// sound, the media id and stream type), and a note on how each container picks its children. Only play actions
    /// are followed; the other actions are listed as ignored. The tree stops at 12 levels and about 400 lines and then
    /// says how many more lines there were.
    pub fn event_tree(&self, event_id: u32) -> String {
        let Some(ev) = self.object(event_id).filter(|o| o.kind == kind::EVENT) else {
            return format!("event {event_id} ({event_id:#010x}) is not in this bank");
        };
        let mut t = TreeText::default();
        let actions = self.event_actions(ev);
        t.push(0, format!("event {} ({:#010x}): {} action{}", ev.id, ev.id, actions.len(), if actions.len() == 1 { "" } else { "s" }));
        for a in actions {
            match self.object(a).filter(|o| o.kind == kind::ACTION).and_then(|o| self.action(o)) {
                None => t.push(1, format!("action {a}: not in this bank")),
                Some((ACTION_PLAY, target)) => {
                    t.push(1, format!("action {a}: play"));
                    self.tree_node(target, 2, &mut Vec::new(), &mut t);
                }
                Some((ty, _)) => t.push(1, format!("action {a}: type {ty:#06x}, not a play action, ignored")),
            }
        }
        t.finish()
    }

    fn tree_node(&self, id: u32, depth: usize, path: &mut Vec<u32>, t: &mut TreeText) {
        if t.visits >= TREE_MAX_VISITS {
            t.cut_short = true;
            return;
        }
        t.visits += 1;
        let Some(o) = self.object(id) else {
            t.push(depth, format!("object {id}: not in this bank"));
            return;
        };
        let name = kind_name(o.kind);
        if path.contains(&id) {
            t.push(depth, format!("{name} {id}: already above, not followed again"));
            return;
        }
        match o.kind {
            kind::SOUND => match self.sound(o) {
                Some(s) => t.push(depth, format!("sound {id}: media {}, stream type {} ({})", s.media_id, s.stream_type, stream_name(s.stream_type))),
                None => t.push(depth, format!("sound {id}: cut off, cannot be read")),
            },
            kind::RAN_SEQ | kind::SWITCH | kind::LAYER | kind::ACTOR_MIXER => {
                let kids = self.children(o);
                let how = match (o.kind, kids.len()) {
                    (_, 0) => "no children found".to_string(),
                    (kind::RAN_SEQ, n) => format!("plays one of its {n} children, picked at random"),
                    (kind::SWITCH, n) => format!("plays one of its {n} children (a guess: the switch settings are not read)"),
                    (_, n) => format!("plays all of its {n} children"),
                };
                t.push(depth, format!("{name} {id}: {how}"));
                if depth >= TREE_MAX_DEPTH {
                    if !kids.is_empty() {
                        t.push(depth + 1, "... (levels below this are not shown)".to_string());
                    }
                    return;
                }
                path.push(id);
                for c in kids {
                    self.tree_node(c, depth + 1, path, t);
                }
                path.pop();
            }
            _ => t.push(depth, format!("{name} {id}: not played by this tool")),
        }
    }
}

fn stream_name(stream_type: u8) -> &'static str {
    match stream_type {
        0 => "stored in the bank",
        1 => "streamed from a separate file",
        2 => "prefetched in the bank, streamed from a separate file",
        _ => "unknown",
    }
}

/// The lines of an `event_tree` while it is being built: only the first `TREE_MAX_LINES` are kept, the rest is counted.
#[derive(Default)]
struct TreeText {
    lines: Vec<String>,
    total: usize,
    visits: usize,
    cut_short: bool,
}

impl TreeText {
    fn push(&mut self, depth: usize, text: String) {
        self.total += 1;
        if self.lines.len() < TREE_MAX_LINES {
            self.lines.push(format!("{}{text}", "  ".repeat(depth)));
        }
    }

    fn finish(mut self) -> String {
        let hidden = self.total - self.lines.len();
        if hidden > 0 {
            self.lines.push(format!("... {hidden}{} more", if self.cut_short { "+" } else { "" }));
        }
        self.lines.join("\n")
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testbank {
    //! Builds small synthetic banks so the parser can be tested without any game file.
    pub struct Builder {
        pub version: u32,
        objs: Vec<(u8, Vec<u8>)>,
        media: Vec<(u32, Vec<u8>)>,
    }
    fn le(v: u32) -> [u8; 4] { v.to_le_bytes() }
    impl Builder {
        pub fn new(version: u32) -> Self { Builder { version, objs: vec![], media: vec![] } }
        pub fn sound(&mut self, id: u32, media: u32, stream: u8) -> &mut Self {
            let mut b = le(id).to_vec();
            b.extend(le(0x00040001)); // plugin: vorbis codec
            b.push(stream);
            b.extend(le(media));
            b.extend(le(0)); // in-memory size
            b.push(0); // source bits
            b.extend([0u8; 12]); // start of the (unparsed) node parameters
            self.objs.push((2, b));
            self
        }
        pub fn container(&mut self, kind: u8, id: u32, children: &[u32]) -> &mut Self {
            let mut b = le(id).to_vec();
            b.extend([0u8, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0x55, 0xAA, 3, 0]); // stand-in for node parameters
            b.extend(le(children.len() as u32));
            for c in children {
                b.extend(le(*c));
            }
            b.extend([1u8, 0]); // trailing data
            self.objs.push((kind, b));
            self
        }
        pub fn action(&mut self, id: u32, ty: u16, target: u32) -> &mut Self {
            let mut b = le(id).to_vec();
            b.extend(ty.to_le_bytes());
            b.extend(le(target));
            b.extend([0u8, 0, 0]);
            self.objs.push((3, b));
            self
        }
        pub fn event(&mut self, id: u32, actions: &[u32]) -> &mut Self {
            let mut b = le(id).to_vec();
            if self.version <= 122 {
                b.extend(le(actions.len() as u32));
            } else {
                assert!(actions.len() < 128);
                b.push(actions.len() as u8);
            }
            for a in actions {
                b.extend(le(*a));
            }
            self.objs.push((4, b));
            self
        }
        pub fn media(&mut self, id: u32, bytes: &[u8]) -> &mut Self {
            self.media.push((id, bytes.to_vec()));
            self
        }
        pub fn build(&self) -> Vec<u8> {
            let mut out = Vec::new();
            let mut chunk = |tag: &[u8; 4], body: &[u8]| {
                out.extend_from_slice(tag);
                out.extend(le(body.len() as u32));
                out.extend_from_slice(body);
            };
            let mut bkhd = le(self.version).to_vec();
            bkhd.extend(le(0xB4A4)); // bank id
            bkhd.extend([0u8; 16]);
            chunk(b"BKHD", &bkhd);
            if !self.media.is_empty() {
                let mut didx = Vec::new();
                let mut data = Vec::new();
                for (id, bytes) in &self.media {
                    didx.extend(le(*id));
                    didx.extend(le(data.len() as u32));
                    didx.extend(le(bytes.len() as u32));
                    data.extend_from_slice(bytes);
                    while data.len() % 16 != 0 {
                        data.push(0);
                    }
                }
                chunk(b"DIDX", &didx);
                chunk(b"DATA", &data);
            }
            let mut hirc = le(self.objs.len() as u32).to_vec();
            for (k, b) in &self.objs {
                hirc.push(*k);
                hirc.extend(le(b.len() as u32));
                hirc.extend_from_slice(b);
            }
            chunk(b"HIRC", &hirc);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testbank::Builder;
    use super::*;

    #[test]
    fn fnv1_matches_the_wwise_convention() {
        // FNV-1 (multiply, then xor) of "play_x" over lower-cased bytes, computed independently below
        let mut h: u64 = 2166136261;
        for b in b"play_x" {
            h = (h * 16777619) & 0xFFFF_FFFF;
            h ^= *b as u64;
        }
        assert_eq!(fnv1_lower("Play_X"), h as u32);
        assert_eq!(fnv1_lower(""), 2166136261);
        assert_ne!(fnv1_lower("a"), fnv1_lower("b"));
    }

    #[test]
    fn follows_event_action_container_sound_chains() {
        let mut b = Builder::new(150);
        b.sound(100, 5001, 1).sound(101, 5002, 1).sound(102, 5003, 0);
        b.container(kind::RAN_SEQ, 200, &[100, 101]);
        b.container(kind::ACTOR_MIXER, 201, &[200, 102]);
        b.action(300, 0x0403, 201);
        b.action(301, 0x0403, 102);
        b.event(400, &[300]).event(401, &[301]);
        b.media(5003, b"embedded wem bytes");
        let bank = Bank::parse(b.build()).unwrap();
        assert_eq!((bank.version, bank.bank_id), (150, 0xB4A4));
        assert_eq!(bank.objects.len(), 9);
        assert_eq!(bank.kind_counts().iter().find(|(k, _)| *k == kind::SOUND).unwrap().1, 3);

        let all: Vec<u32> = bank.event_sounds(400).iter().map(|s| s.media_id).collect();
        assert_eq!(all, vec![5001, 5002, 5003]);
        let one = bank.event_sounds(401);
        assert_eq!(one.len(), 1);
        assert_eq!((one[0].media_id, one[0].stream_type), (5003, 0));
        assert!(bank.event_sounds(999).is_empty());

        assert_eq!(bank.embedded_media(5003).unwrap(), b"embedded wem bytes");
        assert!(bank.embedded_media(5001).is_none());
        let rev = bank.media_to_events();
        assert_eq!(rev[&5003], vec![400, 401]);
        assert_eq!(rev[&5001], vec![400]);
    }

    #[test]
    fn old_event_layout_uses_a_u32_count() {
        let mut b = Builder::new(88);
        b.sound(1, 77, 0).action(2, 0x0403, 1).event(3, &[2]);
        let bank = Bank::parse(b.build()).unwrap();
        assert_eq!(bank.event_sounds(3).iter().map(|s| s.media_id).collect::<Vec<_>>(), vec![77]);
    }

    #[test]
    fn rejects_non_banks_and_truncated_chunks() {
        assert!(Bank::parse(b"RIFF....".to_vec()).is_err());
        let mut data = Builder::new(150).build();
        data.truncate(data.len() - 3);
        assert!(Bank::parse(data).is_err());
    }

    #[test]
    fn does_not_loop_on_cyclic_containers() {
        let mut b = Builder::new(150);
        b.container(kind::RAN_SEQ, 10, &[11]).container(kind::RAN_SEQ, 11, &[10]).sound(12, 1, 1);
        b.action(20, 0x0403, 10).event(30, &[20]);
        let bank = Bank::parse(b.build()).unwrap();
        assert!(bank.event_sounds(30).is_empty());
    }

    // ---- resolve_take / event_tree

    const EVENT: u32 = 1000;

    /// A bank holding what `f` adds, plus the event 1000 that plays every object of `targets` through a play action.
    fn play_bank(targets: &[u32], f: impl FnOnce(&mut Builder)) -> Bank {
        let mut b = Builder::new(150);
        f(&mut b);
        let actions: Vec<u32> = (0..targets.len() as u32).map(|i| 900 + i).collect();
        for (a, t) in actions.iter().zip(targets) {
            b.action(*a, ACTION_PLAY, *t);
        }
        b.event(EVENT, &actions);
        Bank::parse(b.build()).unwrap()
    }

    fn media(sounds: &[SoundInfo]) -> Vec<u32> {
        sounds.iter().map(|s| s.media_id).collect()
    }

    /// `depth` actor-mixers inside each other (ids 100..), the innermost holding sound 1 (media 11). The sound sits
    /// `depth` levels below the action's target.
    fn nested(depth: usize) -> Bank {
        play_bank(&[100], |b| {
            b.sound(1, 11, 1);
            for i in 0..depth {
                let inner = if i + 1 == depth { 1 } else { 101 + i as u32 };
                b.container(kind::ACTOR_MIXER, 100 + i as u32, &[inner]);
            }
        })
    }

    #[test]
    fn a_random_container_plays_exactly_one_child() {
        let bank = play_bank(&[10], |b| {
            b.sound(1, 11, 1).sound(2, 12, 1).sound(3, 13, 1).container(kind::RAN_SEQ, 10, &[1, 2, 3]);
        });
        let mut asked = Vec::new();
        for want in 0..3usize {
            let take = bank.resolve_take(EVENT, &mut |n| {
                asked.push(n);
                want
            });
            assert_eq!(media(&take), vec![11 + want as u32], "rng answer {want}");
        }
        assert_eq!(asked, vec![3, 3, 3], "one question per take, about the three children");
        // a generator that answers outside 0..n still gets a child (7 % 3 = 1)
        assert_eq!(media(&bank.resolve_take(EVENT, &mut |_| 7)), vec![12]);
    }

    #[test]
    fn a_switch_container_also_plays_one_child() {
        let bank = play_bank(&[10], |b| {
            b.sound(1, 11, 1).sound(2, 12, 1).container(kind::SWITCH, 10, &[1, 2]);
        });
        assert_eq!(media(&bank.resolve_take(EVENT, &mut |_| 0)), vec![11]);
        assert_eq!(media(&bank.resolve_take(EVENT, &mut |_| 1)), vec![12]);
    }

    #[test]
    fn a_layer_and_an_actor_mixer_play_all_their_children() {
        let bank = play_bank(&[10, 11], |b| {
            b.sound(1, 11, 1).sound(2, 12, 1).sound(3, 13, 1).sound(4, 14, 0);
            b.container(kind::LAYER, 10, &[1, 2, 3]).container(kind::ACTOR_MIXER, 11, &[4]);
        });
        let take = bank.resolve_take(EVENT, &mut |_| panic!("nothing is chosen here"));
        assert_eq!(media(&take), vec![11, 12, 13, 14], "two play actions give the sounds of both, in order");
        assert_eq!(take[3].stream_type, 0);
    }

    #[test]
    fn random_containers_inside_a_layer_each_pick_one() {
        let bank = play_bank(&[30], |b| {
            b.sound(1, 11, 1).sound(2, 12, 1).sound(3, 13, 1).sound(4, 14, 1).sound(5, 15, 1);
            b.container(kind::RAN_SEQ, 10, &[1, 2]).container(kind::RAN_SEQ, 20, &[3, 4]).container(kind::LAYER, 30, &[10, 20, 5]);
        });
        let mut answers = [1usize, 0].into_iter();
        let take = bank.resolve_take(EVENT, &mut |n| {
            assert_eq!(n, 2);
            answers.next().expect("only two containers choose")
        });
        assert_eq!(media(&take), vec![12, 13, 15], "second child of the first, first of the second, plus the plain sound");
        assert_eq!(media(&bank.resolve_take(EVENT, &mut |_| 1)), vec![12, 14, 15]);
    }

    #[test]
    fn only_play_actions_start_sounds() {
        let mut b = Builder::new(150);
        b.sound(1, 11, 1).sound(2, 12, 1).sound(3, 13, 1).sound(4, 14, 1);
        // play, stop, pause, and an action of some other kind
        b.action(900, ACTION_PLAY, 1).action(901, 0x0102, 2).action(902, 0x0202, 3).action(903, 0x1203, 4);
        b.event(EVENT, &[900, 901, 902, 903]);
        let bank = Bank::parse(b.build()).unwrap();
        assert_eq!(media(&bank.resolve_take(EVENT, &mut |_| 0)), vec![11]);
        assert_eq!(bank.event_sounds(EVENT).len(), 4, "the older union over every action is unchanged");
    }

    #[test]
    fn things_that_cannot_play_give_nothing() {
        let mut b = Builder::new(150);
        b.sound(1, 11, 1).container(kind::MUSIC_SEGMENT, 10, &[1]).container(kind::BUS, 11, &[1]);
        b.action(900, ACTION_PLAY, 10).action(901, ACTION_PLAY, 11).action(902, ACTION_PLAY, 424242);
        b.event(EVENT, &[900, 901, 902]).event(1001, &[777]); // 777 is no action
        let bank = Bank::parse(b.build()).unwrap();
        assert!(bank.resolve_take(EVENT, &mut |_| 0).is_empty());
        assert!(bank.resolve_take(1001, &mut |_| 0).is_empty());
        assert!(bank.resolve_take(999_999, &mut |_| 0).is_empty(), "not in the bank");
        assert!(bank.resolve_take(1, &mut |_| 0).is_empty(), "a sound is no event");
    }

    #[test]
    fn cycles_and_endless_nesting_end() {
        // two layers that contain each other and one sound each: every sound plays once, the loop is cut
        let bank = play_bank(&[10], |b| {
            b.sound(1, 11, 1).sound(2, 12, 1).container(kind::LAYER, 10, &[11, 1]).container(kind::LAYER, 11, &[10, 2]);
        });
        assert_eq!(media(&bank.resolve_take(EVENT, &mut |_| 0)), vec![12, 11]);
        // random containers in a ring never reach a sound
        let ring = play_bank(&[10], |b| {
            b.container(kind::RAN_SEQ, 10, &[11]).container(kind::RAN_SEQ, 11, &[10]).sound(12, 1, 1);
        });
        assert!(ring.resolve_take(EVENT, &mut |_| 0).is_empty());
        // the sound MAX_DEPTH levels down is still found, one more level is too deep
        assert_eq!(media(&nested(MAX_DEPTH).resolve_take(EVENT, &mut |_| 0)), vec![11]);
        assert!(nested(MAX_DEPTH + 1).resolve_take(EVENT, &mut |_| 0).is_empty());
        assert!(nested(200).resolve_take(EVENT, &mut |_| 0).is_empty());
    }

    #[test]
    fn the_tree_shows_kinds_ids_media_and_stream_types() {
        let bank = play_bank(&[20], |b| {
            b.sound(1, 5001, 1).sound(2, 5002, 2).sound(3, 5003, 0);
            b.container(kind::RAN_SEQ, 10, &[1, 2]).container(kind::LAYER, 20, &[10, 3]);
            b.action(901, 0x0102, 3); // a stop action next to the play action
            b.event(1001, &[900, 901]);
        });
        let lines: Vec<String> = bank.event_tree(EVENT).lines().map(str::to_string).collect();
        assert_eq!(
            lines,
            [
                "event 1000 (0x000003e8): 1 action",
                "  action 900: play",
                "    layer container 20: plays all of its 2 children",
                "      random/sequence container 10: plays one of its 2 children, picked at random",
                "        sound 1: media 5001, stream type 1 (streamed from a separate file)",
                "        sound 2: media 5002, stream type 2 (prefetched in the bank, streamed from a separate file)",
                "      sound 3: media 5003, stream type 0 (stored in the bank)",
            ]
        );
        let ignored = bank.event_tree(1001);
        assert!(ignored.contains("action 901: type 0x0102, not a play action, ignored"), "{ignored}");
        assert_eq!(bank.event_tree(5), "event 5 (0x00000005) is not in this bank");
    }

    #[test]
    fn the_tree_marks_loops_switches_and_things_it_does_not_play() {
        let bank = play_bank(&[10, 12], |b| {
            b.sound(1, 11, 1).container(kind::LAYER, 10, &[11, 1]).container(kind::LAYER, 11, &[10, 1]).container(kind::SWITCH, 12, &[1]);
            b.container(kind::MUSIC_SEGMENT, 13, &[1]);
            b.action(950, ACTION_PLAY, 13).action(951, ACTION_PLAY, 31337).event(1001, &[950, 951]);
        });
        let tree = bank.event_tree(EVENT);
        assert!(tree.contains("layer container 10: already above, not followed again"), "{tree}");
        assert!(tree.contains("switch container 12: plays one of its 1 children (a guess"), "{tree}");
        let other = bank.event_tree(1001);
        assert!(other.contains("music segment 13: not played by this tool"), "{other}");
        assert!(other.contains("object 31337: not in this bank"), "{other}");
    }

    /// xorshift64: enough randomness to wire up nonsense banks
    struct Xorshift(u64);

    impl Xorshift {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    #[test]
    fn banks_with_nonsense_wiring_never_hang_or_panic() {
        let mut x = Xorshift(0x1234_5678_9ABC_DEF1);
        for round in 0..60u64 {
            let mut b = Builder::new(150);
            let sounds = 1 + x.below(8) as u32;
            for id in 1..=sounds {
                b.sound(id, 500 + id, x.below(3) as u8);
            }
            // containers of any kind whose children are any objects at all: shared children, loops, deep chains
            let containers = 1 + x.below(12) as u32;
            let all: Vec<u32> = (1..=sounds).chain(100..100 + containers).collect();
            for c in 0..containers {
                let k = [kind::RAN_SEQ, kind::SWITCH, kind::LAYER, kind::ACTOR_MIXER, kind::MUSIC_SEGMENT][x.below(5)];
                let kids: Vec<u32> = (0..1 + x.below(4)).map(|_| all[x.below(all.len())]).collect();
                b.container(k, 100 + c, &kids);
            }
            let actions: Vec<u32> = (900..900 + 1 + x.below(4) as u32).collect();
            for a in &actions {
                b.action(*a, if x.below(4) == 0 { 0x0102 } else { ACTION_PLAY }, all[x.below(all.len())]);
            }
            b.event(EVENT, &actions);
            let bank = Bank::parse(b.build()).unwrap();

            let family: HashSet<u32> = bank.event_sounds(EVENT).iter().map(|s| s.media_id).collect();
            for seed in 0..20u64 {
                let draw = |bank: &Bank| {
                    let mut r = Xorshift(0x9E37_79B9_7F4A_7C15 ^ (seed << 8) ^ round);
                    media(&bank.resolve_take(EVENT, &mut |n| r.below(n)))
                };
                let take = draw(&bank);
                assert!(take.iter().all(|m| family.contains(m)), "round {round}: a take plays only sounds of the event's family");
                assert_eq!(take, draw(&bank), "the same generator gives the same take");
            }
            assert!(bank.event_tree(EVENT).lines().count() <= TREE_MAX_LINES + 1);
        }
    }

    #[test]
    fn the_tree_is_capped_in_lines_and_depth() {
        // 600 sounds in one layer: event + action + layer + 600 = 603 lines, 400 are kept
        let bank = play_bank(&[5000], |b| {
            let ids: Vec<u32> = (1..=600).collect();
            for id in &ids {
                b.sound(*id, 10_000 + *id, 1);
            }
            b.container(kind::LAYER, 5000, &ids);
        });
        let tree = bank.event_tree(EVENT);
        assert_eq!(tree.lines().count(), 401);
        assert_eq!(tree.lines().last(), Some("... 203 more"));
        assert!(tree.contains("media 10001,") && !tree.contains("media 10600,"));

        // 20 nested mixers: the tree goes 12 levels down and says that it stops
        let deep = nested(20).event_tree(EVENT);
        assert_eq!(deep.lines().count(), 14, "{deep}");
        assert!(deep.lines().last().unwrap().trim_start().starts_with("... (levels below this are not shown)"), "{deep}");
        assert!(!deep.contains("sound 1:"), "{deep}");
    }
}
