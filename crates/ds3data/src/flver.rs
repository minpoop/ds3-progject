//! `FLVER` (version 2): the model format of Dark Souls III (`wp_a_0200.flver` inside `wp_a_0200.partsbnd.dcx`, characters,
//! maps). Only little-endian files of the Dark Souls III versions (`0x20013`, `0x20014`) are read.
//!
//! Layout as documented by the Souls modding community (all numbers little endian):
//!
//! ```text
//! 0x00 "FLVER\0" "L\0"   0x08 version   0x0C data offset   0x10 data size
//! 0x14 counts: dummies, materials, bones, meshes, vertex buffers   0x28 bounding box (6 floats)
//! 0x40 face count, total face count   0x48 u8 index size (0 in DS3), unicode, unk4A, unk4B   0x4C i32
//! 0x50 counts: face sets, buffer layouts, textures   0x5C u8, u8, 2 zero bytes   0x60 two zero words
//! 0x68 i16 unk68, i16 special modifier   0x6C, 0x70 zero   0x74 i32   0x78, 0x7C zero
//! 0x80 arrays of fixed-size records: dummies (0x40), materials (0x20), bones (0x80), meshes (0x30), face sets (0x20),
//!      vertex buffers (0x20), buffer layouts (0x10), textures (0x20); then the layout members (0x14 each), the mesh
//!      bounding boxes, bone index lists, face set / vertex buffer index lists, the GX lists, the strings (UTF-16),
//!      and at the data offset the face indices and the vertex buffers
//! ```
//!
//! [`Flver::write`] writes the same order again. For a file that was read, `write(parse(bytes))` is expected to give the
//! same bytes (the self-check of `ds3-probe` says whether it does); the vertex buffers are kept as raw bytes, so a model
//! whose geometry is not touched is written back exactly. The vertex format itself is described by the buffer layouts.
use crate::util::{get, i32_le, u16_le};
use std::fmt;

pub const HEADER_LEN: usize = 0x80;
const MAX_ITEMS: usize = 1 << 20;
const MAX_STRING_UNITS: usize = 1024;
const DUMMY_LEN: usize = 0x40;
const MATERIAL_LEN: usize = 0x20;
const NODE_LEN: usize = 0x80;
const MESH_LEN: usize = 0x30;
const FACE_SET_LEN: usize = 0x20;
const VERTEX_BUFFER_LEN: usize = 0x20;
const LAYOUT_LEN: usize = 0x10;
const TEXTURE_LEN: usize = 0x20;
const MEMBER_LEN: usize = 0x14;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlverError {
    /// The data does not start with `FLVER\0`.
    NotFlver,
    /// A big-endian file (console games).
    BigEndian,
    /// A version, layout type or feature this reader does not handle.
    Unsupported(String),
    Truncated { what: &'static str },
    Malformed(String),
}

impl fmt::Display for FlverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlverError::NotFlver => write!(f, "not a FLVER file"),
            FlverError::BigEndian => write!(f, "a big-endian FLVER (not the PC format)"),
            FlverError::Unsupported(what) => write!(f, "not supported: {what}"),
            FlverError::Truncated { what } => write!(f, "the FLVER file is cut off ({what})"),
            FlverError::Malformed(why) => write!(f, "damaged FLVER file: {why}"),
        }
    }
}

impl std::error::Error for FlverError {}

type Result<T> = std::result::Result<T, FlverError>;

fn bad<T>(why: impl Into<String>) -> Result<T> {
    Err(FlverError::Malformed(why.into()))
}

// ------------------------------------------------------------------------------------------------ a reader

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn at(b: &'a [u8], p: usize) -> Rd<'a> {
        Rd { b, p }
    }

    fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8]> {
        let s = get(self.b, self.p, n).ok_or(FlverError::Truncated { what })?;
        self.p += n;
        Ok(s)
    }

    fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.take(1, what)?[0])
    }

    fn i16(&mut self, what: &'static str) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take(2, what)?.try_into().map_err(|_| FlverError::Truncated { what })?))
    }

    fn i32(&mut self, what: &'static str) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4, what)?.try_into().map_err(|_| FlverError::Truncated { what })?))
    }

    fn u32(&mut self, what: &'static str) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4, what)?.try_into().map_err(|_| FlverError::Truncated { what })?))
    }

    fn f32(&mut self, what: &'static str) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4, what)?.try_into().map_err(|_| FlverError::Truncated { what })?))
    }

    fn v3(&mut self, what: &'static str) -> Result<[f32; 3]> {
        Ok([self.f32(what)?, self.f32(what)?, self.f32(what)?])
    }

    fn zero_i32(&mut self, what: &'static str) -> Result<()> {
        if self.i32(what)? != 0 {
            return bad(format!("{what}: a field that is always zero is not"));
        }
        Ok(())
    }
}

/// A UTF-16 string ending in a zero, starting at `at`.
fn utf16_at(b: &[u8], at: i64, what: &'static str) -> Result<String> {
    let at = usize::try_from(at).map_err(|_| FlverError::Malformed(format!("{what}: a negative string offset")))?;
    let mut units = Vec::new();
    let mut p = at;
    loop {
        let u = u16_le(b, p).ok_or(FlverError::Truncated { what })?;
        if u == 0 {
            break;
        }
        units.push(u);
        if units.len() > MAX_STRING_UNITS {
            return bad(format!("{what}: a string without an end"));
        }
        p += 2;
    }
    Ok(String::from_utf16_lossy(&units))
}

fn count(v: i32, what: &'static str) -> Result<usize> {
    match usize::try_from(v) {
        Ok(n) if n <= MAX_ITEMS => Ok(n),
        _ => bad(format!("{what}: a count of {v} is not believable")),
    }
}

// ------------------------------------------------------------------------------------------------ the model

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub version: i32,
    pub bounding_box_min: [f32; 3],
    pub bounding_box_max: [f32; 3],
    pub unicode: bool,
    pub unk4a: bool,
    pub unk4b: bool,
    pub unk4c: i32,
    pub unk5c: u8,
    pub unk5d: u8,
    pub unk68: i16,
    pub special_modifier: i16,
    pub unk74: i32,
    /// The face counts the file stated (the writer recomputes them from the face sets).
    pub face_count: i32,
    pub total_face_count: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dummy {
    pub position: [f32; 3],
    /// The four colour bytes as stored.
    pub color: [u8; 4],
    pub forward: [f32; 3],
    pub reference_id: i16,
    pub parent_bone: i16,
    pub upward: [f32; 3],
    pub attach_bone: i16,
    pub flag1: bool,
    pub use_upward_vector: bool,
    pub unk30: i32,
    pub unk34: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// The shader parameter this texture is for (`g_DiffuseTexture`, `g_BumpmapTexture`, ...).
    pub param_name: String,
    /// The path the texture was exported from; its file name (without extension) names the texture inside the `.tpf`.
    pub path: String,
    pub scale: [f32; 2],
    pub tiling_u: u8,
    pub tiling_v: u8,
    pub unk14: f32,
    pub unk18: f32,
    pub unk1c: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GxItem {
    pub id: String,
    pub unk04: i32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GxList {
    pub items: Vec<GxItem>,
    pub terminator_id: i32,
    pub terminator_length: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    /// The material definition file (`N:\FDP\mtd\...`), which decides the shader and the textures it wants.
    pub mtd: String,
    pub textures: Vec<Texture>,
    /// Index into [`Flver::gx_lists`], or -1.
    pub gx_index: i32,
    pub index: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub parent: i16,
    pub child: i16,
    pub next_sibling: i16,
    pub previous_sibling: i16,
    pub bounding_box_min: [f32; 3],
    pub unk3c: i32,
    pub bounding_box_max: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct FaceSet {
    /// Bit flags: 0x01000000 / 0x02000000 / 0x04000000 level of detail, 0x80000000 motion blur.
    pub flags: u32,
    pub triangle_strip: bool,
    pub cull_backfaces: bool,
    pub unk06: i16,
    pub indices: Vec<u32>,
    /// 16 or 32 bits per index in the file.
    pub index_bits: u8,
}

/// One vertex stream of a mesh: its layout and the raw bytes (`vertex_size` bytes per vertex).
#[derive(Debug, Clone, PartialEq)]
pub struct VertexBuffer {
    pub layout_index: i32,
    pub vertex_size: i32,
    pub vertex_count: i32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundingBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    /// Vertices are skinned to several bones (the file's "dynamic" byte).
    pub dynamic: u8,
    pub material_index: i32,
    pub node_index: i32,
    pub bone_indices: Vec<i32>,
    pub bounding_box: Option<BoundingBox>,
    pub face_sets: Vec<FaceSet>,
    pub vertex_buffers: Vec<VertexBuffer>,
}

/// How one value of a vertex is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutMember {
    pub unk00: i32,
    pub struct_offset: i32,
    pub ty: u32,
    pub semantic: u32,
    pub index: i32,
}

/// The number of bytes a layout type takes, or `None` for a type this reader does not know.
pub fn type_size(ty: u32) -> Option<usize> {
    Some(match ty {
        0x01 => 8,                    // Float2
        0x02 => 12,                   // Float3
        0x03 => 16,                   // Float4
        0x10 | 0x11 | 0x13 => 4,      // Byte4A, Byte4B, Byte4C
        0x12 | 0x15 => 4,             // Short2toFloat2, UV
        0x16 => 8,                    // UVPair
        0x18 | 0x1A | 0x2E => 8,      // ShortBoneIndices, Short4toFloat4A, Short4toFloat4B
        0x2F => 4,                    // Byte4E
        _ => return None,
    })
}

pub fn type_name(ty: u32) -> String {
    match ty {
        0x01 => "Float2",
        0x02 => "Float3",
        0x03 => "Float4",
        0x10 => "Byte4A",
        0x11 => "Byte4B",
        0x12 => "Short2toFloat2",
        0x13 => "Byte4C",
        0x15 => "UV",
        0x16 => "UVPair",
        0x18 => "ShortBoneIndices",
        0x1A => "Short4toFloat4A",
        0x2E => "Short4toFloat4B",
        0x2F => "Byte4E",
        0xF0 => "EdgeCompressed",
        other => return format!("type {other:#x}"),
    }
    .to_string()
}

pub fn semantic_name(semantic: u32) -> String {
    match semantic {
        0 => "Position",
        1 => "BoneWeights",
        2 => "BoneIndices",
        3 => "Normal",
        5 => "UV",
        6 => "Tangent",
        7 => "Bitangent",
        10 => "VertexColor",
        other => return format!("semantic {other}"),
    }
    .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    pub members: Vec<LayoutMember>,
}

impl Layout {
    /// The bytes of one vertex, or `None` when a member has a type this reader does not know.
    pub fn size(&self) -> Option<usize> {
        self.members.iter().map(|m| type_size(m.ty)).sum()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Flver {
    pub header: Header,
    pub dummies: Vec<Dummy>,
    pub materials: Vec<Material>,
    pub gx_lists: Vec<GxList>,
    pub nodes: Vec<Node>,
    pub meshes: Vec<Mesh>,
    pub layouts: Vec<Layout>,
}

// ------------------------------------------------------------------------------------------------ reading

fn read_gx_list(b: &[u8], at: usize) -> Result<GxList> {
    let mut p = at;
    let mut items = Vec::new();
    loop {
        let id = i32_le(b, p).ok_or(FlverError::Truncated { what: "a GX list" })?;
        if id == i32::MAX || id == -1 {
            break;
        }
        if items.len() > 4096 {
            return bad("a GX list that never ends");
        }
        let tag = get(b, p, 4).ok_or(FlverError::Truncated { what: "a GX item" })?;
        let unk04 = i32_le(b, p + 4).ok_or(FlverError::Truncated { what: "a GX item" })?;
        let length = i32_le(b, p + 8).ok_or(FlverError::Truncated { what: "a GX item" })?;
        let Some(data_len) = usize::try_from(length).ok().and_then(|l| l.checked_sub(0xC)) else { return bad("a GX item with an impossible length") };
        let data = get(b, p + 12, data_len).ok_or(FlverError::Truncated { what: "GX item data" })?.to_vec();
        items.push(GxItem { id: String::from_utf8_lossy(tag).trim_end_matches('\0').to_string(), unk04, data });
        p += 12 + data_len;
    }
    let terminator_id = i32_le(b, p).ok_or(FlverError::Truncated { what: "a GX list end" })?;
    if i32_le(b, p + 4) != Some(100) {
        return bad("a GX list terminator without the number 100");
    }
    let length = i32_le(b, p + 8).ok_or(FlverError::Truncated { what: "a GX list end" })?;
    let terminator_length = length.checked_sub(0xC).filter(|l| *l >= 0).ok_or(FlverError::Malformed("a GX list terminator with an impossible length".to_string()))?;
    let pad = get(b, p + 12, terminator_length as usize).ok_or(FlverError::Truncated { what: "a GX list end" })?;
    if pad.iter().any(|x| *x != 0) {
        return bad("a GX list terminator with non-zero padding");
    }
    Ok(GxList { items, terminator_id, terminator_length })
}

impl Flver {
    /// Reads a FLVER file.
    pub fn parse(b: &[u8]) -> Result<Flver> {
        if b.len() < HEADER_LEN || &b[..6] != b"FLVER\0" {
            return Err(FlverError::NotFlver);
        }
        match &b[6..8] {
            b"L\0" => {}
            b"B\0" => return Err(FlverError::BigEndian),
            _ => return Err(FlverError::NotFlver),
        }
        let mut r = Rd::at(b, 8);
        let version = r.i32("version")?;
        if version != 0x20013 && version != 0x20014 {
            return Err(FlverError::Unsupported(format!("FLVER version {version:#x} (only the Dark Souls III versions 0x20013 and 0x20014 are read)")));
        }
        let data_offset = r.i32("data offset")?;
        let _data_size = r.i32("data size")?;
        let dummy_count = count(r.i32("dummy count")?, "dummies")?;
        let material_count = count(r.i32("material count")?, "materials")?;
        let node_count = count(r.i32("bone count")?, "bones")?;
        let mesh_count = count(r.i32("mesh count")?, "meshes")?;
        let vertex_buffer_count = count(r.i32("vertex buffer count")?, "vertex buffers")?;
        let bounding_box_min = r.v3("bounding box")?;
        let bounding_box_max = r.v3("bounding box")?;
        let face_count = r.i32("face count")?;
        let total_face_count = r.i32("total face count")?;
        let header_index_size = r.u8("index size")?;
        if !matches!(header_index_size, 0 | 8 | 16 | 32) {
            return bad(format!("the header index size {header_index_size} is not one of 0, 8, 16, 32"));
        }
        let unicode = r.u8("unicode flag")? != 0;
        let unk4a = r.u8("unk4A")? != 0;
        let unk4b = r.u8("unk4B")? != 0;
        let unk4c = r.i32("unk4C")?;
        let face_set_count = count(r.i32("face set count")?, "face sets")?;
        let layout_count = count(r.i32("layout count")?, "buffer layouts")?;
        let texture_count = count(r.i32("texture count")?, "textures")?;
        let unk5c = r.u8("unk5C")?;
        let unk5d = r.u8("unk5D")?;
        if r.u8("padding")? != 0 || r.u8("padding")? != 0 {
            return bad("padding bytes in the header are not zero");
        }
        r.zero_i32("header")?;
        r.zero_i32("header")?;
        let unk68 = r.i16("unk68")?;
        let special_modifier = r.i16("special modifier")?;
        if special_modifier != 0 {
            return Err(FlverError::Unsupported("a FLVER with a special modifier (speed tree)".to_string()));
        }
        r.zero_i32("header")?;
        r.zero_i32("header")?;
        let unk74 = r.i32("unk74")?;
        r.zero_i32("header")?;
        r.zero_i32("header")?;
        if !unicode {
            return Err(FlverError::Unsupported("a FLVER with Shift-JIS strings (Dark Souls III files use UTF-16)".to_string()));
        }
        let data_offset = usize::try_from(data_offset).map_err(|_| FlverError::Malformed("a negative data offset".to_string()))?;

        // the arrays of records, one after the other
        let mut p = HEADER_LEN;
        let mut take = |n: usize, len: usize, what: &'static str| -> Result<usize> {
            let start = p;
            let total = n.checked_mul(len).and_then(|t| start.checked_add(t)).ok_or(FlverError::Truncated { what })?;
            if total > b.len() {
                return Err(FlverError::Truncated { what });
            }
            p = total;
            Ok(start)
        };
        let dummies_at = take(dummy_count, DUMMY_LEN, "dummies")?;
        let materials_at = take(material_count, MATERIAL_LEN, "materials")?;
        let nodes_at = take(node_count, NODE_LEN, "bones")?;
        let meshes_at = take(mesh_count, MESH_LEN, "meshes")?;
        let face_sets_at = take(face_set_count, FACE_SET_LEN, "face sets")?;
        let vertex_buffers_at = take(vertex_buffer_count, VERTEX_BUFFER_LEN, "vertex buffers")?;
        let layouts_at = take(layout_count, LAYOUT_LEN, "buffer layouts")?;
        let textures_at = take(texture_count, TEXTURE_LEN, "textures")?;

        let mut dummies = Vec::with_capacity(dummy_count);
        for i in 0..dummy_count {
            let mut r = Rd::at(b, dummies_at + i * DUMMY_LEN);
            let position = r.v3("a dummy")?;
            let color: [u8; 4] = r.take(4, "a dummy")?.try_into().map_err(|_| FlverError::Truncated { what: "a dummy" })?;
            let forward = r.v3("a dummy")?;
            let reference_id = r.i16("a dummy")?;
            let parent_bone = r.i16("a dummy")?;
            let upward = r.v3("a dummy")?;
            let attach_bone = r.i16("a dummy")?;
            let flag1 = r.u8("a dummy")? != 0;
            let use_upward_vector = r.u8("a dummy")? != 0;
            let unk30 = r.i32("a dummy")?;
            let unk34 = r.i32("a dummy")?;
            r.zero_i32("a dummy")?;
            r.zero_i32("a dummy")?;
            dummies.push(Dummy { position, color, forward, reference_id, parent_bone, upward, attach_bone, flag1, use_upward_vector, unk30, unk34 });
        }

        // textures first (materials take a run of them)
        let mut textures = Vec::with_capacity(texture_count);
        for i in 0..texture_count {
            let mut r = Rd::at(b, textures_at + i * TEXTURE_LEN);
            let path_offset = r.i32("a texture")?;
            let type_offset = r.i32("a texture")?;
            let scale = [r.f32("a texture")?, r.f32("a texture")?];
            let tiling_u = r.u8("a texture")?;
            let tiling_v = r.u8("a texture")?;
            if r.u8("a texture")? != 0 || r.u8("a texture")? != 0 {
                return bad("padding bytes of a texture are not zero");
            }
            let (unk14, unk18, unk1c) = (r.f32("a texture")?, r.f32("a texture")?, r.f32("a texture")?);
            textures.push(Texture {
                param_name: utf16_at(b, i64::from(type_offset), "a texture type")?,
                path: utf16_at(b, i64::from(path_offset), "a texture path")?,
                scale,
                tiling_u,
                tiling_v,
                unk14,
                unk18,
                unk1c,
            });
        }

        let mut gx_lists: Vec<GxList> = Vec::new();
        let mut gx_offsets: Vec<usize> = Vec::new();
        let mut materials = Vec::with_capacity(material_count);
        let mut used_textures = 0usize;
        for i in 0..material_count {
            let mut r = Rd::at(b, materials_at + i * MATERIAL_LEN);
            let name_offset = r.i32("a material")?;
            let mtd_offset = r.i32("a material")?;
            let tex_count = count(r.i32("a material")?, "material textures")?;
            let tex_index = count(r.i32("a material")?, "material texture index")?;
            let _string_bytes = r.i32("a material")?;
            let gx_offset = r.i32("a material")?;
            let index = r.i32("a material")?;
            r.zero_i32("a material")?;
            let Some(run) = tex_index.checked_add(tex_count).and_then(|end| textures.get(tex_index..end)) else { return bad("a material refers to textures that do not exist") };
            used_textures += run.len();
            let gx_index = if gx_offset == 0 {
                -1
            } else {
                let at = usize::try_from(gx_offset).map_err(|_| FlverError::Malformed("a negative GX offset".to_string()))?;
                match gx_offsets.iter().position(|o| *o == at) {
                    Some(k) => k as i32,
                    None => {
                        gx_lists.push(read_gx_list(b, at)?);
                        gx_offsets.push(at);
                        (gx_lists.len() - 1) as i32
                    }
                }
            };
            materials.push(Material { name: utf16_at(b, i64::from(name_offset), "a material name")?, mtd: utf16_at(b, i64::from(mtd_offset), "a material file")?, textures: run.to_vec(), gx_index, index });
        }
        if used_textures != texture_count {
            return bad("some textures belong to no material");
        }

        let mut nodes = Vec::with_capacity(node_count);
        for i in 0..node_count {
            let mut r = Rd::at(b, nodes_at + i * NODE_LEN);
            let position = r.v3("a bone")?;
            let name_offset = r.i32("a bone")?;
            let rotation = r.v3("a bone")?;
            let parent = r.i16("a bone")?;
            let child = r.i16("a bone")?;
            let scale = r.v3("a bone")?;
            let next_sibling = r.i16("a bone")?;
            let previous_sibling = r.i16("a bone")?;
            let bounding_box_min = r.v3("a bone")?;
            let unk3c = r.i32("a bone")?;
            let bounding_box_max = r.v3("a bone")?;
            let pad = r.take(0x34, "a bone")?;
            if pad.iter().any(|x| *x != 0) {
                return bad("the padding of a bone is not zero");
            }
            nodes.push(Node {
                name: utf16_at(b, i64::from(name_offset), "a bone name")?,
                position,
                rotation,
                scale,
                parent,
                child,
                next_sibling,
                previous_sibling,
                bounding_box_min,
                unk3c,
                bounding_box_max,
            });
        }

        // the global lists of face sets and vertex buffers
        let mut face_sets = Vec::with_capacity(face_set_count);
        for i in 0..face_set_count {
            let mut r = Rd::at(b, face_sets_at + i * FACE_SET_LEN);
            let flags = r.u32("a face set")?;
            let triangle_strip = r.u8("a face set")? != 0;
            let cull_backfaces = r.u8("a face set")? != 0;
            let unk06 = r.i16("a face set")?;
            let index_count = count(r.i32("a face set")?, "face indices")?;
            let indices_offset = r.i32("a face set")?;
            let _indices_length = r.i32("a face set")?;
            r.zero_i32("a face set")?;
            let own_size = r.i32("a face set")?;
            r.zero_i32("a face set")?;
            let bits = if own_size == 0 { header_index_size } else { own_size as u8 };
            if !matches!(own_size, 0 | 16 | 32) || !matches!(bits, 16 | 32) {
                return Err(FlverError::Unsupported(format!("a face set with {own_size}-bit indices (header {header_index_size})")));
            }
            let start = usize::try_from(indices_offset).ok().and_then(|o| data_offset.checked_add(o)).ok_or(FlverError::Malformed("a face set with an impossible offset".to_string()))?;
            let width = usize::from(bits / 8);
            let raw = index_count.checked_mul(width).and_then(|n| get(b, start, n)).ok_or(FlverError::Truncated { what: "face indices" })?;
            let indices: Vec<u32> = if bits == 16 { raw.chunks_exact(2).map(|c| u32::from(u16::from_le_bytes([c[0], c[1]]))).collect() } else { raw.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() };
            face_sets.push(FaceSet { flags, triangle_strip, cull_backfaces, unk06, indices, index_bits: bits });
        }

        // layouts (the vertex buffers need their sizes)
        let mut layouts = Vec::with_capacity(layout_count);
        for i in 0..layout_count {
            let mut r = Rd::at(b, layouts_at + i * LAYOUT_LEN);
            let member_count = count(r.i32("a layout")?, "layout members")?;
            r.zero_i32("a layout")?;
            r.zero_i32("a layout")?;
            let members_at = usize::try_from(r.i32("a layout")?).map_err(|_| FlverError::Malformed("a layout with a negative offset".to_string()))?;
            let mut members = Vec::with_capacity(member_count);
            let mut offset = 0i32;
            for k in 0..member_count {
                let mut r = Rd::at(b, members_at + k * MEMBER_LEN);
                let unk00 = r.i32("a layout member")?;
                let struct_offset = r.i32("a layout member")?;
                let ty = r.u32("a layout member")?;
                let semantic = r.u32("a layout member")?;
                let index = r.i32("a layout member")?;
                let Some(size) = type_size(ty) else { return Err(FlverError::Unsupported(format!("a vertex member of {}", type_name(ty)))) };
                if struct_offset != offset {
                    return bad(format!("a layout member at offset {struct_offset} where {offset} was expected"));
                }
                offset += size as i32;
                members.push(LayoutMember { unk00, struct_offset, ty, semantic, index });
            }
            layouts.push(Layout { members });
        }

        let mut vertex_buffers = Vec::with_capacity(vertex_buffer_count);
        for i in 0..vertex_buffer_count {
            let mut r = Rd::at(b, vertex_buffers_at + i * VERTEX_BUFFER_LEN);
            let buffer_index = r.i32("a vertex buffer")?;
            if buffer_index & 0x6000_0000 != 0 {
                return Err(FlverError::Unsupported("an edge-compressed vertex buffer (console models)".to_string()));
            }
            let layout_index = r.i32("a vertex buffer")?;
            let vertex_size = r.i32("a vertex buffer")?;
            let vertex_count = r.i32("a vertex buffer")?;
            r.zero_i32("a vertex buffer")?;
            r.zero_i32("a vertex buffer")?;
            let _buffer_length = r.i32("a vertex buffer")?;
            let buffer_offset = r.i32("a vertex buffer")?;
            let Some(layout) = usize::try_from(layout_index).ok().and_then(|l| layouts.get(l)) else { return bad("a vertex buffer refers to a layout that does not exist") };
            if layout.size() != usize::try_from(vertex_size).ok() {
                return bad(format!("a vertex buffer of {vertex_size} bytes per vertex with a layout of {:?}", layout.size()));
            }
            let start = usize::try_from(buffer_offset).ok().and_then(|o| data_offset.checked_add(o)).ok_or(FlverError::Malformed("a vertex buffer with an impossible offset".to_string()))?;
            let n = usize::try_from(vertex_count).ok().filter(|n| *n <= MAX_ITEMS * 16).ok_or(FlverError::Malformed("a vertex buffer with an impossible count".to_string()))?;
            let data = (vertex_size as usize).checked_mul(n).and_then(|len| get(b, start, len)).ok_or(FlverError::Truncated { what: "vertex data" })?.to_vec();
            vertex_buffers.push(VertexBuffer { layout_index, vertex_size, vertex_count, data });
        }

        let mut meshes = Vec::with_capacity(mesh_count);
        for i in 0..mesh_count {
            let mut r = Rd::at(b, meshes_at + i * MESH_LEN);
            let dynamic = r.u8("a mesh")?;
            if r.u8("a mesh")? != 0 || r.u8("a mesh")? != 0 || r.u8("a mesh")? != 0 {
                return bad("padding bytes of a mesh are not zero");
            }
            let material_index = r.i32("a mesh")?;
            r.zero_i32("a mesh")?;
            r.zero_i32("a mesh")?;
            let node_index = r.i32("a mesh")?;
            let bone_count = count(r.i32("a mesh")?, "mesh bones")?;
            let box_offset = r.i32("a mesh")?;
            let bone_offset = r.i32("a mesh")?;
            let face_set_n = count(r.i32("a mesh")?, "mesh face sets")?;
            let face_set_offset = r.i32("a mesh")?;
            let vb_n = count(r.i32("a mesh")?, "mesh vertex buffers")?;
            let vb_offset = r.i32("a mesh")?;
            let ints = |offset: i32, n: usize, what: &'static str| -> Result<Vec<i32>> {
                let at = usize::try_from(offset).map_err(|_| FlverError::Malformed(format!("{what}: a negative offset")))?;
                let raw = n.checked_mul(4).and_then(|len| get(b, at, len)).ok_or(FlverError::Truncated { what })?;
                Ok(raw.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
            };
            let bone_indices = ints(bone_offset, bone_count, "mesh bone indices")?;
            let fs_indices = ints(face_set_offset, face_set_n, "mesh face set indices")?;
            let vb_indices = ints(vb_offset, vb_n, "mesh vertex buffer indices")?;
            let bounding_box = if box_offset == 0 {
                None
            } else {
                let mut r = Rd::at(b, usize::try_from(box_offset).map_err(|_| FlverError::Malformed("a negative box offset".to_string()))?);
                Some(BoundingBox { min: r.v3("a mesh box")?, max: r.v3("a mesh box")? })
            };
            let mut own_face_sets = Vec::with_capacity(fs_indices.len());
            for k in fs_indices {
                let Some(fs) = usize::try_from(k).ok().and_then(|k| face_sets.get(k)) else { return bad("a mesh refers to a face set that does not exist") };
                own_face_sets.push(fs.clone());
            }
            let mut own_buffers = Vec::with_capacity(vb_indices.len());
            for k in vb_indices {
                let Some(vb) = usize::try_from(k).ok().and_then(|k| vertex_buffers.get(k)) else { return bad("a mesh refers to a vertex buffer that does not exist") };
                own_buffers.push(vb.clone());
            }
            meshes.push(Mesh { dynamic, material_index, node_index, bone_indices, bounding_box, face_sets: own_face_sets, vertex_buffers: own_buffers });
        }

        Ok(Flver {
            header: Header { version, bounding_box_min, bounding_box_max, unicode, unk4a, unk4b, unk4c, unk5c, unk5d, unk68, special_modifier, unk74, face_count, total_face_count },
            dummies,
            materials,
            gx_lists,
            nodes,
            meshes,
            layouts,
        })
    }
}

// ------------------------------------------------------------------------------------------------ writing

fn put_i32(o: &mut [u8], at: usize, v: i32) {
    o[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// Stores the current length of `o` (where the next thing will be written) at `at`.
fn put_here(o: &mut [u8], at: usize, here: usize) {
    put_i32(o, at, here as i32);
}

fn push_i32(o: &mut Vec<u8>, v: i32) {
    o.extend(v.to_le_bytes());
}

fn push_f32(o: &mut Vec<u8>, v: f32) {
    o.extend(v.to_le_bytes());
}

fn push_v3(o: &mut Vec<u8>, v: [f32; 3]) {
    for x in v {
        push_f32(o, x);
    }
}

fn pad(o: &mut Vec<u8>, align: usize) {
    while !o.len().is_multiple_of(align) {
        o.push(0);
    }
}

fn push_utf16(o: &mut Vec<u8>, s: &str) {
    for u in s.encode_utf16() {
        o.extend(u.to_le_bytes());
    }
    o.extend([0, 0]);
}

/// The size of one string block of a material (name, file, and the parameter names and paths of its textures), in bytes.
fn material_string_bytes(m: &Material) -> i32 {
    let mut chars = m.name.encode_utf16().count() + 1 + m.mtd.encode_utf16().count() + 1;
    for t in &m.textures {
        chars += t.param_name.encode_utf16().count() + 1 + t.path.encode_utf16().count() + 1;
    }
    (chars * 2) as i32
}

impl Flver {
    /// The face counts of the file: triangles that are not degenerate, and all triangles (strips lose the restarts).
    fn face_counts(&self) -> (i32, i32) {
        let (mut real, mut total) = (0i32, 0i32);
        for mesh in &self.meshes {
            let vertices = mesh.vertex_buffers.first().map_or(0, |b| b.vertex_count);
            let restarts = vertices < i32::from(u16::MAX);
            for fs in &mesh.face_sets {
                if fs.triangle_strip {
                    for w in fs.indices.windows(3) {
                        let (a, b, c) = (w[0], w[1], w[2]);
                        if !restarts || (a != 0xFFFF && b != 0xFFFF && c != 0xFFFF) {
                            total += 1;
                            if fs.flags & 0x8000_0000 == 0 && a != b && b != c && c != a {
                                real += 1;
                            }
                        }
                    }
                } else {
                    total += (fs.indices.len() / 3) as i32;
                    real += (fs.indices.len() / 3) as i32;
                }
            }
        }
        (real, total)
    }

    /// Writes the model as a FLVER file (the order the game's own tools use).
    pub fn write(&self) -> Result<Vec<u8>> {
        let h = &self.header;
        if h.version != 0x20013 && h.version != 0x20014 {
            return Err(FlverError::Unsupported(format!("writing FLVER version {:#x}", h.version)));
        }
        let mut o: Vec<u8> = Vec::new();
        o.extend(b"FLVER\0L\0");
        push_i32(&mut o, h.version);
        let data_offset_at = o.len();
        push_i32(&mut o, 0);
        let data_size_at = o.len();
        push_i32(&mut o, 0);
        push_i32(&mut o, self.dummies.len() as i32);
        push_i32(&mut o, self.materials.len() as i32);
        push_i32(&mut o, self.nodes.len() as i32);
        push_i32(&mut o, self.meshes.len() as i32);
        push_i32(&mut o, self.meshes.iter().map(|m| m.vertex_buffers.len()).sum::<usize>() as i32);
        push_v3(&mut o, h.bounding_box_min);
        push_v3(&mut o, h.bounding_box_max);
        let (real, total) = self.face_counts();
        push_i32(&mut o, real);
        push_i32(&mut o, total);
        o.push(0); // the index size is stored per face set in these versions
        o.push(u8::from(h.unicode));
        o.push(u8::from(h.unk4a));
        o.push(u8::from(h.unk4b));
        push_i32(&mut o, h.unk4c);
        push_i32(&mut o, self.meshes.iter().map(|m| m.face_sets.len()).sum::<usize>() as i32);
        push_i32(&mut o, self.layouts.len() as i32);
        push_i32(&mut o, self.materials.iter().map(|m| m.textures.len()).sum::<usize>() as i32);
        o.extend([h.unk5c, h.unk5d, 0, 0]);
        push_i32(&mut o, 0);
        push_i32(&mut o, 0);
        o.extend(h.unk68.to_le_bytes());
        o.extend(h.special_modifier.to_le_bytes());
        push_i32(&mut o, 0);
        push_i32(&mut o, 0);
        push_i32(&mut o, h.unk74);
        push_i32(&mut o, 0);
        push_i32(&mut o, 0);
        debug_assert_eq!(o.len(), HEADER_LEN);

        for d in &self.dummies {
            push_v3(&mut o, d.position);
            o.extend(d.color);
            push_v3(&mut o, d.forward);
            o.extend(d.reference_id.to_le_bytes());
            o.extend(d.parent_bone.to_le_bytes());
            push_v3(&mut o, d.upward);
            o.extend(d.attach_bone.to_le_bytes());
            o.push(u8::from(d.flag1));
            o.push(u8::from(d.use_upward_vector));
            push_i32(&mut o, d.unk30);
            push_i32(&mut o, d.unk34);
            push_i32(&mut o, 0);
            push_i32(&mut o, 0);
        }

        // materials: the offsets are filled in when the strings and lists are written
        let material_at: Vec<usize> = self.materials.iter().map(|_| 0).collect();
        let mut material_at = material_at;
        for (i, m) in self.materials.iter().enumerate() {
            material_at[i] = o.len();
            push_i32(&mut o, 0); // name
            push_i32(&mut o, 0); // file
            push_i32(&mut o, m.textures.len() as i32);
            push_i32(&mut o, 0); // first texture
            push_i32(&mut o, material_string_bytes(m));
            push_i32(&mut o, 0); // GX list
            push_i32(&mut o, m.index);
            push_i32(&mut o, 0);
        }

        let mut node_at = Vec::with_capacity(self.nodes.len());
        for n in &self.nodes {
            node_at.push(o.len());
            push_v3(&mut o, n.position);
            push_i32(&mut o, 0); // name
            push_v3(&mut o, n.rotation);
            o.extend(n.parent.to_le_bytes());
            o.extend(n.child.to_le_bytes());
            push_v3(&mut o, n.scale);
            o.extend(n.next_sibling.to_le_bytes());
            o.extend(n.previous_sibling.to_le_bytes());
            push_v3(&mut o, n.bounding_box_min);
            push_i32(&mut o, n.unk3c);
            push_v3(&mut o, n.bounding_box_max);
            o.extend([0u8; 0x34]);
        }

        let mut mesh_at = Vec::with_capacity(self.meshes.len());
        for m in &self.meshes {
            mesh_at.push(o.len());
            o.extend([m.dynamic, 0, 0, 0]);
            push_i32(&mut o, m.material_index);
            push_i32(&mut o, 0);
            push_i32(&mut o, 0);
            push_i32(&mut o, m.node_index);
            push_i32(&mut o, m.bone_indices.len() as i32);
            push_i32(&mut o, 0); // bounding box
            push_i32(&mut o, 0); // bone indices
            push_i32(&mut o, m.face_sets.len() as i32);
            push_i32(&mut o, 0); // face set indices
            push_i32(&mut o, m.vertex_buffers.len() as i32);
            push_i32(&mut o, 0); // vertex buffer indices
        }

        // face set and vertex buffer records, numbered over all meshes
        let mut face_set_at = Vec::new();
        for m in &self.meshes {
            for fs in &m.face_sets {
                if !matches!(fs.index_bits, 16 | 32) {
                    return Err(FlverError::Unsupported(format!("writing {}-bit face indices", fs.index_bits)));
                }
                face_set_at.push(o.len());
                o.extend(fs.flags.to_le_bytes());
                o.push(u8::from(fs.triangle_strip));
                o.push(u8::from(fs.cull_backfaces));
                o.extend(fs.unk06.to_le_bytes());
                push_i32(&mut o, fs.indices.len() as i32);
                push_i32(&mut o, 0); // offset of the indices
                push_i32(&mut o, (fs.indices.len() * usize::from(fs.index_bits / 8)) as i32);
                push_i32(&mut o, 0);
                push_i32(&mut o, i32::from(fs.index_bits));
                push_i32(&mut o, 0);
            }
        }
        let mut vertex_buffer_at = Vec::new();
        for m in &self.meshes {
            for (i, vb) in m.vertex_buffers.iter().enumerate() {
                vertex_buffer_at.push(o.len());
                push_i32(&mut o, i as i32);
                push_i32(&mut o, vb.layout_index);
                push_i32(&mut o, vb.vertex_size);
                push_i32(&mut o, vb.vertex_count);
                push_i32(&mut o, 0);
                push_i32(&mut o, 0);
                push_i32(&mut o, vb.vertex_size * vb.vertex_count);
                push_i32(&mut o, 0); // offset of the data
            }
        }
        let mut layout_at = Vec::with_capacity(self.layouts.len());
        for l in &self.layouts {
            layout_at.push(o.len());
            push_i32(&mut o, l.members.len() as i32);
            push_i32(&mut o, 0);
            push_i32(&mut o, 0);
            push_i32(&mut o, 0); // members
        }
        // textures, run by run
        let mut first_texture = 0i32;
        let mut texture_at = Vec::new();
        for (i, m) in self.materials.iter().enumerate() {
            put_i32(&mut o, material_at[i] + 12, first_texture);
            for t in &m.textures {
                texture_at.push(o.len());
                push_i32(&mut o, 0); // path
                push_i32(&mut o, 0); // type
                push_f32(&mut o, t.scale[0]);
                push_f32(&mut o, t.scale[1]);
                o.extend([t.tiling_u, t.tiling_v, 0, 0]);
                push_f32(&mut o, t.unk14);
                push_f32(&mut o, t.unk18);
                push_f32(&mut o, t.unk1c);
            }
            first_texture += m.textures.len() as i32;
        }

        pad(&mut o, 0x10);
        for (i, l) in self.layouts.iter().enumerate() {
            { let here = o.len(); put_here(&mut o, layout_at[i] + 12, here); }
            let mut offset = 0i32;
            for m in &l.members {
                let Some(size) = type_size(m.ty) else { return Err(FlverError::Unsupported(format!("writing a vertex member of {}", type_name(m.ty)))) };
                push_i32(&mut o, m.unk00);
                push_i32(&mut o, offset);
                push_i32(&mut o, m.ty as i32);
                push_i32(&mut o, m.semantic as i32);
                push_i32(&mut o, m.index);
                offset += size as i32;
            }
        }

        pad(&mut o, 0x10);
        for (i, m) in self.meshes.iter().enumerate() {
            match &m.bounding_box {
                None => put_i32(&mut o, mesh_at[i] + 0x18, 0),
                Some(b) => {
                    let here = o.len();
                    put_here(&mut o, mesh_at[i] + 0x18, here);
                    push_v3(&mut o, b.min);
                    push_v3(&mut o, b.max);
                }
            }
        }

        pad(&mut o, 0x10);
        let bone_indices_start = o.len();
        for (i, m) in self.meshes.iter().enumerate() {
            if m.bone_indices.is_empty() {
                // the game's tools point an empty list at the start of the section
                put_here(&mut o, mesh_at[i] + 0x1C, bone_indices_start);
            } else {
                { let here = o.len(); put_here(&mut o, mesh_at[i] + 0x1C, here); }
                for b in &m.bone_indices {
                    push_i32(&mut o, *b);
                }
            }
        }

        pad(&mut o, 0x10);
        let mut next = 0i32;
        for (i, m) in self.meshes.iter().enumerate() {
            { let here = o.len(); put_here(&mut o, mesh_at[i] + 0x24, here); }
            for _ in &m.face_sets {
                push_i32(&mut o, next);
                next += 1;
            }
        }

        pad(&mut o, 0x10);
        let mut next = 0i32;
        for (i, m) in self.meshes.iter().enumerate() {
            { let here = o.len(); put_here(&mut o, mesh_at[i] + 0x2C, here); }
            for _ in &m.vertex_buffers {
                push_i32(&mut o, next);
                next += 1;
            }
        }

        pad(&mut o, 0x10);
        let mut gx_at = Vec::with_capacity(self.gx_lists.len());
        for g in &self.gx_lists {
            gx_at.push(o.len());
            for item in &g.items {
                let mut tag = [0u8; 4];
                for (k, c) in item.id.bytes().take(4).enumerate() {
                    tag[k] = c;
                }
                o.extend(tag);
                push_i32(&mut o, item.unk04);
                push_i32(&mut o, item.data.len() as i32 + 0xC);
                o.extend(&item.data);
            }
            push_i32(&mut o, g.terminator_id);
            push_i32(&mut o, 100);
            push_i32(&mut o, g.terminator_length + 0xC);
            o.extend(std::iter::repeat_n(0u8, g.terminator_length.max(0) as usize));
        }
        for (i, m) in self.materials.iter().enumerate() {
            let at = usize::try_from(m.gx_index).ok().and_then(|k| gx_at.get(k)).copied().unwrap_or(0);
            put_i32(&mut o, material_at[i] + 20, at as i32);
        }

        pad(&mut o, 0x10);
        let mut texture_index = 0;
        for (i, m) in self.materials.iter().enumerate() {
            { let here = o.len(); put_here(&mut o, material_at[i], here); }
            push_utf16(&mut o, &m.name);
            { let here = o.len(); put_here(&mut o, material_at[i] + 4, here); }
            push_utf16(&mut o, &m.mtd);
            for t in &m.textures {
                { let here = o.len(); put_here(&mut o, texture_at[texture_index], here); }
                push_utf16(&mut o, &t.path);
                { let here = o.len(); put_here(&mut o, texture_at[texture_index] + 4, here); }
                push_utf16(&mut o, &t.param_name);
                texture_index += 1;
            }
        }

        pad(&mut o, 0x10);
        for (i, n) in self.nodes.iter().enumerate() {
            { let here = o.len(); put_here(&mut o, node_at[i] + 12, here); }
            push_utf16(&mut o, &n.name);
        }

        pad(&mut o, 0x10);
        let data_start = o.len();
        put_i32(&mut o, data_offset_at, data_start as i32);
        let (mut face_set_n, mut vertex_buffer_n) = (0, 0);
        for m in &self.meshes {
            for fs in &m.face_sets {
                pad(&mut o, 0x10);
                { let rel = o.len() - data_start; put_here(&mut o, face_set_at[face_set_n] + 0x0C, rel); }
                for i in &fs.indices {
                    if fs.index_bits == 16 {
                        if *i > u32::from(u16::MAX) {
                            return bad("a face index that does not fit 16 bits");
                        }
                        o.extend((*i as u16).to_le_bytes());
                    } else {
                        o.extend(i.to_le_bytes());
                    }
                }
                face_set_n += 1;
            }
            for vb in &m.vertex_buffers {
                pad(&mut o, 0x10);
                { let rel = o.len() - data_start; put_here(&mut o, vertex_buffer_at[vertex_buffer_n] + 0x1C, rel); }
                o.extend(&vb.data);
                vertex_buffer_n += 1;
            }
        }
        pad(&mut o, 0x10);
        { let rel = o.len() - data_start; put_here(&mut o, data_size_at, rel); }
        Ok(o)
    }

    /// The triangles of a mesh as a plain list of vertex numbers: the face set without flags (the full-detail one), or the
    /// first one. Strips are unrolled (a restart is the index 0xFFFF in a mesh with fewer than 65535 vertices).
    pub fn triangles(&self, mesh: usize) -> Vec<[u32; 3]> {
        let Some(m) = self.meshes.get(mesh) else { return Vec::new() };
        let Some(fs) = m.face_sets.iter().find(|f| f.flags == 0).or_else(|| m.face_sets.first()) else { return Vec::new() };
        if !fs.triangle_strip {
            return fs.indices.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        }
        let restarts = m.vertex_buffers.first().is_some_and(|b| b.vertex_count < i32::from(u16::MAX));
        let mut out = Vec::new();
        let mut flip = false;
        for w in fs.indices.windows(3) {
            let (a, b, c) = (w[0], w[1], w[2]);
            if restarts && (a == 0xFFFF || b == 0xFFFF || c == 0xFFFF) {
                flip = false;
                continue;
            }
            if a != b && b != c && c != a {
                out.push(if flip { [c, b, a] } else { [a, b, c] });
            }
            flip = !flip;
        }
        out
    }

    /// The positions of the first vertex buffer of a mesh, when its layout stores them as three or four floats.
    pub fn positions(&self, mesh: usize) -> Option<Vec<[f32; 3]>> {
        let m = self.meshes.get(mesh)?;
        for vb in &m.vertex_buffers {
            let layout = self.layouts.get(usize::try_from(vb.layout_index).ok()?)?;
            let member = layout.members.iter().find(|x| x.semantic == 0)?;
            if member.ty != 0x02 && member.ty != 0x03 {
                continue;
            }
            let size = usize::try_from(vb.vertex_size).ok()?;
            let at = usize::try_from(member.struct_offset).ok()?;
            let mut out = Vec::with_capacity(vb.vertex_count.max(0) as usize);
            for v in vb.data.chunks_exact(size) {
                let f = |k: usize| f32::from_le_bytes([v[at + k * 4], v[at + k * 4 + 1], v[at + k * 4 + 2], v[at + k * 4 + 3]]);
                out.push([f(0), f(1), f(2)]);
            }
            return Some(out);
        }
        None
    }
}

// ------------------------------------------------------------------------------------------------ describing a model

fn f3(v: [f32; 3]) -> String {
    format!("({:.3}, {:.3}, {:.3})", v[0], v[1], v[2])
}

impl Layout {
    /// `Position Float3, Normal Byte4C, ...` for reports.
    pub fn describe(&self) -> String {
        self.members.iter().map(|m| format!("{} {}{}", semantic_name(m.semantic), type_name(m.ty), if m.index != 0 { format!("#{}", m.index) } else { String::new() })).collect::<Vec<_>>().join(", ")
    }
}

impl Flver {
    /// A few lines about the whole model (counts, materials, bones, dummies, meshes): structure only.
    pub fn describe(&self) -> Vec<String> {
        let h = &self.header;
        let mut out = vec![
            format!(
                "FLVER version {:#x}, box {} to {}, {} dummies, {} materials, {} bones, {} meshes, {} layouts; faces {} ({} with restarts); unknown fields: 4A {} 4B {} 4C {} 5C {} 5D {} 68 {} special {} 74 {:#x}",
                h.version,
                f3(h.bounding_box_min),
                f3(h.bounding_box_max),
                self.dummies.len(),
                self.materials.len(),
                self.nodes.len(),
                self.meshes.len(),
                self.layouts.len(),
                h.face_count,
                h.total_face_count,
                h.unk4a,
                h.unk4b,
                h.unk4c,
                h.unk5c,
                h.unk5d,
                h.unk68,
                h.special_modifier,
                h.unk74
            ),
        ];
        for (i, m) in self.materials.iter().enumerate() {
            out.push(format!("  material {i}: \"{}\" mtd \"{}\" gx {} index {}", m.name, m.mtd, m.gx_index, m.index));
            for t in &m.textures {
                out.push(format!("      {} = {}  (scale {:.2} x {:.2}, tiling {} {}, {:.2} {:.2} {:.2})", t.param_name, t.path, t.scale[0], t.scale[1], t.tiling_u, t.tiling_v, t.unk14, t.unk18, t.unk1c));
            }
        }
        for (i, g) in self.gx_lists.iter().enumerate() {
            out.push(format!("  GX list {i}: {} items [{}], terminator {:#x} length {}", g.items.len(), g.items.iter().map(|x| format!("{} ({} bytes)", x.id, x.data.len())).collect::<Vec<_>>().join(", "), g.terminator_id, g.terminator_length));
        }
        for (i, n) in self.nodes.iter().enumerate().take(40) {
            out.push(format!("  bone {i}: \"{}\" parent {} child {} next {} previous {} at {} rotation {} scale {}", n.name, n.parent, n.child, n.next_sibling, n.previous_sibling, f3(n.position), f3(n.rotation), f3(n.scale)));
        }
        if self.nodes.len() > 40 {
            out.push(format!("  ... and {} more bones", self.nodes.len() - 40));
        }
        for (i, d) in self.dummies.iter().enumerate().take(24) {
            out.push(format!("  dummy {i}: reference {} parent bone {} attach bone {} at {} color {:?} flags {} {}", d.reference_id, d.parent_bone, d.attach_bone, f3(d.position), d.color, d.flag1, d.use_upward_vector));
        }
        if self.dummies.len() > 24 {
            out.push(format!("  ... and {} more dummies", self.dummies.len() - 24));
        }
        for (i, l) in self.layouts.iter().enumerate() {
            out.push(format!("  layout {i}: {} bytes per vertex: {}", l.size().map_or("?".to_string(), |s| s.to_string()), l.describe()));
        }
        for (i, m) in self.meshes.iter().enumerate() {
            let verts: Vec<i32> = m.vertex_buffers.iter().map(|b| b.vertex_count).collect();
            out.push(format!(
                "  mesh {i}: material {} node {} dynamic {} bones {:?} box {:?}; vertex buffers {} (layouts {:?}, vertices {:?}); face sets {}",
                m.material_index,
                m.node_index,
                m.dynamic,
                m.bone_indices,
                m.bounding_box.as_ref().map(|b| (f3(b.min), f3(b.max))),
                m.vertex_buffers.len(),
                m.vertex_buffers.iter().map(|b| b.layout_index).collect::<Vec<_>>(),
                verts,
                m.face_sets.iter().map(|f| format!("[flags {:#x} {} {}-bit {} indices cull {} unk06 {}]", f.flags, if f.triangle_strip { "strip" } else { "list" }, f.index_bits, f.indices.len(), f.cull_backfaces, f.unk06)).collect::<Vec<_>>().join(" ")
            ));
        }
        out
    }

    /// What the vertex data of one mesh looks like, member by member: value ranges and how well the common ways of reading a
    /// member fit (unit length of normals and tangents, weights adding up to one, texture coordinates inside the picture).
    /// Plus the raw bytes of the first two vertices. This is how the meaning of a member type is checked against real data.
    pub fn vertex_stats(&self, mesh: usize) -> Vec<String> {
        let mut out = Vec::new();
        let Some(m) = self.meshes.get(mesh) else { return out };
        for (bi, vb) in m.vertex_buffers.iter().enumerate() {
            let Some(layout) = usize::try_from(vb.layout_index).ok().and_then(|l| self.layouts.get(l)) else { continue };
            let size = vb.vertex_size.max(1) as usize;
            let n = vb.data.len() / size;
            out.push(format!("    mesh {mesh} buffer {bi}: {n} vertices of {size} bytes; first two: {} | {}", crate::util::hex(vb.data.get(..size).unwrap_or(&[])), crate::util::hex(vb.data.get(size..2 * size).unwrap_or(&[]))));
            for member in &layout.members {
                let at = member.struct_offset as usize;
                let width = type_size(member.ty).unwrap_or(0);
                let raw = |v: usize| -> &[u8] { vb.data.get(v * size + at..v * size + at + width).unwrap_or(&[]) };
                let label = format!("{} {}", semantic_name(member.semantic), type_name(member.ty));
                let mut line = String::new();
                match member.ty {
                    0x01..=0x03 => {
                        let comps = width / 4;
                        let (mut lo, mut hi) = ([f32::MAX; 4], [f32::MIN; 4]);
                        for v in 0..n {
                            let r = raw(v);
                            for k in 0..comps {
                                let x = f32::from_le_bytes([r[k * 4], r[k * 4 + 1], r[k * 4 + 2], r[k * 4 + 3]]);
                                lo[k] = lo[k].min(x);
                                hi[k] = hi[k].max(x);
                            }
                        }
                        line = (0..comps).map(|k| format!("[{:.3} .. {:.3}]", lo[k], hi[k])).collect::<Vec<_>>().join(" ");
                    }
                    0x10 | 0x11 | 0x13 | 0x2F => {
                        // four bytes: unit vector, weights or indices - report each reading
                        let mut unit_a = 0usize; // bytes 0..3 as (b - 127) / 127
                        let mut unit_b = 0usize; // bytes 1..4 as (b - 127) / 127
                        let mut sums_u8 = (f32::MAX, f32::MIN);
                        let mut sums_i8 = (f32::MAX, f32::MIN);
                        let mut mins = [255u8; 4];
                        let mut maxs = [0u8; 4];
                        for v in 0..n {
                            let r = raw(v);
                            let len = |a: usize| (0..3).map(|k| ((f32::from(r[a + k]) - 127.0) / 127.0).powi(2)).sum::<f32>().sqrt();
                            if (len(0) - 1.0).abs() < 0.06 {
                                unit_a += 1;
                            }
                            if (len(1) - 1.0).abs() < 0.06 {
                                unit_b += 1;
                            }
                            let su: f32 = r.iter().map(|b| f32::from(*b) / 255.0).sum();
                            let si: f32 = r.iter().map(|b| f32::from(*b as i8) / 127.0).sum();
                            sums_u8 = (sums_u8.0.min(su), sums_u8.1.max(su));
                            sums_i8 = (sums_i8.0.min(si), sums_i8.1.max(si));
                            for k in 0..4 {
                                mins[k] = mins[k].min(r[k]);
                                maxs[k] = maxs[k].max(r[k]);
                            }
                        }
                        let pct = |c: usize| if n == 0 { 0.0 } else { 100.0 * c as f32 / n as f32 };
                        line = format!(
                            "bytes min {:?} max {:?}; unit length as (b-127)/127: bytes 0-2 {:.1}% bytes 1-3 {:.1}%; sum as u8/255 {:.2}..{:.2}, as i8/127 {:.2}..{:.2}",
                            mins,
                            maxs,
                            pct(unit_a),
                            pct(unit_b),
                            sums_u8.0,
                            sums_u8.1,
                            sums_i8.0,
                            sums_i8.1
                        );
                    }
                    0x12 | 0x15 | 0x16 | 0x18 | 0x1A | 0x2E => {
                        let comps = width / 2;
                        let (mut lo, mut hi) = ([i16::MAX; 4], [i16::MIN; 4]);
                        let (mut sum_lo, mut sum_hi) = (f32::MAX, f32::MIN);
                        for v in 0..n {
                            let r = raw(v);
                            let mut sum = 0.0;
                            for k in 0..comps.min(4) {
                                let x = i16::from_le_bytes([r[k * 2], r[k * 2 + 1]]);
                                lo[k] = lo[k].min(x);
                                hi[k] = hi[k].max(x);
                                sum += f32::from(u16::from_le_bytes([r[k * 2], r[k * 2 + 1]])) / 65535.0;
                            }
                            sum_lo = sum_lo.min(sum);
                            sum_hi = sum_hi.max(sum);
                        }
                        line = format!("shorts {} (sum as u16/65535 {:.2}..{:.2})", (0..comps.min(4)).map(|k| format!("[{} .. {}]", lo[k], hi[k])).collect::<Vec<_>>().join(" "), sum_lo, sum_hi);
                    }
                    _ => {}
                }
                out.push(format!("      {label}: {line}"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::flver::sample_flver;

    #[test]
    fn a_made_up_model_writes_and_reads_back_the_same() {
        let model = sample_flver();
        let bytes = model.write().unwrap();
        assert_eq!(&bytes[..8], b"FLVER\0L\0");
        assert_eq!(bytes.len() % 16, 0);
        let back = Flver::parse(&bytes).unwrap_or_else(|e| panic!("{e}"));
        // the face counts in the header are computed on writing
        let mut expected = model.clone();
        expected.header.face_count = back.header.face_count;
        expected.header.total_face_count = back.header.total_face_count;
        assert_eq!(back, expected);
        // writing what was read gives the same bytes
        assert_eq!(back.write().unwrap(), bytes);
        // 8 strip vertices: 6 triangles + 1 (second set: 7 indices -> 5), and a list of 4 indices -> 1 triangle
        assert_eq!((back.header.face_count, back.header.total_face_count), (6 + 5 + 1 + 1, 6 + 5 + 1 + 1));
    }

    #[test]
    fn triangles_and_positions_are_read_from_the_streams() {
        let model = sample_flver();
        let t0 = model.triangles(0);
        assert_eq!(t0.len(), 6);
        assert_eq!(t0[0], [0, 1, 2]);
        assert_eq!(t0[1], [3, 2, 1], "every second triangle of a strip is flipped");
        assert_eq!(model.triangles(1), vec![[0, 1, 2]]);
        let p = model.positions(0).unwrap();
        assert_eq!(p.len(), 8);
        assert_eq!(p[1], [0.75 + 1.0, 1.0 + 1.0, 1.25 + 1.0]);
        assert_eq!(model.triangles(9), Vec::<[u32; 3]>::new());
        assert_eq!(model.positions(9), None);
    }

    #[test]
    fn strips_restart_at_the_marker_index() {
        let mut model = sample_flver();
        model.meshes[0].face_sets[0].indices = vec![0, 1, 2, 3, 0xFFFF, 4, 5, 6, 7];
        assert_eq!(model.triangles(0), vec![[0, 1, 2], [3, 2, 1], [4, 5, 6], [7, 6, 5]]);
    }

    #[test]
    fn the_description_and_the_vertex_statistics_cover_the_model() {
        let model = sample_flver();
        let lines = model.describe();
        let text = lines.join("\n");
        assert!(text.contains("FLVER version 0x20014") && text.contains("2 materials, 3 bones, 2 meshes, 1 layouts"), "{text}");
        assert!(text.contains("material 0: \"blade\" mtd \"N:\\FDP\\mtd\\Wp\\Wp_Metal[DSB].mtd\""), "{text}");
        assert!(text.contains("g_Bumpmap = N:\\FDP\\data\\Other\\wp_a_9999_n.tga"), "{text}");
        assert!(text.contains("layout 0: 28 bytes per vertex: Position Float3, Normal Byte4C, Tangent Byte4C, BoneIndices Byte4B, UV Short2toFloat2"), "{text}");
        assert!(text.contains("flags 0x1000000 strip 16-bit 7 indices"), "{text}");
        let stats = model.vertex_stats(0).join("\n");
        assert!(stats.contains("mesh 0 buffer 0: 8 vertices of 28 bytes"), "{stats}");
        assert!(stats.contains("Position Float3: [1.000 .. 6.250]"), "{stats}");
        assert!(stats.contains("Normal Byte4C: bytes min") && stats.contains("UV Short2toFloat2: shorts"), "{stats}");
        assert!(model.vertex_stats(9).is_empty());
    }

    #[test]
    fn what_is_not_a_dark_souls_iii_flver_is_refused_with_a_reason() {
        assert_eq!(Flver::parse(b"nothing like it"), Err(FlverError::NotFlver));
        assert_eq!(Flver::parse(&[]), Err(FlverError::NotFlver));
        let mut x = sample_flver().write().unwrap();
        x[6] = b'B';
        assert_eq!(Flver::parse(&x), Err(FlverError::BigEndian));
        let mut x = sample_flver().write().unwrap();
        x[8..12].copy_from_slice(&0x2000Bi32.to_le_bytes());
        assert!(matches!(Flver::parse(&x), Err(FlverError::Unsupported(_))));
        let mut x = sample_flver().write().unwrap();
        x[0x49] = 0;
        assert!(matches!(Flver::parse(&x), Err(FlverError::Unsupported(_))));
        let mut m = sample_flver();
        m.header.version = 0x20010;
        assert!(matches!(m.write(), Err(FlverError::Unsupported(_))));
    }

    #[test]
    fn damaged_copies_are_errors_and_never_panics() {
        let bytes = sample_flver().write().unwrap();
        for cut in (0..bytes.len()).step_by(5) {
            assert!(Flver::parse(&bytes[..cut]).is_err() || cut == bytes.len(), "cut at {cut}");
        }
        for i in (0..bytes.len()).step_by(3) {
            for flip in [0x01u8, 0x80, 0xFF] {
                let mut x = bytes.clone();
                x[i] ^= flip;
                let _ = Flver::parse(&x);
            }
        }
        // huge counts do not allocate
        let mut x = bytes.clone();
        x[0x14..0x18].copy_from_slice(&0x7FFF_FFFFi32.to_le_bytes());
        assert!(Flver::parse(&x).is_err());
    }
}
