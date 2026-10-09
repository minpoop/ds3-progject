//! Space Marine 2 template files: the `.tpl` that describes a model (name, skeleton, animation names, level-of-detail
//! definitions, texture names and the geometry graph) and the `.tpl_data` next to it that holds the vertex and index buffers.
//!
//! This is an independent reader. The layout was worked out from the player's own files with the help of the format facts
//! published by the modding community (the Saber "1SER" property-flag serialization: a list of properties, a presence byte
//! per property, then the values of all items of a list one property after the other; sentinel chunks that carry the file
//! offset at which they end). Every chunk that names its end is checked against where the reader really is, so a layout
//! that does not fit stops with the file offset in the message instead of producing garbage.
//!
//! Only reading: nothing here writes a file, and the files are never modified.
use std::fmt;

/// Where and why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TplError {
    pub at: usize,
    pub what: String,
}

impl fmt::Display for TplError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at file offset {:#x})", self.what, self.at)
    }
}

impl std::error::Error for TplError {}

type Result<T> = std::result::Result<T, TplError>;

/// Longest string / biggest list this reader accepts (a damaged length must not allocate wildly).
const MAX_STRING: usize = 1 << 20;
const MAX_ITEMS: usize = 1 << 20;

/// A little-endian reader that never reads outside its slice.
pub struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    pub fn new(b: &'a [u8]) -> Cur<'a> {
        Cur { b, p: 0 }
    }

    pub fn at(b: &'a [u8], p: usize) -> Cur<'a> {
        Cur { b, p }
    }

    pub fn pos(&self) -> usize {
        self.p
    }

    pub fn set_pos(&mut self, p: usize) {
        self.p = p;
    }

    pub fn len(&self) -> usize {
        self.b.len()
    }

    pub fn is_empty(&self) -> bool {
        self.b.is_empty()
    }

    fn err<T>(&self, what: impl Into<String>) -> Result<T> {
        Err(TplError { at: self.p, what: what.into() })
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let Some(end) = self.p.checked_add(n).filter(|e| *e <= self.b.len()) else { return self.err(format!("the file ends too early (wanted {n} bytes)")) };
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let s = self.bytes(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    pub fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    pub fn vec4(&mut self) -> Result<[f32; 4]> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }

    /// 16 floats, as they are stored (row by row).
    pub fn matrix4(&mut self) -> Result<[f32; 16]> {
        let mut m = [0f32; 16];
        for v in &mut m {
            *v = self.f32()?;
        }
        Ok(m)
    }

    /// A 3x3 matrix, widened to 4x4 with the usual identity row and column.
    pub fn matrix3(&mut self) -> Result<[f32; 16]> {
        let mut m = [0f32; 16];
        for row in 0..3 {
            for col in 0..3 {
                m[row * 4 + col] = self.f32()?;
            }
        }
        m[15] = 1.0;
        Ok(m)
    }

    fn text(&mut self, len: usize) -> Result<String> {
        if len > MAX_STRING {
            return self.err(format!("a string of {len} bytes is not believable"));
        }
        let raw = self.bytes(len)?;
        // the files use plain ASCII / UTF-8 names; anything else is shown with replacement characters
        Ok(String::from_utf8_lossy(raw).into_owned())
    }

    /// A string with a 32-bit length in front.
    pub fn lps32(&mut self) -> Result<String> {
        let len = self.i32()?;
        match usize::try_from(len) {
            Ok(n) => self.text(n),
            Err(_) => self.err(format!("a string has the negative length {len}")),
        }
    }

    /// A string with a 16-bit length in front.
    pub fn lps16(&mut self) -> Result<String> {
        let len = self.i16()?;
        match usize::try_from(len) {
            Ok(n) => self.text(n),
            Err(_) => self.err(format!("a string has the negative length {len}")),
        }
    }

    /// A list length that is plausible.
    fn count32(&mut self, what: &str) -> Result<usize> {
        let n = self.i32()?;
        match usize::try_from(n) {
            Ok(n) if n <= MAX_ITEMS => Ok(n),
            _ => self.err(format!("{what}: a count of {n} is not believable")),
        }
    }

    /// The position must be exactly `end` (a chunk said where it ends).
    pub fn expect_pos(&self, end: usize, what: &str) -> Result<()> {
        if self.p == end {
            Ok(())
        } else {
            self.err(format!("{what}: the chunk should end at {end:#x} but the reader is at {:#x}", self.p))
        }
    }

    /// The presence byte in front of a property of a list: false = the property is not stored.
    fn present(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }

    /// `n` bits, least significant bit of the first byte first (a bit array stored as bytes).
    fn bit_array(&mut self, n: usize) -> Result<Vec<bool>> {
        let bytes = self.bytes(n.div_ceil(8))?;
        Ok((0..n).map(|i| bytes[i / 8] >> (i % 8) & 1 != 0).collect())
    }
}

/// A set of flags stored as a bit count followed by that many bits (rounded up to whole bytes).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BitSet {
    pub count: usize,
    bits: Vec<u8>,
}

impl BitSet {
    /// `count_bytes` is the width of the stored bit count (2 or 4).
    fn read(c: &mut Cur, count_bytes: usize) -> Result<BitSet> {
        let count = match count_bytes {
            2 => usize::from(c.u16()?),
            _ => {
                let n = c.i32()?;
                match usize::try_from(n) {
                    Ok(n) if n <= 4096 => n,
                    _ => return c.err(format!("a flag set of {n} bits is not believable")),
                }
            }
        };
        let bits = c.bytes(count.div_ceil(8))?.to_vec();
        Ok(BitSet { count, bits })
    }

    pub fn get(&self, i: usize) -> bool {
        self.bits.get(i / 8).is_some_and(|b| b >> (i % 8) & 1 != 0)
    }

    /// The bits below 64 as a number (bit i of the set = bit i of the number).
    pub fn low64(&self) -> u64 {
        (0..64).filter(|i| self.get(*i)).fold(0u64, |acc, i| acc | 1 << i)
    }
}

// ------------------------------------------------------------------------------------------------ the parts

/// The skeleton description: how many bones, and how many of them each level of detail keeps.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skin {
    pub bone_count: u32,
    pub lod_bone_counts: Vec<u16>,
    /// One 4x4 matrix per bone, when the file has them (the weapon files do not).
    pub inverse_bind: Option<Vec<[f32; 16]>>,
}

/// A named animation sequence of the template (the weapons have a few: `anim1`, `chain_anim`, `finisher1`, ...).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimSeq {
    pub name: String,
    pub layer: u32,
    pub start_frame: f32,
    pub end_frame: f32,
    pub time_sec: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LodDef {
    pub object: i16,
    pub index: u8,
    pub last_lod_up_to_infinity: bool,
}

/// One node of the model's object tree.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Object {
    pub id: i16,
    pub name: Option<String>,
    pub parent: i16,
    pub next: i16,
    pub prev: i16,
    pub child: i16,
    /// The transform relative to the parent / in model space, when stored.
    pub local: Option<[f32; 16]>,
    pub model: Option<[f32; 16]>,
    /// The first split (sub mesh) of this object and how many follow, when it has geometry.
    pub split_index: Option<u32>,
    pub num_splits: Option<u32>,
    pub bbox: Option<[f32; 6]>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LodRoot {
    pub object_ids: Vec<u32>,
    pub max_lod_indices: Vec<u32>,
    pub max_distances: Vec<f32>,
    pub bbox: Option<[f32; 6]>,
}

/// A buffer of vertex or index data and how its elements are laid out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Buffer {
    /// The flags that say what each element holds (the "flexible vertex format").
    pub flags: BitSet,
    /// Bytes from one element to the next.
    pub stride: u16,
    pub length: u32,
    /// Where the buffer's bytes start inside the data (the `.tpl_data` file, or this file when it holds them).
    pub start: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    pub flags: BitSet,
    /// (buffer id, offset inside that buffer's sub buffer) for each buffer the mesh draws from.
    pub buffers: Vec<(i32, i32)>,
}

/// A value in a material description (the files use a small typed tree: numbers, text, lists and nested property lists).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
    Bool(bool),
    Text(String),
    List(Vec<Value>),
    Class(Vec<(String, Value)>),
}

impl Value {
    /// A short text for reports: `name=value` pairs of a class, comma separated lists, plain numbers and text.
    pub fn show(&self) -> String {
        match self {
            Value::Int(v) => v.to_string(),
            Value::Float(v) => format!("{v}"),
            Value::Bool(v) => v.to_string(),
            Value::Text(v) => format!("{v:?}"),
            Value::List(items) => format!("[{}]", items.iter().map(Value::show).collect::<Vec<_>>().join(", ")),
            Value::Class(props) => format!("{{{}}}", props.iter().map(|(k, v)| format!("{k}={}", v.show())).collect::<Vec<_>>().join(", ")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SubMesh {
    pub vertex_offset: u16,
    pub vertex_count: u16,
    pub face_offset: u16,
    pub face_count: u16,
    pub node: i16,
    pub skin_compound: i16,
    pub mesh: i32,
    /// The bones the vertices' bone numbers refer to (when the mesh has them).
    pub bone_ids: Vec<i16>,
    /// Scale of each set of texture coordinates (set number -> scale), when stored.
    pub uv_scaling: Vec<(u8, i16)>,
    /// Position and scale (as 16-bit numbers) the vertex positions are relative to, when stored.
    pub transform: Option<([i16; 3], [i16; 3])>,
    /// The material as the file stores it: a list of named values (texture names, numbers, flags).
    pub material: Vec<(String, Value)>,
}

impl SubMesh {
    /// The name of the texture the material names (`shadingMtl_Tex`), when it names one.
    pub fn texture_name(&self) -> Option<&str> {
        self.material.iter().find_map(|(k, v)| match v {
            Value::Text(t) if k == "shadingMtl_Tex" && !t.is_empty() => Some(t.as_str()),
            _ => None,
        })
    }

    /// The name of the shader the material names (`shadingMtl_Mtl`).
    pub fn shader_name(&self) -> Option<&str> {
        self.material.iter().find_map(|(k, v)| match v {
            Value::Text(t) if k == "shadingMtl_Mtl" && !t.is_empty() => Some(t.as_str()),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    pub root_node: i16,
    pub node_count: i32,
    pub buffer_count: i32,
    pub mesh_count: i32,
    pub sub_mesh_count: i32,
    pub objects: Vec<Object>,
    pub lod_roots: Vec<LodRoot>,
    /// Per buffer: where its bytes sit in the data file, as the mapping says (0 = right after the previous one).
    pub stream_offsets: Vec<i32>,
    pub stream_sizes: Vec<i32>,
    pub buffers: Vec<Buffer>,
    pub meshes: Vec<Mesh>,
    pub sub_meshes: Vec<SubMesh>,
    /// Whether the buffers' bytes follow in this file (else they are in the `.tpl_data`).
    pub buffers_inline: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Template {
    /// The strings of the file header (usually none).
    pub header_strings: Vec<String>,
    pub name: Option<String>,
    pub state: Option<u32>,
    pub affixes: Option<String>,
    pub ps: Option<String>,
    pub skin: Option<Skin>,
    pub anim_sequences: Vec<AnimSeq>,
    pub bbox: Option<[f32; 6]>,
    pub lods: Vec<LodDef>,
    pub textures: Vec<String>,
    pub geometry: Option<Geometry>,
    /// The bits of the property set (which parts the file has).
    pub property_bits: u64,
    /// Where the geometry graph started and the file offset the reader stopped at.
    pub geometry_at: Option<usize>,
    pub end_at: usize,
}

// ------------------------------------------------------------------------------------------------ reading

const MAGIC_1SER: &[u8; 4] = b"1SER";
const MAGIC_TPL: &[u8; 4] = b"tpl\0";
const MAGIC_TPL1: &[u8; 4] = b"TPL1";
const MAGIC_OGM1: &[u8; 4] = b"OGM1";

/// Bits of a mesh's flag set that say which per-sub-mesh tables follow (they mirror the vertex format: compressed positions
/// need a position and scale, bone indices need the list of bones, compressed texture coordinates need a scale).
const MESH_HAS_TRANSFORM: usize = 3;
const MESH_HAS_BONE_IDS: usize = 9;
const MESH_HAS_UV_SCALING: usize = 30;

impl Template {
    /// Reads a `.tpl` file.
    pub fn parse(bytes: &[u8]) -> Result<Template> {
        let (t, err) = Template::parse_partial(bytes);
        match err {
            None => Ok(t),
            Some(e) => Err(e),
        }
    }

    /// The sub meshes of the full-detail model: the ones that belong to the object the level-of-detail definition number 0
    /// names. The templates of the weapons keep, next to the loose parts (a base and its separate teeth, a magazine and a
    /// bolt), one merged mesh per level that holds all of them in their resting place; that merged mesh is what this finds.
    /// Empty when the file defines no levels or has no geometry.
    pub fn full_detail_sub_meshes(&self) -> Vec<usize> {
        let Some(g) = &self.geometry else { return Vec::new() };
        let Some(level0) = self.lods.iter().find(|l| l.index == 0) else { return Vec::new() };
        g.sub_meshes.iter().enumerate().filter(|(_, s)| s.node == level0.object).map(|(i, _)| i).collect()
    }

    /// The texture names the materials of the full-detail sub meshes name, without repeats, in file order.
    pub fn full_detail_texture_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let Some(g) = &self.geometry else { return names };
        for i in self.full_detail_sub_meshes() {
            if let Some(name) = g.sub_meshes[i].texture_name() {
                if !names.iter().any(|n| n == name) {
                    names.push(name.to_string());
                }
            }
        }
        names
    }

    /// Reads as much of a `.tpl` file as it can: what was read before the first problem, and the problem (if any).
    pub fn parse_partial(bytes: &[u8]) -> (Template, Option<TplError>) {
        let mut t = Template::default();
        let result = Template::fill(bytes, &mut t);
        (t, result.err())
    }

    fn fill(bytes: &[u8], t: &mut Template) -> Result<()> {
        let mut c = Cur::new(bytes);
        t.header_strings = read_header(&mut c)?;
        if c.bytes(4)? != MAGIC_TPL1 {
            return c.err("the property section does not start with TPL1");
        }
        let props = BitSet::read(&mut c, 4)?;
        t.property_bits = props.low64();
        if props.get(0) {
            t.name = Some(c.lps32()?);
        }
        if props.get(1) {
            // the class name of the template (never seen in use)
            let _ = c.lps32()?;
        }
        if props.get(2) {
            let _bit_count = c.u16()?;
            t.state = Some(c.u32()?);
        }
        if props.get(3) {
            t.affixes = Some(c.lps32()?);
        }
        if props.get(4) {
            t.ps = Some(c.lps32()?);
        }
        if props.get(5) {
            t.skin = Some(read_skin(&mut c)?);
        }
        if props.get(6) {
            t.anim_sequences = read_track(&mut c)?;
        }
        if props.get(8) {
            let (a, b) = (c.vec3()?, c.vec3()?);
            t.bbox = Some([a[0], a[1], a[2], b[0], b[1], b[2]]);
        }
        if props.get(9) {
            t.lods = read_lod_defs(&mut c)?;
        }
        if props.get(10) {
            t.textures = read_texture_list(&mut c)?;
        }
        t.end_at = c.pos();
        if props.get(11) {
            t.geometry_at = Some(c.pos());
            let mut g = Geometry::default();
            let result = read_geometry(&mut c, &mut g);
            t.geometry = Some(g);
            t.end_at = c.pos();
            result?;
        }
        Ok(())
    }
}

/// The 0x40 bytes in front of the properties: "1SER", "tpl", counters, a 16-character resource id and the header strings.
fn read_header(c: &mut Cur) -> Result<Vec<String>> {
    if c.bytes(4)? != MAGIC_1SER {
        return c.err("not a Saber 1SER file");
    }
    if c.bytes(4)? != MAGIC_TPL {
        return c.err("a 1SER file, but not a template (.tpl)");
    }
    c.bytes(8 + 8 + 8)?; // counters and sizes
    c.u32()?; // flags
    c.bytes(16)?; // "S3DRESOURCE    "
    c.i32()?;
    let strings = c.count32("header strings")?;
    c.i32()?;
    let mut out = Vec::new();
    for _ in 0..strings {
        c.u16()?;
        c.u8()?;
        c.u8()?;
        c.bytes(16)?;
        out.push(c.lps32()?);
    }
    Ok(out)
}

fn read_skin(c: &mut Cur) -> Result<Skin> {
    let mut skin = Skin { bone_count: c.u32()?, ..Skin::default() };
    c.u16()?;
    c.u8()?;
    loop {
        let sentinel = c.u16()?;
        let end = c.u32()? as usize;
        match sentinel {
            0xFFFF => {
                c.expect_pos(end, "skin end")?;
                return Ok(skin);
            }
            0 => {
                if skin.bone_count as usize > MAX_ITEMS {
                    return c.err("too many bones");
                }
                let mut list = Vec::with_capacity(skin.bone_count as usize);
                for _ in 0..skin.bone_count {
                    list.push(c.matrix4()?);
                }
                skin.inverse_bind = Some(list);
            }
            1 => {
                let n = c.u32()? as usize;
                if n > MAX_ITEMS {
                    return c.err("too many levels of detail");
                }
                for _ in 0..n {
                    skin.lod_bone_counts.push(c.u16()?);
                }
            }
            other => return c.err(format!("unknown skin chunk {other:#x}")),
        }
        c.expect_pos(end, "skin chunk")?;
    }
}

fn read_track(c: &mut Cur) -> Result<Vec<AnimSeq>> {
    let props = BitSet::read(c, 4)?;
    let mut seqs = Vec::new();
    if props.get(0) {
        seqs = read_anim_sequences(c)?;
    }
    if props.get(1) {
        skip_object_animations(c)?;
    }
    if props.get(2) {
        return c.err("the object map of an animation track has never been seen and is not read");
    }
    if props.get(3) {
        skip_rooted(c)?;
    }
    Ok(seqs)
}

fn read_anim_sequences(c: &mut Cur) -> Result<Vec<AnimSeq>> {
    let count = c.count32("animation sequences")?;
    let _props = c.i32()?;
    let mut seqs = vec![AnimSeq::default(); count];
    if c.present()? {
        for s in &mut seqs {
            s.name = c.lps32()?;
        }
    }
    if c.present()? {
        for s in &mut seqs {
            s.layer = c.u32()?;
        }
    }
    if c.present()? {
        for s in &mut seqs {
            s.start_frame = c.f32()?;
        }
    }
    if c.present()? {
        for s in &mut seqs {
            s.end_frame = c.f32()?;
        }
    }
    for _ in 0..2 {
        // offset frame, length in frames
        if c.present()? {
            for _ in 0..count {
                c.f32()?;
            }
        }
    }
    if c.present()? {
        for s in &mut seqs {
            s.time_sec = c.f32()?;
        }
    }
    if c.present()? {
        // action frames: for every sequence a count, a number, a byte, the frame numbers and optionally their names
        for _ in 0..count {
            let frames = c.count32("action frames")?;
            c.i32()?;
            c.u8()?;
            if frames == 0 {
                continue;
            }
            for _ in 0..frames {
                c.i32()?;
            }
            if c.u8()? != 0 {
                for _ in 0..frames {
                    c.lps32()?;
                }
            }
        }
    }
    if c.present()? {
        for _ in 0..count {
            c.vec3()?;
            c.vec3()?;
        }
    }
    Ok(seqs)
}

/// A spline: chunks of (id, end offset, value) until the chunk with id 1.
fn skip_spline(c: &mut Cur) -> Result<()> {
    let mut compressed = 0u8;
    let mut count = 0u32;
    let mut size = 0u32;
    loop {
        let id = c.u16()?;
        let end = c.u32()? as usize;
        match id {
            1 => return c.expect_pos(end, "spline end"),
            0xF0 | 0xF2 | 0xF3 => {
                c.u8()?;
            }
            0xF1 => compressed = c.u8()?,
            0xF4 => count = c.u32()?,
            0xF5 => size = c.u32()?,
            0xF6 => {
                // the numbers: 16-bit when compressed, else 32-bit; an odd byte count has one padding byte
                let n = size as usize;
                if n > 1 << 28 || (compressed == 0 && count > 0 && !n.is_multiple_of(count as usize)) {
                    return c.err("a spline with impossible sizes");
                }
                c.bytes(n)?;
            }
            other => return c.err(format!("unknown spline chunk {other:#x}")),
        }
        c.expect_pos(end, "spline chunk")?;
    }
}

fn skip_object_animations(c: &mut Cur) -> Result<()> {
    let count = c.count32("object animations")?;
    let props = c.u32()?;
    if props > 0 && c.present()? {
        for _ in 0..count {
            c.vec3()?;
        }
    }
    let spline_group = |c: &mut Cur, count: usize| -> Result<()> {
        if c.present()? {
            for has in c.bit_array(count)? {
                if has {
                    skip_spline(c)?;
                }
            }
        }
        Ok(())
    };
    if props > 1 {
        spline_group(c, count)?;
    }
    if props > 2 && c.present()? {
        for _ in 0..count {
            c.vec4()?;
        }
    }
    if props > 3 {
        spline_group(c, count)?;
    }
    if props > 4 && c.present()? {
        for _ in 0..count {
            c.vec3()?;
        }
    }
    if props > 5 {
        spline_group(c, count)?;
    }
    if props > 6 && c.present()? {
        for _ in 0..count {
            c.f32()?;
        }
    }
    if props > 7 {
        spline_group(c, count)?;
    }
    Ok(())
}

fn skip_rooted(c: &mut Cur) -> Result<()> {
    let props = BitSet::read(c, 4)?;
    if props.get(0) {
        c.vec3()?;
    }
    if props.get(1) {
        skip_spline(c)?;
    }
    if props.get(2) {
        c.vec4()?;
    }
    if props.get(3) {
        skip_spline(c)?;
    }
    Ok(())
}

fn read_lod_defs(c: &mut Cur) -> Result<Vec<LodDef>> {
    let count = c.count32("level of detail definitions")?;
    let _props = c.i32()?;
    let mut defs = vec![LodDef::default(); count];
    if c.present()? {
        for d in &mut defs {
            d.object = c.i16()?;
        }
    }
    if c.present()? {
        for d in &mut defs {
            d.index = c.u8()?;
        }
    }
    if c.present()? {
        for d in &mut defs {
            d.last_lod_up_to_infinity = c.u8()? != 0;
        }
    }
    Ok(defs)
}

fn read_texture_list(c: &mut Cur) -> Result<Vec<String>> {
    let count = c.u32()? as usize;
    if count > MAX_ITEMS {
        return c.err("too many textures");
    }
    c.u16()?;
    let end = c.u32()? as usize;
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        names.push(c.lps16()?);
    }
    c.expect_pos(end.min(c.pos()), "texture list")?;
    let marker = c.u16()?;
    let _next = c.u32()?;
    if marker != 0xFFFF {
        return c.err("the texture list does not end with its marker");
    }
    Ok(names)
}

fn read_geometry(c: &mut Cur, g: &mut Geometry) -> Result<()> {
    if c.bytes(4)? != MAGIC_OGM1 {
        return c.err("the geometry graph does not start with OGM1");
    }
    let first = c.u32()?;
    let vars = (first & 0xFFFF) as usize;
    let _version = c.u16()?;
    let flags: u32 = if vars <= 8 { u32::from(c.u8()?) } else { u32::from(c.u16()?) };
    if flags & 1 != 0 {
        g.objects = read_objects(c)?;
    }
    if flags & 2 != 0 {
        // object properties: for every object an id, a 64-bit number, a 32-bit number and a 16-bit number (meaning unknown)
        let count = c.count32("object properties")?;
        if c.present()? {
            for _ in 0..count {
                c.i16()?;
                c.u64()?;
                c.i32()?;
                c.i16()?;
            }
        }
    }
    if flags & 4 != 0 {
        let count = c.count32("object scripts")?;
        if c.present()? {
            for _ in 0..count {
                let id = c.i32()?;
                let text = c.lps32()?;
                let _ = (id, text);
            }
        }
    }
    if flags & 8 != 0 {
        g.lod_roots = read_lod_roots(c)?;
    }
    if flags & 16 != 0 {
        read_vbuffer_mapping(c, g)?;
    }
    let mut names: Vec<String> = Vec::new();
    if flags & 32 != 0 {
        let count = c.count32("named objects")?;
        if c.present()? {
            for _ in 0..count {
                names.push(c.lps32()?);
            }
        }
    }
    if flags & 64 != 0 {
        let count = c.count32("named object ids")?;
        let mut ids = Vec::with_capacity(count);
        for _ in 0..count {
            ids.push(c.u16()?);
        }
        for (id, name) in ids.iter().zip(names.iter()) {
            if let Some(o) = g.objects.iter_mut().find(|o| o.id == *id as i16) {
                if o.name.is_none() {
                    o.name = Some(name.clone());
                }
            }
        }
    }
    if flags & 128 != 0 {
        let count = c.count32("local matrices")?;
        for i in 0..count {
            let m = c.matrix4()?;
            if let Some(o) = g.objects.get_mut(i) {
                o.local = Some(m);
            }
        }
    }
    if flags & 256 != 0 {
        let count = c.count32("model matrices")?;
        for i in 0..count {
            let m = c.matrix4()?;
            if let Some(o) = g.objects.get_mut(i) {
                o.model = Some(m);
            }
        }
    }
    if flags & 512 != 0 {
        // split ranges: (start index, number of splits) per object
        let count = c.count32("split ranges")?;
        let _props = c.i32()?;
        let mut ranges = vec![(0i16, 0i16); count];
        if c.present()? {
            for r in &mut ranges {
                r.0 = c.i16()?;
            }
        }
        if c.present()? {
            for r in &mut ranges {
                r.1 = c.i16()?;
            }
        }
        let _ = ranges;
    }
    read_geometry_data(c, g)?;
    Ok(())
}

fn read_objects(c: &mut Cur) -> Result<Vec<Object>> {
    let count = c.i16()?;
    let count = usize::try_from(count).map_err(|_| TplError { at: c.pos(), what: format!("{count} objects") })?;
    let _unk = c.u16()?;
    let props = c.u16()?;
    let _unk3 = c.u16()?;
    let mut objs = vec![Object::default(); count];
    // property 0: id
    if props > 0 && c.present()? {
        for o in &mut objs {
            o.id = c.i16()?;
        }
    }
    // 1: name
    if props > 1 && c.present()? {
        for o in &mut objs {
            o.name = Some(c.lps32()?);
        }
    }
    // 2: state (two numbers each)
    if props > 2 && c.present()? {
        for _ in 0..count {
            c.u32()?;
            c.u32()?;
        }
    }
    for index in 3..=6 {
        // 3..6: parent, next, previous, first child
        if props > index && c.present()? {
            for o in &mut objs {
                let v = c.i16()?;
                match index {
                    3 => o.parent = v,
                    4 => o.next = v,
                    5 => o.prev = v,
                    _ => o.child = v,
                }
            }
        }
    }
    // 7: animation number
    if props > 7 && c.present()? {
        for _ in 0..count {
            c.i16()?;
        }
    }
    // 8: affixes
    if props > 8 && c.present()? {
        for _ in 0..count {
            c.lps32()?;
        }
    }
    // 9, 10: matrices
    if props > 9 && c.present()? {
        for o in &mut objs {
            o.local = Some(c.matrix4()?);
        }
    }
    if props > 10 && c.present()? {
        for o in &mut objs {
            o.model = Some(c.matrix4()?);
        }
    }
    // 11: geometry data of the objects that have some
    if props > 11 && c.present()? {
        let has = c.bit_array(count)?;
        for (o, has) in objs.iter_mut().zip(has) {
            if !has {
                continue;
            }
            let _n = c.i32()?;
            let f = c.u8()?;
            if f & 1 != 0 {
                o.split_index = Some(c.u32()?);
            }
            if f & 2 != 0 {
                o.num_splits = Some(c.u32()?);
            }
            if f & 4 != 0 {
                let (a, b) = (c.vec3()?, c.vec3()?);
                o.bbox = Some([a[0], a[1], a[2], b[0], b[1], b[2]]);
            }
            if f & 8 != 0 {
                // an oriented box: origin, 3x3 matrix, size
                c.vec3()?;
                c.bytes(36)?;
                c.vec3()?;
            }
        }
    }
    // 12: source id
    if props > 12 && c.present()? {
        for _ in 0..count {
            c.lps32()?;
        }
    }
    // 13: a 60-byte box per object
    if props > 13 && c.present()? {
        for _ in 0..count {
            c.bytes(60)?;
        }
    }
    // 14: read-only name (16-bit length)
    if props > 14 && c.present()? {
        for o in &mut objs {
            let n = c.lps16()?;
            if o.name.is_none() {
                o.name = Some(n);
            }
        }
    }
    if props > 15 && c.present()? {
        return c.err("the read-only affixes of objects (property 15) are not read");
    }
    // 16: read-only object name
    if props > 16 && c.present()? {
        for o in &mut objs {
            let n = c.lps32()?;
            if o.name.is_none() {
                o.name = Some(n);
            }
        }
    }
    // 17: affixes
    if props > 17 && c.present()? {
        for _ in 0..count {
            c.lps32()?;
        }
    }
    Ok(objs)
}

fn read_lod_roots(c: &mut Cur) -> Result<Vec<LodRoot>> {
    let count = c.u32()? as usize;
    if count > MAX_ITEMS {
        return c.err("too many level of detail roots");
    }
    let props = c.u32()?;
    let mut roots = vec![LodRoot::default(); count];
    if props > 0 && c.present()? {
        for r in &mut roots {
            let n = c.count32("object ids")?;
            for _ in 0..n {
                r.object_ids.push(c.u32()?);
            }
        }
    }
    if props > 1 && c.present()? {
        for r in &mut roots {
            let n = c.count32("max lod indices")?;
            for _ in 0..n {
                r.max_lod_indices.push(c.u32()?);
            }
        }
    }
    if props > 2 && c.present()? {
        for r in &mut roots {
            let n = c.count32("lod distances")?;
            let _p = c.u32()?;
            if c.present()? {
                for _ in 0..n {
                    r.max_distances.push(c.f32()?);
                }
            }
        }
    }
    if props > 3 && c.present()? {
        for r in &mut roots {
            let (a, b) = (c.vec3()?, c.vec3()?);
            r.bbox = Some([a[0], a[1], a[2], b[0], b[1], b[2]]);
        }
    }
    if props > 4 && c.present()? {
        for _ in 0..count {
            c.f32()?;
        }
    }
    if props > 5 && c.present()? {
        for _ in 0..count {
            c.u8()?;
        }
    }
    Ok(roots)
}

fn read_vbuffer_mapping(c: &mut Cur, g: &mut Geometry) -> Result<()> {
    let _props = c.i32()?;
    let flags = c.u8()?;
    if flags & 1 != 0 {
        // stream -> vertex buffer: index and offset per stream
        let count = c.count32("streams")?;
        let props = c.i32()?;
        let mut offsets = vec![0i32; count];
        if props > 0 && c.present()? {
            for _ in 0..count {
                c.i32()?;
            }
        }
        if props > 1 && c.present()? {
            for o in &mut offsets {
                *o = c.i32()?;
            }
        }
        g.stream_offsets = offsets;
        // vertex buffer info: size, a number to skip, flags
        let count = c.count32("vertex buffers")?;
        let props = c.i32()?;
        let mut sizes = vec![0i32; count];
        if props > 0 && c.present()? {
            for s in &mut sizes {
                *s = c.i32()?;
            }
        }
        if props > 1 && c.present()? {
            for _ in 0..count {
                c.i32()?;
            }
        }
        if props > 2 && c.present()? {
            return c.err("old vertex buffer flags are not read");
        }
        if props > 3 && c.present()? {
            for _ in 0..count {
                if c.u8()? != 0 {
                    c.u16()?;
                }
            }
        }
        g.stream_sizes = sizes;
    }
    Ok(())
}

fn read_geometry_data(c: &mut Cur, g: &mut Geometry) -> Result<()> {
    loop {
        let sentinel = c.u16()?;
        match sentinel {
            0x0000 => {
                let end = c.u32()? as usize;
                g.root_node = c.i16()?;
                g.node_count = c.i32()?;
                g.buffer_count = c.i32()?;
                g.mesh_count = c.i32()?;
                g.sub_mesh_count = c.i32()?;
                c.u32()?;
                c.u32()?;
                c.expect_pos(end, "geometry header")?;
            }
            0x0005 => {
                let end = c.u32()? as usize;
                if end < c.pos() || end > c.len() {
                    return c.err("the reference section ends outside the file");
                }
                c.set_pos(end);
            }
            0x0002 => read_buffers(c, g)?,
            0x0003 => read_meshes(c, g)?,
            0x0004 => read_sub_meshes(c, g)?,
            0xFFFF => {
                let end = c.u32()? as usize;
                return c.expect_pos(end, "geometry end");
            }
            other => return c.err(format!("unknown geometry section {other:#x}")),
        }
    }
}

fn read_buffers(c: &mut Cur, g: &mut Geometry) -> Result<()> {
    let section_end = c.i32()? as usize;
    let n = usize::try_from(g.buffer_count).map_err(|_| TplError { at: c.pos(), what: "a negative buffer count".to_string() })?;
    if n > MAX_ITEMS {
        return c.err("too many buffers");
    }
    let mut buffers = vec![Buffer::default(); n];
    while c.pos() < section_end {
        let id = c.u16()?;
        let end = c.u32()? as usize;
        match id {
            0 => {
                for b in &mut buffers {
                    b.flags = BitSet::read(c, 2)?;
                }
            }
            1 => {
                for b in &mut buffers {
                    b.stride = c.u16()?;
                }
            }
            2 => {
                let mut start = 0u64;
                for b in &mut buffers {
                    b.length = c.u32()?;
                    b.start = start;
                    start += u64::from(b.length);
                }
            }
            3 => {
                g.buffers_inline = true;
                for b in &mut buffers {
                    b.start = c.pos() as u64;
                    let to = c.pos() + b.length as usize;
                    if to > c.len() {
                        return c.err("a buffer reaches past the end of the file");
                    }
                    c.set_pos(to);
                }
            }
            _ => c.set_pos(end),
        }
        c.expect_pos(end, &format!("buffer chunk {id}"))?;
    }
    c.expect_pos(section_end, "buffer section")?;
    g.buffers = buffers;
    Ok(())
}

fn read_meshes(c: &mut Cur, g: &mut Geometry) -> Result<()> {
    let section_end = c.i32()? as usize;
    let n = usize::try_from(g.mesh_count).map_err(|_| TplError { at: c.pos(), what: "a negative mesh count".to_string() })?;
    if n > MAX_ITEMS {
        return c.err("too many meshes");
    }
    let mut meshes = vec![Mesh::default(); n];
    while c.pos() < section_end {
        let id = c.u16()?;
        let end = c.u32()? as usize;
        match id {
            0 => {
                for m in &mut meshes {
                    m.flags = BitSet::read(c, 2)?;
                }
            }
            2 => {
                for m in &mut meshes {
                    let k = c.u8()?;
                    for _ in 0..k {
                        let buffer = c.i32()?;
                        let offset = c.i32()?;
                        m.buffers.push((buffer, offset));
                    }
                }
            }
            other => return c.err(format!("unknown mesh chunk {other}")),
        }
        c.expect_pos(end, &format!("mesh chunk {id}"))?;
    }
    c.expect_pos(section_end, "mesh section")?;
    g.meshes = meshes;
    Ok(())
}

/// A list of named, typed values: a count, then for every property a name, a type number and the value. Types: 1 integer,
/// 2 float, 3 boolean (one byte), 4 text, 6 list (a count, then a type number and a value for each item), 7 nested list of
/// properties. `dynamic` = names have a length in front; else they end with a zero byte and are followed by a number.
fn read_property_list(c: &mut Cur, dynamic: bool) -> Result<Vec<(String, Value)>> {
    read_property_list_deep(c, dynamic, 0)
}

fn read_property_list_deep(c: &mut Cur, dynamic: bool, depth: usize) -> Result<Vec<(String, Value)>> {
    if depth > 8 {
        return c.err("property lists nested too deeply");
    }
    let count = c.u32()? as usize;
    if count > 4096 {
        return c.err(format!("a property list of {count} entries is not believable"));
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let name = if dynamic {
            c.lps32()?
        } else {
            let mut bytes = Vec::new();
            loop {
                let b = c.u8()?;
                if b == 0 {
                    break;
                }
                bytes.push(b);
                if bytes.len() > 1024 {
                    return c.err("a property name without an end");
                }
            }
            c.u32()?;
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let kind = c.u32()?;
        let value = read_typed_value(c, kind, dynamic, depth)?;
        out.push((name, value));
    }
    Ok(out)
}

fn read_typed_value(c: &mut Cur, kind: u32, dynamic: bool, depth: usize) -> Result<Value> {
    Ok(match kind {
        1 => Value::Int(c.i32()?),
        2 => Value::Float(c.f32()?),
        3 => Value::Bool(c.u8()? != 0),
        4 => Value::Text(c.lps32()?),
        6 => {
            let n = c.u32()? as usize;
            if n > 65536 {
                return c.err(format!("a list of {n} items is not believable"));
            }
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                let k = c.u32()?;
                items.push(read_typed_value(c, k, dynamic, depth + 1)?);
            }
            Value::List(items)
        }
        7 => Value::Class(read_property_list_deep(c, dynamic, depth + 1)?),
        other => return c.err(format!("unknown property type {other}")),
    })
}

fn read_sub_meshes(c: &mut Cur, g: &mut Geometry) -> Result<()> {
    let section_end = c.i32()? as usize;
    let n = usize::try_from(g.sub_mesh_count).map_err(|_| TplError { at: c.pos(), what: "a negative sub mesh count".to_string() })?;
    if n > MAX_ITEMS {
        return c.err("too many sub meshes");
    }
    let mut subs = vec![SubMesh::default(); n];
    while c.pos() < section_end {
        let id = c.u16()?;
        let end = c.u32()? as usize;
        match id {
            0 => {
                // 12 bytes each, and when the section is bigger than that, a flag word and pairs of floats per set flag
                let have = end.saturating_sub(c.pos());
                let extra = n > 0 && have / n > 12;
                for s in &mut subs {
                    s.vertex_offset = c.u16()?;
                    s.vertex_count = c.u16()?;
                    s.face_offset = c.u16()?;
                    s.face_count = c.u16()?;
                    s.node = c.i16()?;
                    s.skin_compound = c.i16()?;
                    if extra {
                        let flags = c.u32()?;
                        for bit in 0..32 {
                            if flags >> bit & 1 != 0 {
                                c.f32()?;
                                c.f32()?;
                            }
                        }
                    }
                }
            }
            1 => {
                for s in &mut subs {
                    s.mesh = c.i32()?;
                }
            }
            2 => {
                // render flags and a nested list; skipped by its own end offset (read below)
                c.set_pos(end);
            }
            3 => {
                for s in &mut subs {
                    let has = g.meshes.get(s.mesh as usize).is_some_and(|m| m.flags.get(MESH_HAS_BONE_IDS));
                    if !has {
                        continue;
                    }
                    let k = c.u8()?;
                    for _ in 0..k {
                        s.bone_ids.push(c.i16()?);
                    }
                }
            }
            4 => {
                for s in &mut subs {
                    let has = g.meshes.get(s.mesh as usize).is_some_and(|m| m.flags.get(MESH_HAS_UV_SCALING));
                    if !has {
                        continue;
                    }
                    let k = c.u8()?;
                    for _ in 0..k {
                        let set = c.u8()?;
                        let scale = c.i16()?;
                        s.uv_scaling.push((set, scale));
                    }
                }
            }
            5 => {
                for s in &mut subs {
                    let has = g.meshes.get(s.mesh as usize).is_some_and(|m| m.flags.get(MESH_HAS_TRANSFORM));
                    if !has {
                        continue;
                    }
                    let position = [c.i16()?, c.i16()?, c.i16()?];
                    let scale = [c.i16()?, c.i16()?, c.i16()?];
                    s.transform = Some((position, scale));
                }
            }
            // materials: for every sub mesh the node it belongs to and a list of named values. Chunk 6 is a text form that is not
            // read here; 7 and 8 are binary lists (7 with null-terminated names and a number in front of each type, 8 with
            // length-prefixed names), the usual one is 8
            6 => c.set_pos(end),
            7 | 8 => {
                for s in &mut subs {
                    s.node = c.u16()? as i16;
                    s.material = read_property_list(c, id == 8)?;
                }
            }
            other => return c.err(format!("unknown sub mesh chunk {other}")),
        }
        c.expect_pos(end, &format!("sub mesh chunk {id}"))?;
    }
    c.expect_pos(section_end, "sub mesh section")?;
    g.sub_meshes = subs;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::made_up_model;

    #[test]
    fn the_reader_never_reads_outside_its_slice() {
        let mut c = Cur::new(&[1, 2, 3]);
        assert_eq!(c.u16().unwrap(), 0x0201);
        assert!(c.u16().is_err());
        assert_eq!(c.pos(), 2, "a failed read does not move");
        assert!(Cur::new(&[]).u8().is_err());
        // a huge string length is refused, a negative one too
        let mut huge = Cur::new(&[0xFF, 0xFF, 0xFF, 0x7F, 1, 2, 3]);
        assert!(huge.lps32().is_err());
        let mut neg = Cur::new(&[0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(neg.lps32().is_err());
    }

    #[test]
    fn bit_sets_count_bits_from_the_low_end_of_the_first_byte() {
        // count 12 -> two bytes
        let bytes = [12u8, 0, 0, 0, 0x65, 0x0F];
        let set = BitSet::read(&mut Cur::new(&bytes), 4).unwrap();
        assert_eq!(set.count, 12);
        let on: Vec<usize> = (0..12).filter(|i| set.get(*i)).collect();
        assert_eq!(on, vec![0, 2, 5, 6, 8, 9, 10, 11]);
        assert_eq!(set.low64(), 0xF65);
        let short = BitSet::read(&mut Cur::new(&[3, 0, 0b101]), 2).unwrap();
        assert!(short.get(0) && !short.get(1) && short.get(2) && !short.get(3) && !short.get(99));
    }

    #[test]
    fn a_made_up_model_reads_completely_and_decodes_to_the_square() {
        let (tpl, data) = made_up_model();
        let t = Template::parse(&tpl).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(t.name.as_deref(), Some("made_up_square"));
        assert_eq!(t.end_at, tpl.len(), "everything was read");
        let g = t.geometry.as_ref().unwrap();
        assert_eq!((g.buffers.len(), g.meshes.len(), g.sub_meshes.len()), (4, 1, 1));
        assert_eq!(g.buffers.iter().map(|b| (b.stride, b.length, b.start)).collect::<Vec<_>>(), vec![(8, 32, 0), (6, 12, 32), (8, 32, 44), (4, 16, 76)]);
        let s = &g.sub_meshes[0];
        assert_eq!((s.vertex_count, s.face_count, s.skin_compound, s.bone_ids.clone()), (4, 2, -1, vec![7, 9]));
        assert_eq!(s.uv_scaling, vec![(0, 2)]);
        assert_eq!(s.transform, Some(([0, 0, 1], [2, 2, 2])));
        assert_eq!(s.material, vec![("shadingMtl_Tex".to_string(), Value::Text("square_tex".to_string())), ("tiling".to_string(), Value::Float(1.5))]);
        assert_eq!(s.material[0].1.show(), "\"square_tex\"");
        assert_eq!((s.texture_name(), s.shader_name()), (Some("square_tex"), None));
        let m = crate::mesh::decode_sub_mesh(g, &data, 0).unwrap_or_else(|e| panic!("{e}"));
        // positions: raw / 32767 * scale 2 + position (0, 0, 1)
        assert_eq!(m.positions, vec![[-2.0, -2.0, 1.0], [2.0, -2.0, 1.0], [2.0, 2.0, 1.0], [-2.0, 2.0, 1.0]]);
        assert_eq!(m.triangles, vec![[0, 1, 2], [0, 2, 3]]);
        assert_eq!(m.uvs.len(), 4);
        assert_eq!(m.uvs[0], [0.0, 1.0]);
        assert_eq!(m.uvs[2], [2.0, -1.0], "texture coordinates are scaled by the sub mesh's uv scale (2) and v is flipped");
        assert_eq!(m.tangents[0], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(m.bones, vec![[0, 0, 0, 0], [1, 0, 0, 0], [2, 0, 0, 0], [3, 0, 0, 0]]);
        assert_eq!(m.normals.len(), 4);
    }

    #[test]
    fn damaged_copies_of_the_made_up_model_are_errors_not_crashes() {
        let (tpl, data) = made_up_model();
        // every cut-off copy either fails with a message or (for a cut after the last chunk) reads as much as it can
        for cut in (0..tpl.len()).step_by(7) {
            let (t, err) = Template::parse_partial(&tpl[..cut]);
            if cut < tpl.len() {
                assert!(err.is_some(), "a copy cut at {cut} should not read completely");
            }
            let _ = t;
        }
        // flipped bytes never panic (decoding included)
        for i in (0x40..tpl.len()).step_by(3) {
            let mut x = tpl.clone();
            x[i] ^= 0xA5;
            let (t, _) = Template::parse_partial(&x);
            if let Some(g) = &t.geometry {
                for k in 0..g.sub_meshes.len() {
                    let _ = crate::mesh::decode_sub_mesh(g, &data, k);
                }
            }
        }
        // a short data file is an error
        let t = Template::parse(&tpl).unwrap();
        assert!(crate::mesh::decode_sub_mesh(t.geometry.as_ref().unwrap(), &data[..60], 0).is_err());
        assert!(crate::mesh::decode_sub_mesh(t.geometry.as_ref().unwrap(), &data, 5).is_err());
    }

    #[test]
    fn a_file_that_is_not_a_template_is_refused_with_a_reason() {
        let e = Template::parse(b"not a template at all, just text that is long enough to have a header in it, really").unwrap_err();
        assert!(e.what.contains("1SER"), "{e}");
        let mut other = b"1SERgeom_dbg".to_vec();
        other.resize(0x60, 0);
        let e = Template::parse(&other).unwrap_err();
        assert!(e.what.contains("not a template"), "{e}");
        assert!(Template::parse(&[]).is_err());
    }
    #[test]
    fn the_full_detail_model_is_the_sub_mesh_of_the_level_zero_object() {
        let sub = |node: i16| SubMesh { node, ..SubMesh::default() };
        let mut t = Template { lods: vec![LodDef { object: 52, index: 1, last_lod_up_to_infinity: false }, LodDef { object: 51, index: 0, last_lod_up_to_infinity: false }], ..Template::default() };
        assert!(t.full_detail_sub_meshes().is_empty(), "no geometry: nothing");
        t.geometry = Some(Geometry { sub_meshes: vec![sub(10), sub(51), sub(52), sub(51)], ..Geometry::default() });
        assert_eq!(t.full_detail_sub_meshes(), vec![1, 3], "every sub mesh of the object that level 0 names, in file order");
        t.lods.retain(|l| l.index != 0);
        assert!(t.full_detail_sub_meshes().is_empty(), "no level 0 definition: nothing is guessed");
    }

}
