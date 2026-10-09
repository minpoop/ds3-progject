//! Turns the buffers of a Space Marine 2 model (see [`crate::tpl`]) into plain triangles: positions, normals, texture
//! coordinates, bone numbers and the index list of one sub mesh.
//!
//! The vertex data of a mesh is spread over several buffers that are read together, one element per vertex: a vertex
//! stream (position, packed normal), optionally a stream of bone numbers, an interleaved stream (tangent, colour, texture
//! coordinates) and a face stream (three 16-bit vertex numbers per triangle). Numbers are stored as 16-bit fixed point and
//! are turned back into floats here. Nothing is written anywhere; the functions only read byte slices.
use crate::tpl::{Buffer, Geometry};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshError(pub String);

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MeshError {}

type Result<T> = std::result::Result<T, MeshError>;

fn fail<T>(what: impl Into<String>) -> Result<T> {
    Err(MeshError(what.into()))
}

// Bits of a buffer's format flags (the vertex format).
const FVF_VERT: usize = 0;
const FVF_VERT_COMPR: usize = 3;
const FVF_WEIGHT4: usize = 7;
const FVF_INDICES: usize = 9;
const FVF_NORM: usize = 10;
const FVF_NORM_COMPR: usize = 11;
const FVF_TANG0: usize = 12;
const FVF_TANG_COMPR: usize = 17;
const FVF_COLOR0: usize = 22;
const FVF_TEX0: usize = 25;
const FVF_TEX0_COMPR: usize = 30;
const FVF_NORM_IN_VERT4: usize = 45;
const FVF_COLOR3: usize = 46;
const FVF_COLOR4: usize = 47;
const FVF_TEX5: usize = 59;
const FVF_COLOR5: usize = 63;
const FVF_MASKING: usize = 67;
const FVF_BS_INFO: usize = 68;
const FVF_WEIGHT8: usize = 69;
const FVF_INDICES16: usize = 70;

/// One sub mesh as triangles.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DecodedMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// First set of texture coordinates (empty when the format has none).
    pub uvs: Vec<[f32; 2]>,
    /// First tangent of each vertex (xyz plus the handedness in w), when the format has one.
    pub tangents: Vec<[f32; 4]>,
    /// Bone numbers of each vertex (up to four), when the mesh has them.
    pub bones: Vec<[u16; 4]>,
    /// Bone weights (0..1), when the mesh has them; missing weights with bone numbers mean "all on the first bone".
    pub weights: Vec<[f32; 4]>,
    /// Vertex numbers, relative to the start of `positions`.
    pub triangles: Vec<[u32; 3]>,
}

fn snorm16(v: i16) -> f32 {
    (f32::from(v) / 32767.0).max(-1.0)
}

fn snorm8(v: i8) -> f32 {
    (f32::from(v) / 127.0).max(-1.0)
}

fn frac(v: f32) -> f32 {
    v - v.floor()
}

/// The 16-bit packed normal stored in the fourth component of a compressed position.
pub fn unpack_normal16(w: i16) -> [f32; 3] {
    let w = if w == i16::MIN { 0 } else { w };
    let a = f32::from(w).abs();
    let x = (-1.0 + 2.0 * frac(a / 181.0)) * (181.0 / 179.0);
    let z = (-1.0 + 2.0 * frac(a / 181.0 / 181.0)) * (181.0 / 180.0);
    let y = f32::from(w.signum()) * (1.0 - x * x - z * z).clamp(0.0, 1.0).sqrt();
    [x, y, z]
}

/// A float that carries a packed normal in three 8-bit parts (the uncompressed form of "normal in the position's fourth value").
pub fn unpack_normal_f32(w: f32) -> [f32; 3] {
    [-1.0 + 2.0 * frac(w / 256.0), -1.0 + 2.0 * frac(w / 65536.0), -1.0 + 2.0 * frac(w / 16_777_216.0)]
}

/// What a buffer holds, from its format flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferRole {
    Positions,
    Faces,
    BoneIndices,
    Interleaved,
    Unknown,
}

pub fn role_of(b: &Buffer) -> BufferRole {
    let f = &b.flags;
    if f.get(FVF_VERT) {
        BufferRole::Positions
    } else if f.low64() == 0 && b.stride == 6 {
        BufferRole::Faces
    } else if f.get(FVF_INDICES) || f.get(FVF_WEIGHT4) || f.get(FVF_WEIGHT8) {
        BufferRole::BoneIndices
    } else if (FVF_TANG0..=FVF_TANG0 + 4).any(|i| f.get(i)) || (FVF_COLOR0..=FVF_COLOR0 + 2).any(|i| f.get(i)) || (FVF_TEX0..=FVF_TEX0 + 4).any(|i| f.get(i)) || f.get(FVF_TEX5) {
        BufferRole::Interleaved
    } else {
        BufferRole::Unknown
    }
}

/// The bytes of buffer `b` inside `data`, starting `sub_offset` bytes into it.
fn buffer_bytes<'a>(data: &'a [u8], b: &Buffer, sub_offset: i32) -> Result<&'a [u8]> {
    let Ok(sub) = u64::try_from(sub_offset) else { return fail("a negative offset inside a buffer") };
    let start = b.start + sub;
    let end = b.start + u64::from(b.length);
    if sub > u64::from(b.length) || end > data.len() as u64 {
        return fail(format!("a buffer reaches to {end} but the data has {} bytes", data.len()));
    }
    Ok(&data[start as usize..end as usize])
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let Some(s) = self.b.get(self.p..self.p + N) else { return fail("an element reaches past the end of its buffer") };
        self.p += N;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take()?))
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take()?))
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take::<1>()?[0])
    }
    fn i8(&mut self) -> Result<i8> {
        Ok(self.take::<1>()?[0] as i8)
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take()?))
    }
}

/// Reads sub mesh `index` of the geometry. `data` is the `.tpl_data` file (or the `.tpl` itself when the buffers are inline).
pub fn decode_sub_mesh(g: &Geometry, data: &[u8], index: usize) -> Result<DecodedMesh> {
    let Some(sub) = g.sub_meshes.get(index) else { return fail(format!("there is no sub mesh {index}")) };
    let Some(mesh) = usize::try_from(sub.mesh).ok().and_then(|m| g.meshes.get(m)) else { return fail(format!("sub mesh {index} refers to a mesh that does not exist")) };
    // the buffers of the mesh by what they hold
    let (mut positions, mut faces, mut bones, mut inter) = (None, None, None, None);
    for (id, offset) in &mesh.buffers {
        let Some(b) = usize::try_from(*id).ok().and_then(|i| g.buffers.get(i)) else { return fail(format!("mesh {} refers to buffer {id}, which does not exist", sub.mesh)) };
        let slot = match role_of(b) {
            BufferRole::Positions => &mut positions,
            BufferRole::Faces => &mut faces,
            BufferRole::BoneIndices => &mut bones,
            BufferRole::Interleaved => &mut inter,
            BufferRole::Unknown => continue,
        };
        if slot.is_none() {
            *slot = Some((b, buffer_bytes(data, b, *offset)?));
        }
    }
    let Some((pos_buf, pos_bytes)) = positions else { return fail(format!("mesh {} has no position stream", sub.mesh)) };
    let Some((_, face_bytes)) = faces else { return fail(format!("mesh {} has no face stream", sub.mesh)) };

    // which vertices the triangles of this sub mesh use
    let first_face = usize::from(sub.face_offset);
    let face_count = usize::from(sub.face_count);
    let mut tris = Vec::with_capacity(face_count);
    let mut lo = u32::MAX;
    let mut hi = 0u32;
    for f in first_face..first_face + face_count {
        let at = f * 6;
        let Some(raw) = face_bytes.get(at..at + 6) else { return fail("a triangle reaches past the face stream") };
        let t = [u32::from(u16::from_le_bytes([raw[0], raw[1]])), u32::from(u16::from_le_bytes([raw[2], raw[3]])), u32::from(u16::from_le_bytes([raw[4], raw[5]]))];
        for v in t {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        tris.push(t);
    }
    let mut out = DecodedMesh::default();
    if tris.is_empty() {
        return Ok(out);
    }
    // the vertices from the first used to the last used one are decoded; triangles are renumbered to start at 0
    let count = (hi - lo + 1) as usize;
    out.triangles = tris.iter().map(|t| [t[0] - lo, t[1] - lo, t[2] - lo]).collect();

    // positions (and the normal that may sit in the fourth value)
    let pf = &pos_buf.flags;
    let stride = usize::from(pos_buf.stride);
    let compressed = pf.get(FVF_VERT_COMPR);
    let normal_in_vert4 = pf.get(FVF_NORM_IN_VERT4);
    let (offset, scale) = match sub.transform {
        Some((p, s)) => ([f32::from(p[0]), f32::from(p[1]), f32::from(p[2])], [f32::from(s[0]), f32::from(s[1]), f32::from(s[2])]),
        None => ([0.0; 3], [1.0; 3]),
    };
    for k in 0..count {
        let at = (lo as usize + k) * stride;
        let Some(raw) = pos_bytes.get(at..at + stride) else { return fail("a vertex reaches past the position stream") };
        let mut r = Reader { b: raw, p: 0 };
        let (mut p, mut n);
        if compressed {
            p = [snorm16(r.i16()?), snorm16(r.i16()?), snorm16(r.i16()?)];
            let w = r.i16()?;
            n = if normal_in_vert4 { unpack_normal16(w) } else { [0.0, 0.0, 1.0] };
            for a in 0..3 {
                p[a] = p[a] * scale[a] + offset[a];
            }
        } else {
            p = [r.f32()?, r.f32()?, r.f32()?];
            n = [0.0, 0.0, 1.0];
            if normal_in_vert4 {
                n = unpack_normal_f32(r.f32()?);
            }
        }
        // weights, bone numbers, masking value, separate normal: all in the same element, in this order
        if pf.get(FVF_WEIGHT4) || pf.get(FVF_WEIGHT8) {
            let n_w = if pf.get(FVF_WEIGHT8) { 8 } else { 4 };
            let mut w = [0f32; 4];
            for i in 0..n_w {
                let v = f32::from(r.u8()?) / 255.0;
                if i < 4 {
                    w[i] = v;
                }
            }
            out.weights.push(w);
        }
        if pf.get(FVF_INDICES) || pf.get(FVF_INDICES16) {
            let n_i = if pf.get(FVF_WEIGHT8) { 8 } else { 4 };
            let mut b = [0u16; 4];
            for i in 0..n_i {
                let v = if pf.get(FVF_INDICES16) { r.u16()? } else { u16::from(r.u8()?) };
                if i < 4 {
                    b[i] = v;
                }
            }
            out.bones.push(b);
        }
        if pf.get(FVF_MASKING) {
            r.take::<4>()?;
        }
        if pf.get(FVF_NORM) && !normal_in_vert4 {
            n = if pf.get(FVF_NORM_COMPR) { unpack_normal16(r.i16()?) } else { [r.f32()?, r.f32()?, r.f32()?] };
        }
        out.positions.push(p);
        out.normals.push(n);
    }

    // bone numbers from their own stream
    if let Some((bb, bytes)) = bones {
        let st = usize::from(bb.stride);
        let wide = bb.flags.get(FVF_INDICES16);
        for k in 0..count {
            let at = (lo as usize + k) * st;
            let Some(raw) = bytes.get(at..at + st) else { return fail("a bone number reaches past its stream") };
            let mut r = Reader { b: raw, p: 0 };
            let mut b = [0u16; 4];
            for slot in b.iter_mut().take(if wide { st / 2 } else { st }.min(4)) {
                *slot = if wide { r.u16()? } else { u16::from(r.u8()?) };
            }
            out.bones.push(b);
        }
    }

    // texture coordinates from the interleaved stream: tangent(s), colour(s), (a 16-bit value), then the coordinates
    if let Some((ib, bytes)) = inter {
        let f = &ib.flags;
        let st = usize::from(ib.stride);
        let tangents = (0..5).filter(|i| f.get(FVF_TANG0 + i)).count();
        let tangent_bytes = tangents * if f.get(FVF_TANG_COMPR) { 4 } else { 16 };
        let colors = (0..3).filter(|i| f.get(FVF_COLOR0 + i)).count() + [FVF_COLOR3, FVF_COLOR4, FVF_COLOR5].iter().filter(|b| f.get(**b)).count();
        let skip = tangent_bytes + colors * 4 + if f.get(FVF_BS_INFO) { 2 } else { 0 };
        if f.get(FVF_TANG0) {
            for k in 0..count {
                let at = (lo as usize + k) * st;
                let Some(raw) = bytes.get(at..at + 4) else { return fail("a tangent reaches past its stream") };
                let mut r = Reader { b: raw, p: 0 };
                if f.get(FVF_TANG_COMPR) {
                    out.tangents.push([snorm8(r.i8()?), snorm8(r.i8()?), snorm8(r.i8()?), snorm8(r.i8()?)]);
                } else {
                    let Some(wide) = bytes.get(at..at + 16) else { return fail("a tangent reaches past its stream") };
                    let mut r = Reader { b: wide, p: 0 };
                    out.tangents.push([r.f32()?, r.f32()?, r.f32()?, r.f32()?]);
                }
            }
        }
        if f.get(FVF_TEX0) {
            let compressed_uv = f.get(FVF_TEX0_COMPR);
            let uv_scale = sub.uv_scaling.iter().find(|(set, _)| *set == 0).map_or(1.0, |(_, s)| f32::from(*s));
            for k in 0..count {
                let at = (lo as usize + k) * st + skip;
                let width = if compressed_uv { 4 } else { 8 };
                let Some(raw) = bytes.get(at..at + width) else { return fail("a texture coordinate reaches past its stream") };
                let mut r = Reader { b: raw, p: 0 };
                let (u, v) = if compressed_uv {
                    (snorm16(r.i16()?), snorm16(r.i16()?))
                } else {
                    (r.f32()?, r.f32()?)
                };
                out.uvs.push([u * uv_scale, 1.0 - v * uv_scale]);
            }
        }
    }
    Ok(out)
}

/// A small text form (Wavefront OBJ) of decoded triangles, for looking at a model in any viewer.
pub fn to_obj(mesh: &DecodedMesh, name: &str) -> String {
    let mut out = format!("o {name}\n");
    for p in &mesh.positions {
        out.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
    }
    for n in &mesh.normals {
        out.push_str(&format!("vn {} {} {}\n", n[0], n[1], n[2]));
    }
    for uv in &mesh.uvs {
        out.push_str(&format!("vt {} {}\n", uv[0], uv[1]));
    }
    let has_uv = !mesh.uvs.is_empty();
    for t in &mesh.triangles {
        let f = |i: u32| {
            let i = i + 1;
            if has_uv {
                format!("{i}/{i}/{i}")
            } else {
                format!("{i}//{i}")
            }
        };
        out.push_str(&format!("f {} {} {}\n", f(t[0]), f(t[1]), f(t[2])));
    }
    out
}

/// The box around the positions: (min, max).
pub fn bounds(mesh: &DecodedMesh) -> Option<([f32; 3], [f32; 3])> {
    let first = mesh.positions.first()?;
    let (mut lo, mut hi) = (*first, *first);
    for p in &mesh.positions {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    Some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_numbers_cover_their_range() {
        assert_eq!(snorm16(32767), 1.0);
        assert_eq!(snorm16(-32768), -1.0);
        assert_eq!(snorm16(0), 0.0);
        assert_eq!(snorm8(127), 1.0);
        assert_eq!(snorm8(-128), -1.0);
    }

    #[test]
    fn packed_normals_have_components_in_range() {
        for w in [0i16, 1, -1, 181, 5000, -5000, 12345, i16::MAX, i16::MIN] {
            let n = unpack_normal16(w);
            assert!(n.iter().all(|c| c.is_finite() && c.abs() <= 1.1), "{w}: {n:?}");
        }
        // zero packs the direction (-1.0099, 0, -1.0056): the vertical part is clamped to flat
        assert_eq!(unpack_normal16(0)[1], 0.0);
        // the sign of the number is the sign of the vertical part
        assert!(unpack_normal16(5000)[1] >= 0.0 && unpack_normal16(-5000)[1] <= 0.0);
    }

    #[test]
    fn the_obj_text_has_one_line_per_vertex_and_triangle() {
        let m = DecodedMesh { positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], normals: vec![[0.0, 0.0, 1.0]; 3], uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], triangles: vec![[0, 1, 2]], ..DecodedMesh::default() };
        let text = to_obj(&m, "t");
        assert_eq!(text.lines().filter(|l| l.starts_with("v ")).count(), 3);
        assert!(text.contains("f 1/1/1 2/2/2 3/3/3"), "{text}");
        assert_eq!(bounds(&m), Some(([0.0, 0.0, 0.0], [1.0, 1.0, 0.0])));
        assert_eq!(bounds(&DecodedMesh::default()), None);
    }
}
