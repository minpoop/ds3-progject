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

    /// Every sound reachable from an event (event -> play actions -> targets -> containers -> sounds).
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
}
