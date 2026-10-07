//! A strict reader for the Wwise bank objects that decide what an event sounds like: sounds, random/sequence containers,
//! switch containers, layer containers, actor-mixers. "Strict" means a parsed object must use up its bytes exactly; the
//! caller learns how many objects of a real bank were understood, and a wrong guess about the layout shows up as a
//! failure count instead of as wrong audio.
//!
//! The field layout follows the public description of the Wwise soundbank format as implemented by rewwise
//! (vswarte/rewwise, MIT OR Apache-2.0) for bank versions around 145-150, which is what Space Marine 2 uses (150).
use crate::bnk::{kind, Bank, HircObject};
use anyhow::{bail, ensure, Result};
use std::collections::HashMap;

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Reader { b, p: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(self.p + n <= self.b.len(), "ran past the end ({} bytes needed at offset {}, object has {})", n, self.p, self.b.len());
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }
    fn left(&self) -> usize {
        self.b.len() - self.p
    }
    /// A count that must be plausible before it is used to size anything.
    fn count(&mut self, n: usize, max: usize, what: &str) -> Result<usize> {
        ensure!(n <= max, "implausible {what} count {n}");
        Ok(n)
    }
}

/// The properties of a node that change how loud, how high and how late it plays.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Props {
    pub volume_db: f32,
    pub make_up_db: f32,
    pub pitch_cents: f32,
    pub delay_ms: i32,
    /// random offset range added to the volume each time it plays (dB)
    pub volume_range: Option<(f32, f32)>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Base {
    pub parent: u32,
    pub props: Props,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SoundSource {
    pub plugin: u32,
    /// 0 = inside the bank, 1 = prefetch + streamed, 2 = streamed
    pub stream_type: u8,
    pub media_id: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Sound { base: Base, source: SoundSource },
    RanSeq { base: Base, sequence: bool, avoid_repeat: u16, children: Vec<u32>, playlist: Vec<(u32, i32)> },
    Switch { base: Base, default_switch: u32, children: Vec<u32>, packages: Vec<(u32, Vec<u32>)> },
    Layer { base: Base, children: Vec<u32> },
    ActorMixer { base: Base, children: Vec<u32> },
}

impl Node {
    pub fn base(&self) -> &Base {
        match self {
            Node::Sound { base, .. } | Node::RanSeq { base, .. } | Node::Switch { base, .. } | Node::Layer { base, .. } | Node::ActorMixer { base, .. } => base,
        }
    }
}

// ------------------------------------------------------------------------------------------------ node base params

fn fx_params(r: &mut Reader) -> Result<()> {
    let _override_parent_fx = r.u8()?;
    let n = r.u8()? as usize;
    if n > 0 {
        let _bypass = r.u8()?;
        r.count(n, 8, "effect")?;
        for _ in 0..n {
            r.skip(1 + 4 + 1 + 1)?; // index, id, share set, rendered
        }
    }
    Ok(())
}

fn prop_bundle(r: &mut Reader, props: &mut Props) -> Result<()> {
    let n = r.u8()? as usize;
    let mut ids = Vec::with_capacity(n);
    for _ in 0..n {
        ids.push(r.u8()?);
    }
    for id in ids {
        match id {
            0x00 => props.volume_db = r.f32()?,
            0x02 => props.pitch_cents = r.f32()?,
            0x06 => props.make_up_db = r.f32()?,
            0x0F => props.delay_ms = r.i32()?,
            _ => r.skip(4)?, // every other property is one 4-byte value
        }
    }
    let m = r.u8()? as usize;
    for _ in 0..m {
        let id = r.u8()?;
        let (min, max) = (r.f32()?, r.f32()?);
        if id == 0x00 {
            props.volume_range = Some((min, max));
        }
    }
    Ok(())
}

fn positioning(r: &mut Reader) -> Result<()> {
    // first byte, most significant bit first: 1 unknown, 2 bits 3D position type, 3 bits speaker panning,
    // 1 listener-relative routing, 1 override parent
    let b0 = r.u8()?;
    let position_type = (b0 >> 5) & 0b11;
    let listener_relative = (b0 >> 1) & 1 != 0;
    if listener_relative {
        r.skip(1)?; // diffraction / attenuation / listener-orientation flags and the spatialization mode
    }
    if position_type != 0 {
        // emitter with automation / listener with automation: a path
        let _path_mode = r.u8()?;
        let _transition = r.i32()?;
        let vertices = r.u32()? as usize;
        r.count(vertices, 4096, "path vertex")?;
        r.skip(vertices * 16)?;
        let items = r.u32()? as usize;
        r.count(items, 4096, "path list item")?;
        r.skip(items * 8)?;
        r.skip(items * 12)?;
    }
    Ok(())
}

fn aux(r: &mut Reader) -> Result<()> {
    let b = r.u8()?;
    let has_aux = (b >> 3) & 1 != 0; // bits, most significant first: 3 unknown, override reflections, has aux, override user sends, 2 unknown
    if has_aux {
        r.skip(16)?;
    }
    r.skip(4)?; // reflections aux bus
    Ok(())
}

fn adv_settings(r: &mut Reader) -> Result<()> {
    // flags, virtual queue behaviour, max instances (u16), below-threshold behaviour, more flags
    r.skip(1 + 1 + 2 + 1 + 1)
}

fn state_chunk(r: &mut Reader) -> Result<()> {
    let props = r.u8()? as usize;
    r.skip(props * 3)?;
    let groups = r.u8()? as usize;
    for _ in 0..groups {
        r.skip(4 + 1)?; // group id, sync type
        let states = r.u8()? as usize;
        r.skip(states * 8)?;
    }
    Ok(())
}

fn initial_rtpc(r: &mut Reader) -> Result<()> {
    let n = r.u16()? as usize;
    r.count(n, 512, "rtpc")?;
    for _ in 0..n {
        r.skip(4 + 1 + 1 + 1 + 4 + 1)?; // id, type, accumulation, parameter id, curve id, scaling
        let points = r.u16()? as usize;
        r.count(points, 512, "graph point")?;
        r.skip(points * 12)?;
    }
    Ok(())
}

fn node_base(r: &mut Reader) -> Result<Base> {
    fx_params(r)?;
    let _override_attachment = r.u8()?;
    let _override_bus = r.u32()?;
    let parent = r.u32()?;
    let _flags = r.u8()?;
    let mut props = Props::default();
    prop_bundle(r, &mut props)?;
    positioning(r)?;
    aux(r)?;
    adv_settings(r)?;
    state_chunk(r)?;
    initial_rtpc(r)?;
    Ok(Base { parent, props })
}

fn children(r: &mut Reader) -> Result<Vec<u32>> {
    let n = r.u32()? as usize;
    r.count(n, 4096, "child")?;
    (0..n).map(|_| r.u32()).collect()
}

// ------------------------------------------------------------------------------------------------ objects

/// Only "source" plugins (generators such as silence or tone; plugin type nibble 2) carry a parameter block after the media
/// information; the codecs real recordings use (type nibble 1: PCM, ADPCM, Vorbis, ...) do not.
fn plugin_has_params(plugin: u32) -> bool {
    plugin & 0x0F == 2
}

/// Parse one HIRC object body (the bytes after its id). `Ok(None)` for kinds this reader does not cover.
pub fn parse_object(kind_id: u8, body: &[u8]) -> Result<Option<Node>> {
    let mut r = Reader::new(body);
    let node = match kind_id {
        kind::SOUND => {
            let plugin = r.u32()?;
            let stream_type = r.u8()?;
            let media_id = r.u32()?;
            let _in_memory_size = r.u32()?;
            let _source_flags = r.u8()?;
            if plugin_has_params(plugin) {
                let n = r.u32()? as usize;
                r.count(n, 1 << 16, "plugin parameter byte")?;
                r.skip(n)?;
            }
            let base = node_base(&mut r)?;
            Node::Sound { base, source: SoundSource { plugin, stream_type, media_id } }
        }
        kind::RAN_SEQ => {
            let base = node_base(&mut r)?;
            let _loop_count = r.u16()?;
            let _loop_min = r.u16()?;
            let _loop_max = r.u16()?;
            r.skip(12)?; // transition time and its modifiers
            let avoid_repeat = r.u16()?;
            let _transition_mode = r.u8()?;
            let _random_mode = r.u8()?;
            let mode = r.u8()?;
            let _flags = r.u8()?;
            let kids = children(&mut r)?;
            let n = r.u16()? as usize;
            r.count(n, 4096, "playlist item")?;
            let mut playlist = Vec::with_capacity(n);
            for _ in 0..n {
                playlist.push((r.u32()?, r.i32()?));
            }
            Node::RanSeq { base, sequence: mode == 1, avoid_repeat, children: kids, playlist }
        }
        kind::SWITCH => {
            let base = node_base(&mut r)?;
            let _group_type = r.u8()?;
            let _group_id = r.u32()?;
            let default_switch = r.u32()?;
            let _continuous = r.u8()?;
            let kids = children(&mut r)?;
            let groups = r.u32()? as usize;
            r.count(groups, 4096, "switch")?;
            let mut packages = Vec::with_capacity(groups);
            for _ in 0..groups {
                let switch_id = r.u32()?;
                let nodes = children(&mut r)?;
                packages.push((switch_id, nodes));
            }
            let params = r.u32()? as usize;
            r.count(params, 4096, "switch parameter")?;
            r.skip(params * (4 + 2 + 4 + 4))?; // node id, flags, fade out, fade in
            Node::Switch { base, default_switch, children: kids, packages }
        }
        kind::LAYER => {
            let base = node_base(&mut r)?;
            let kids = children(&mut r)?;
            let layers = r.u32()? as usize;
            r.count(layers, 256, "layer")?;
            for _ in 0..layers {
                r.skip(4)?; // layer id
                initial_rtpc(&mut r)?;
                r.skip(4 + 1)?; // rtpc id, rtpc type
                let assoc = r.u32()? as usize;
                r.count(assoc, 4096, "associated child")?;
                for _ in 0..assoc {
                    r.skip(4)?;
                    let points = r.u32()? as usize;
                    r.count(points, 512, "graph point")?;
                    r.skip(points * 12)?;
                }
            }
            let _continuous = r.u8()?;
            Node::Layer { base, children: kids }
        }
        kind::ACTOR_MIXER => {
            let base = node_base(&mut r)?;
            let kids = children(&mut r)?;
            Node::ActorMixer { base, children: kids }
        }
        _ => return Ok(None),
    };
    if r.left() != 0 {
        bail!("{} byte(s) left over after the object (parsed {} of {})", r.left(), r.p, body.len());
    }
    Ok(Some(node))
}

/// What reading a whole bank strictly found.
pub struct Parsed {
    pub nodes: HashMap<u32, Node>,
    /// objects of the covered kinds that were understood / that were not
    pub ok: usize,
    pub failed: usize,
    /// the first few failures: (kind, object id, reason)
    pub failures: Vec<(u8, u32, String)>,
}

pub fn parse_bank(bank: &Bank) -> Parsed {
    let mut p = Parsed { nodes: HashMap::new(), ok: 0, failed: 0, failures: Vec::new() };
    for o in &bank.objects {
        p.add(bank, o);
    }
    p
}

impl Parsed {
    fn add(&mut self, bank: &Bank, o: &HircObject) {
        match parse_object(o.kind, bank.body(o)) {
            Ok(Some(n)) => {
                self.ok += 1;
                self.nodes.insert(o.id, n);
            }
            Ok(None) => {}
            Err(e) => {
                self.failed += 1;
                if self.failures.len() < 8 {
                    self.failures.push((o.kind, o.id, format!("{e:#}")));
                }
            }
        }
    }

    pub fn summary(&self) -> String {
        format!("{} of {} sound/container objects read exactly", self.ok, self.ok + self.failed)
    }
}

// ------------------------------------------------------------------------------------------------ resolving an event

/// One sound that plays as part of an event, with everything needed to put it in the mix.
#[derive(Debug, Clone, PartialEq)]
pub struct Voice {
    pub source: SoundSource,
    /// linear gain from the volume of the sound and of every container above it
    pub gain: f32,
    pub delay_ms: u32,
    pub pitch_cents: f32,
}

fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Add up the properties of a node and its ancestors (Wwise volume, pitch and delay are cumulative down the tree).
fn inherited(nodes: &HashMap<u32, Node>, base: &Base, rng: &mut dyn FnMut(usize) -> usize) -> (f32, f32, i32) {
    let (mut db, mut cents, mut delay) = (0.0f32, 0.0f32, 0i32);
    let mut cur = Some(*base);
    for _ in 0..16 {
        let Some(b) = cur else { break };
        db += b.props.volume_db + b.props.make_up_db;
        if let Some((lo, hi)) = b.props.volume_range {
            let t = rng(1001) as f32 / 1000.0;
            db += lo + (hi - lo) * t;
        }
        cents += b.props.pitch_cents;
        delay += b.props.delay_ms;
        cur = nodes.get(&b.parent).map(|n| *n.base());
    }
    (db, cents, delay)
}

/// What plays together for one playback of the object `id`: sounds are collected into `out`. `rng(n)` gives a number
/// in `0..n`. Random containers choose one playlist item by weight, switch containers play what is assigned to their
/// default switch, layer containers and actor-mixers play all their children.
pub fn resolve(nodes: &HashMap<u32, Node>, id: u32, rng: &mut dyn FnMut(usize) -> usize, depth: usize, out: &mut Vec<Voice>) {
    if depth > 32 || out.len() > 64 {
        return;
    }
    let Some(node) = nodes.get(&id) else { return };
    match node {
        Node::Sound { base, source } => {
            let (db, cents, delay) = inherited(nodes, base, rng);
            out.push(Voice { source: source.clone(), gain: db_to_linear(db), delay_ms: delay.max(0) as u32, pitch_cents: cents });
        }
        Node::RanSeq { sequence, children, playlist, .. } => {
            let items: Vec<(u32, i32)> = if playlist.is_empty() { children.iter().map(|c| (*c, 50)).collect() } else { playlist.clone() };
            if items.is_empty() {
                return;
            }
            let pick = if *sequence {
                0
            } else {
                let total: i64 = items.iter().map(|(_, w)| (*w).max(1) as i64).sum();
                let mut t = rng(total as usize) as i64;
                let mut chosen = 0;
                for (i, (_, w)) in items.iter().enumerate() {
                    let w = (*w).max(1) as i64;
                    if t < w {
                        chosen = i;
                        break;
                    }
                    t -= w;
                }
                chosen
            };
            resolve(nodes, items[pick].0, rng, depth + 1, out);
        }
        Node::Switch { default_switch, children, packages, .. } => {
            match packages.iter().find(|(s, _)| s == default_switch) {
                Some((_, assigned)) if !assigned.is_empty() => {
                    for c in assigned {
                        resolve(nodes, *c, rng, depth + 1, out);
                    }
                }
                // no assignment for the default switch: one of the children
                _ => {
                    if !children.is_empty() {
                        let c = children[rng(children.len())];
                        resolve(nodes, c, rng, depth + 1, out);
                    }
                }
            }
        }
        Node::Layer { children, .. } | Node::ActorMixer { children, .. } => {
            for c in children {
                resolve(nodes, *c, rng, depth + 1, out);
            }
        }
    }
}

/// Resolve an event (by its object id) through its Play actions.
pub fn resolve_event(bank: &Bank, nodes: &HashMap<u32, Node>, event_id: u32, rng: &mut dyn FnMut(usize) -> usize) -> Vec<Voice> {
    let mut out = Vec::new();
    let Some(ev) = bank.object(event_id) else { return out };
    for a in bank.event_actions(ev) {
        let Some(ao) = bank.object(a) else { continue };
        let Some((ty, target)) = bank.action(ao) else { continue };
        if ty == 0x0403 {
            resolve(nodes, target, rng, 0, &mut out);
        }
    }
    out
}

#[cfg(test)]
pub mod testenc {
    //! Encoders for the objects above, written from the same layout description, so the tests can build banks.
    pub fn base(parent: u32, volume_db: f32, delay_ms: i32, volume_range: Option<(f32, f32)>) -> Vec<u8> {
        let mut b = vec![0u8, 0]; // no effects
        b.push(0); // override attachment
        b.extend(0u32.to_le_bytes()); // override bus
        b.extend(parent.to_le_bytes());
        b.push(0); // flags
        // property bundle: volume + delay
        b.push(2);
        b.extend([0x00, 0x0F]);
        b.extend(volume_db.to_le_bytes());
        b.extend(delay_ms.to_le_bytes());
        match volume_range {
            Some((lo, hi)) => {
                b.push(1);
                b.push(0x00);
                b.extend(lo.to_le_bytes());
                b.extend(hi.to_le_bytes());
            }
            None => b.push(0),
        }
        b.push(0b0000_0000); // positioning: emitter, not listener relative
        b.push(0); // aux flags
        b.extend(0u32.to_le_bytes()); // reflections bus
        b.extend([0u8; 6]); // advanced settings
        b.extend([0u8, 0]); // state chunk: no properties, no groups
        b.extend(0u16.to_le_bytes()); // no rtpc
        b
    }
    pub fn sound(parent: u32, media: u32, volume_db: f32, delay_ms: i32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(0x0004_0001u32.to_le_bytes());
        b.push(1);
        b.extend(media.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.push(0);
        b.extend(base(parent, volume_db, delay_ms, None));
        b
    }
    pub fn kids(ids: &[u32]) -> Vec<u8> {
        let mut b = (ids.len() as u32).to_le_bytes().to_vec();
        for i in ids {
            b.extend(i.to_le_bytes());
        }
        b
    }
    pub fn ranseq(parent: u32, ids: &[(u32, i32)], sequence: bool) -> Vec<u8> {
        let mut b = base(parent, 0.0, 0, None);
        b.extend([1u8, 0, 0, 0, 0, 0]); // loop count and modifiers
        b.extend([0u8; 12]);
        b.extend(1u16.to_le_bytes()); // avoid repeat
        b.extend([0u8, 0, sequence as u8, 0]);
        b.extend(kids(&ids.iter().map(|i| i.0).collect::<Vec<_>>()));
        b.extend((ids.len() as u16).to_le_bytes());
        for (id, w) in ids {
            b.extend(id.to_le_bytes());
            b.extend(w.to_le_bytes());
        }
        b
    }
    pub fn switch(parent: u32, default_switch: u32, children: &[u32], packages: &[(u32, Vec<u32>)]) -> Vec<u8> {
        let mut b = base(parent, 0.0, 0, None);
        b.push(0);
        b.extend(7u32.to_le_bytes());
        b.extend(default_switch.to_le_bytes());
        b.push(0);
        b.extend(kids(children));
        b.extend((packages.len() as u32).to_le_bytes());
        for (s, nodes) in packages {
            b.extend(s.to_le_bytes());
            b.extend(kids(nodes));
        }
        b.extend(0u32.to_le_bytes());
        b
    }
    pub fn layer(parent: u32, children: &[u32], volume_db: f32) -> Vec<u8> {
        let mut b = base(parent, volume_db, 0, None);
        b.extend(kids(children));
        b.extend(1u32.to_le_bytes()); // one layer with an rtpc curve
        b.extend(9u32.to_le_bytes());
        b.extend(0u16.to_le_bytes()); // layer's own rtpcs
        b.extend(55u32.to_le_bytes());
        b.push(0);
        b.extend((children.len() as u32).to_le_bytes());
        for c in children {
            b.extend(c.to_le_bytes());
            b.extend(2u32.to_le_bytes());
            for _ in 0..2 {
                b.extend(0f32.to_le_bytes());
                b.extend(1f32.to_le_bytes());
                b.extend(4u32.to_le_bytes());
            }
        }
        b.push(0);
        b
    }
    pub fn mixer(parent: u32, children: &[u32], volume_db: f32) -> Vec<u8> {
        let mut b = base(parent, volume_db, 0, None);
        b.extend(kids(children));
        b
    }
    /// (kind, id + body) as the HIRC chunk stores an object
    pub fn obj(kind: u8, id: u32, body: &[u8]) -> (u8, Vec<u8>) {
        let mut v = id.to_le_bytes().to_vec();
        v.extend_from_slice(body);
        (kind, v)
    }
    pub fn action(id: u32, ty: u16, target: u32) -> (u8, Vec<u8>) {
        let mut body = ty.to_le_bytes().to_vec();
        body.extend(target.to_le_bytes());
        body.extend([0u8, 0, 0]);
        obj(3, id, &body)
    }
    pub fn event(id: u32, actions: &[u32]) -> (u8, Vec<u8>) {
        let mut body = vec![actions.len() as u8];
        for a in actions {
            body.extend(a.to_le_bytes());
        }
        obj(4, id, &body)
    }
    /// A bank file (version 150) holding these objects.
    pub fn bank(objs: Vec<(u8, Vec<u8>)>) -> Vec<u8> {
        let mut bkhd = 150u32.to_le_bytes().to_vec();
        bkhd.extend(0xB4A4u32.to_le_bytes());
        bkhd.extend([0u8; 16]);
        let mut hirc = (objs.len() as u32).to_le_bytes().to_vec();
        for (k, body) in &objs {
            hirc.push(*k);
            hirc.extend((body.len() as u32).to_le_bytes());
            hirc.extend_from_slice(body);
        }
        let mut out = b"BKHD".to_vec();
        out.extend((bkhd.len() as u32).to_le_bytes());
        out.extend(bkhd);
        out.extend(b"HIRC");
        out.extend((hirc.len() as u32).to_le_bytes());
        out.extend(hirc);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::testenc as enc;
    use super::*;

    #[test]
    fn a_sound_is_read_exactly_and_extra_bytes_are_an_error() {
        let body = enc::sound(77, 1234, -6.0, 40);
        let n = parse_object(kind::SOUND, &body).unwrap().unwrap();
        match n {
            Node::Sound { base, source } => {
                assert_eq!((base.parent, base.props.volume_db, base.props.delay_ms), (77, -6.0, 40));
                assert_eq!((source.media_id, source.stream_type, source.plugin), (1234, 1, 0x0004_0001));
            }
            other => panic!("{other:?}"),
        }
        let mut more = body.clone();
        more.push(0);
        assert!(parse_object(kind::SOUND, &more).unwrap_err().to_string().contains("left over"));
        assert!(parse_object(kind::SOUND, &body[..body.len() - 3]).unwrap_err().to_string().contains("past the end"));
        assert!(parse_object(kind::EVENT, &body).unwrap().is_none(), "kinds it does not cover are skipped");
    }

    #[test]
    fn containers_round_trip_through_the_reader() {
        let r = parse_object(kind::RAN_SEQ, &enc::ranseq(1, &[(10, 50), (11, 25)], false)).unwrap().unwrap();
        assert!(matches!(&r, Node::RanSeq { sequence: false, playlist, children, .. } if playlist == &vec![(10, 50), (11, 25)] && children == &vec![10, 11]));
        let s = parse_object(kind::SWITCH, &enc::switch(1, 8, &[20, 21], &[(7, vec![20]), (8, vec![21])])).unwrap().unwrap();
        assert!(matches!(&s, Node::Switch { default_switch: 8, packages, .. } if packages.len() == 2));
        let l = parse_object(kind::LAYER, &enc::layer(1, &[30, 31], -3.0)).unwrap().unwrap();
        assert!(matches!(&l, Node::Layer { children, base } if children == &vec![30, 31] && base.props.volume_db == -3.0));
        let m = parse_object(kind::ACTOR_MIXER, &enc::mixer(0, &[40], -1.0)).unwrap().unwrap();
        assert!(matches!(&m, Node::ActorMixer { children, .. } if children == &vec![40]));
    }

    fn bank_with_event() -> (Bank, u32) {
        // event 900 -> play 800 -> mixer 700 (-6 dB) -> layer 600 -> [random 500 -> {sound 1 (media 101), sound 2 (media 102)}, sound 3 (media 103, 25 ms late)]
        let objs = vec![
            enc::obj(kind::SOUND, 1, &enc::sound(500, 101, 0.0, 0)),
            enc::obj(kind::SOUND, 2, &enc::sound(500, 102, -2.0, 0)),
            enc::obj(kind::SOUND, 3, &enc::sound(600, 103, 0.0, 25)),
            enc::obj(kind::RAN_SEQ, 500, &enc::ranseq(600, &[(1, 50), (2, 50)], false)),
            enc::obj(kind::LAYER, 600, &enc::layer(700, &[500, 3], 0.0)),
            enc::obj(kind::ACTOR_MIXER, 700, &enc::mixer(0, &[600], -6.0)),
            enc::action(800, 0x0403, 700),
            enc::action(801, 0x0102, 700),
            enc::event(900, &[800, 801]),
        ];
        (Bank::parse(enc::bank(objs)).unwrap(), 900)
    }

    #[test]
    fn a_whole_bank_is_read_and_an_event_resolves_with_inherited_volume_delay_and_one_random_pick() {
        let (bank, ev) = bank_with_event();
        let parsed = parse_bank(&bank);
        assert_eq!((parsed.ok, parsed.failed), (6, 0), "{:?}", parsed.failures);
        assert!(parsed.summary().contains("6 of 6"));

        let mut picks = std::collections::HashSet::new();
        for seed in 0..40usize {
            let mut n = seed;
            let mut rng = |m: usize| {
                n = n.wrapping_mul(1103515245).wrapping_add(12345);
                (n >> 8) % m
            };
            let v = resolve_event(&bank, &parsed.nodes, ev, &mut rng);
            assert_eq!(v.len(), 2, "one random pick plus the other layer: {v:?}");
            let layer3 = v.iter().find(|x| x.source.media_id == 103).expect("the plain layer always plays");
            assert_eq!(layer3.delay_ms, 25);
            assert!((layer3.gain - db_to_linear(-6.0)).abs() < 1e-5, "the mixer's -6 dB reaches it: {}", layer3.gain);
            let random = v.iter().find(|x| x.source.media_id != 103).unwrap();
            picks.insert(random.source.media_id);
            let expect_db = if random.source.media_id == 102 { -8.0 } else { -6.0 };
            assert!((random.gain - db_to_linear(expect_db)).abs() < 1e-5);
        }
        assert_eq!(picks, [101, 102].into_iter().collect(), "both variants are reachable");
    }

    #[test]
    fn switch_containers_follow_the_default_switch_and_weights_are_respected() {
        let bank = Bank::parse(enc::bank(vec![
            enc::obj(kind::SOUND, 1, &enc::sound(50, 11, 0.0, 0)),
            enc::obj(kind::SOUND, 2, &enc::sound(50, 12, 0.0, 0)),
            enc::obj(kind::SOUND, 3, &enc::sound(51, 13, 0.0, 0)),
            enc::obj(kind::SOUND, 4, &enc::sound(51, 14, 0.0, 0)),
            enc::obj(kind::RAN_SEQ, 51, &enc::ranseq(50, &[(3, 1000), (4, 1)], false)),
            enc::obj(kind::SWITCH, 50, &enc::switch(0, 2, &[1, 2, 51], &[(1, vec![1]), (2, vec![51])])),
            enc::action(60, 0x0403, 50),
            enc::event(70, &[60]),
        ]))
        .unwrap();
        let parsed = parse_bank(&bank);
        assert_eq!(parsed.failed, 0, "{:?}", parsed.failures);
        let mut counts = std::collections::HashMap::new();
        let mut n = 7usize;
        for _ in 0..300 {
            let mut rng = |m: usize| {
                n = n.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (n >> 33) % m
            };
            let v = resolve_event(&bank, &parsed.nodes, 70, &mut rng);
            assert_eq!(v.len(), 1);
            *counts.entry(v[0].source.media_id).or_insert(0) += 1;
        }
        assert!(!counts.contains_key(&11) && !counts.contains_key(&12), "only the default switch's branch plays: {counts:?}");
        assert!(counts[&13] > 250, "the heavy item wins nearly always: {counts:?}");
    }

    #[test]
    fn an_unreadable_object_is_counted_not_fatal() {
        let bank = Bank::parse(enc::bank(vec![enc::obj(kind::SOUND, 1, &enc::sound(0, 5, 0.0, 0)), enc::obj(kind::SOUND, 2, &[1, 2, 3])])).unwrap();
        let p = parse_bank(&bank);
        assert_eq!((p.ok, p.failed), (1, 1));
        assert_eq!(p.failures[0].1, 2);
        assert!(p.summary().contains("1 of 2"));
    }

    #[test]
    fn cycles_and_runaway_trees_stop() {
        let bank = Bank::parse(enc::bank(vec![enc::obj(kind::ACTOR_MIXER, 1, &enc::mixer(0, &[2], 0.0)), enc::obj(kind::ACTOR_MIXER, 2, &enc::mixer(1, &[1], 0.0))])).unwrap();
        let p = parse_bank(&bank);
        let mut out = Vec::new();
        resolve(&p.nodes, 1, &mut |_| 0, 0, &mut out);
        assert!(out.is_empty());
    }
}
