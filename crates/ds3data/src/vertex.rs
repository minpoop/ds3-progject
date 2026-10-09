//! The values of one vertex of a FLVER vertex buffer: reading them from the bytes of a layout and writing them back.
//!
//! The meaning of each member type (which bytes are a unit vector, how texture coordinates are scaled) is the community's
//! file notes for the Souls games. They are conventions, not guarantees, so nothing here is trusted on its own: the model
//! swap first decodes the game's own vertices with [`decode`], checks that the values make sense (unit normals, weights that
//! add up to one) and that [`encode`] gives the very same bytes back, and only then writes new vertices.
//!
//! Members whose job is not known are never invented: [`encode`] starts from a template vertex (a vertex of the model that
//! is being replaced) and changes only the members it knows, so padding bytes, extra texture coordinate sets and the like
//! keep what the game's own vertices have.
use crate::flver::{semantic_name, type_name, type_size, Layout, LayoutMember};
use std::fmt;

/// The semantics of [`LayoutMember::semantic`].
pub mod sem {
    pub const POSITION: u32 = 0;
    pub const BONE_WEIGHTS: u32 = 1;
    pub const BONE_INDICES: u32 = 2;
    pub const NORMAL: u32 = 3;
    pub const UV: u32 = 5;
    pub const TANGENT: u32 = 6;
    pub const BITANGENT: u32 = 7;
    pub const VERTEX_COLOR: u32 = 10;
}

/// The member types of [`LayoutMember::ty`].
pub mod ty {
    pub const FLOAT2: u32 = 0x01;
    pub const FLOAT3: u32 = 0x02;
    pub const FLOAT4: u32 = 0x03;
    pub const BYTE4A: u32 = 0x10;
    pub const BYTE4B: u32 = 0x11;
    pub const SHORT2_TO_FLOAT2: u32 = 0x12;
    pub const BYTE4C: u32 = 0x13;
    pub const UV: u32 = 0x15;
    pub const UV_PAIR: u32 = 0x16;
    pub const SHORT_BONE_INDICES: u32 = 0x18;
    pub const SHORT4_TO_FLOAT4A: u32 = 0x1A;
    pub const SHORT4_TO_FLOAT4B: u32 = 0x2E;
    pub const BYTE4E: u32 = 0x2F;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VertexError {
    /// A member whose semantic and type this code has no reading for.
    Unsupported(String),
    /// The bytes or the layout do not fit each other.
    Malformed(String),
    /// A value that cannot be stored in the member (a texture coordinate far outside the range of a 16-bit number).
    OutOfRange(String),
}

impl fmt::Display for VertexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VertexError::Unsupported(s) => write!(f, "vertex member not supported: {s}"),
            VertexError::Malformed(s) => write!(f, "vertex data does not fit its layout: {s}"),
            VertexError::OutOfRange(s) => write!(f, "value does not fit the vertex format: {s}"),
        }
    }
}

impl std::error::Error for VertexError {}

type Result<T> = std::result::Result<T, VertexError>;

/// How the file version scales texture coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Codec {
    /// A 16-bit texture coordinate is `value / uv_factor`.
    pub uv_factor: f32,
}

impl Codec {
    /// The factor the community's notes give: 1024 before version `0x2000F`, 2048 from there on (Dark Souls III is `0x20014`).
    pub fn for_version(version: i32) -> Codec {
        Codec { uv_factor: if version >= 0x2000F { 2048.0 } else { 1024.0 } }
    }
}

/// What the known members of a vertex hold. Members the layout does not have stay `None` / empty.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Values {
    pub position: Option<[f32; 3]>,
    pub normal: Option<[f32; 3]>,
    /// One per `Tangent` member: x, y, z and the fourth value as stored (handedness or padding).
    pub tangents: Vec<[f32; 4]>,
    pub bitangent: Option<[f32; 3]>,
    /// One per texture coordinate set, in the order of the layout (a member that holds a pair gives two).
    pub uvs: Vec<[f32; 2]>,
    pub bone_indices: Option<[u16; 4]>,
    pub bone_weights: Option<[f32; 4]>,
    /// True when the vertex has a colour member (white is written on encoding).
    pub has_color: bool,
}

fn member_label(m: &LayoutMember) -> String {
    format!("{} {}", semantic_name(m.semantic), type_name(m.ty))
}

/// Whether [`decode`] and [`encode`] know this member.
pub fn member_supported(m: &LayoutMember) -> bool {
    use ty::*;
    match m.semantic {
        sem::POSITION => matches!(m.ty, FLOAT3 | FLOAT4),
        sem::NORMAL | sem::TANGENT | sem::BITANGENT => matches!(m.ty, FLOAT3 | FLOAT4 | BYTE4A | BYTE4B | BYTE4C | SHORT4_TO_FLOAT4A | SHORT4_TO_FLOAT4B),
        sem::UV => matches!(m.ty, FLOAT2 | FLOAT4 | BYTE4A | BYTE4B | SHORT2_TO_FLOAT2 | UV | UV_PAIR),
        sem::BONE_INDICES => matches!(m.ty, BYTE4A | BYTE4B | BYTE4E | SHORT_BONE_INDICES),
        sem::BONE_WEIGHTS => matches!(m.ty, BYTE4A | BYTE4C | SHORT2_TO_FLOAT2 | UV_PAIR | SHORT4_TO_FLOAT4A),
        sem::VERTEX_COLOR => matches!(m.ty, FLOAT4 | BYTE4A | BYTE4B | BYTE4C),
        _ => false,
    }
}

/// The members of a layout that have no reading (empty: every member is known).
pub fn unsupported_members(layout: &Layout) -> Vec<String> {
    layout.members.iter().filter(|m| !member_supported(m)).map(member_label).collect()
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn i16_at(b: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([b[at], b[at + 1]])
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn put_f32(b: &mut [u8], at: usize, v: f32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_i16(b: &mut [u8], at: usize, v: i16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn byte_norm(b: u8) -> f32 {
    (f32::from(b) - 127.0) / 127.0
}

fn to_byte_norm(v: f32) -> u8 {
    (v * 127.0 + 127.0).round().clamp(0.0, 254.0) as u8
}

fn short_norm(s: i16) -> f32 {
    f32::from(s) / 32767.0
}

fn to_short_norm(v: f32) -> i16 {
    (v * 32767.0).round().clamp(-32767.0, 32767.0) as i16
}

fn ushort_norm(s: u16) -> f32 {
    (f32::from(s) - 32767.0) / 32767.0
}

fn to_ushort_norm(v: f32) -> u16 {
    (v * 32767.0 + 32767.0).round().clamp(0.0, 65534.0) as u16
}

/// The slice of one member inside a vertex.
fn slice<'a>(vertex: &'a [u8], m: &LayoutMember) -> Result<&'a [u8]> {
    let width = type_size(m.ty).ok_or_else(|| VertexError::Unsupported(member_label(m)))?;
    let at = usize::try_from(m.struct_offset).map_err(|_| VertexError::Malformed(format!("{} has a negative offset", member_label(m))))?;
    vertex.get(at..at + width).ok_or_else(|| VertexError::Malformed(format!("{} reaches past the end of the vertex", member_label(m))))
}

fn slice_mut<'a>(vertex: &'a mut [u8], m: &LayoutMember) -> Result<&'a mut [u8]> {
    let width = type_size(m.ty).ok_or_else(|| VertexError::Unsupported(member_label(m)))?;
    let at = usize::try_from(m.struct_offset).map_err(|_| VertexError::Malformed(format!("{} has a negative offset", member_label(m))))?;
    vertex.get_mut(at..at + width).ok_or_else(|| VertexError::Malformed(format!("{} reaches past the end of the vertex", member_label(m))))
}

/// Reads the vector of a normal, tangent or bitangent member (three values and the fourth as stored).
fn read_vector(m: &LayoutMember, b: &[u8]) -> Result<[f32; 4]> {
    use ty::*;
    Ok(match m.ty {
        FLOAT3 => [f32_at(b, 0), f32_at(b, 4), f32_at(b, 8), 0.0],
        FLOAT4 => [f32_at(b, 0), f32_at(b, 4), f32_at(b, 8), f32_at(b, 12)],
        BYTE4A | BYTE4B | BYTE4C => [byte_norm(b[0]), byte_norm(b[1]), byte_norm(b[2]), byte_norm(b[3])],
        SHORT4_TO_FLOAT4A => [short_norm(i16_at(b, 0)), short_norm(i16_at(b, 2)), short_norm(i16_at(b, 4)), f32::from(i16_at(b, 6))],
        SHORT4_TO_FLOAT4B => [ushort_norm(u16_at(b, 0)), ushort_norm(u16_at(b, 2)), ushort_norm(u16_at(b, 4)), f32::from(i16_at(b, 6))],
        _ => return Err(VertexError::Unsupported(member_label(m))),
    })
}

/// Writes the first three values of a vector into a member; the fourth value (padding or handedness) stays as it is in `b`
/// unless the member stores floats, where `w` is written when given.
fn write_vector(m: &LayoutMember, b: &mut [u8], v: [f32; 3], w: Option<f32>) -> Result<()> {
    use ty::*;
    match m.ty {
        FLOAT3 => {
            for (k, x) in v.iter().enumerate() {
                put_f32(b, k * 4, *x);
            }
        }
        FLOAT4 => {
            for (k, x) in v.iter().enumerate() {
                put_f32(b, k * 4, *x);
            }
            if let Some(w) = w {
                put_f32(b, 12, w);
            }
        }
        BYTE4A | BYTE4B | BYTE4C => {
            for (k, x) in v.iter().enumerate() {
                b[k] = to_byte_norm(*x);
            }
        }
        SHORT4_TO_FLOAT4A => {
            for (k, x) in v.iter().enumerate() {
                put_i16(b, k * 2, to_short_norm(*x));
            }
        }
        SHORT4_TO_FLOAT4B => {
            for (k, x) in v.iter().enumerate() {
                b[k * 2..k * 2 + 2].copy_from_slice(&to_ushort_norm(*x).to_le_bytes());
            }
        }
        _ => return Err(VertexError::Unsupported(member_label(m))),
    }
    Ok(())
}

/// Reads every known member of one vertex.
pub fn decode(layout: &Layout, vertex: &[u8], codec: Codec) -> Result<Values> {
    use ty::*;
    let mut v = Values::default();
    for m in &layout.members {
        if !member_supported(m) {
            // an unknown member is simply not read
            continue;
        }
        let b = slice(vertex, m)?;
        match m.semantic {
            sem::POSITION => v.position = Some([f32_at(b, 0), f32_at(b, 4), f32_at(b, 8)]),
            sem::NORMAL => {
                let r = read_vector(m, b)?;
                v.normal = Some([r[0], r[1], r[2]]);
            }
            sem::TANGENT => v.tangents.push(read_vector(m, b)?),
            sem::BITANGENT => {
                let r = read_vector(m, b)?;
                v.bitangent = Some([r[0], r[1], r[2]]);
            }
            sem::UV => match m.ty {
                FLOAT2 => v.uvs.push([f32_at(b, 0), f32_at(b, 4)]),
                FLOAT4 => {
                    v.uvs.push([f32_at(b, 0), f32_at(b, 4)]);
                    v.uvs.push([f32_at(b, 8), f32_at(b, 12)]);
                }
                BYTE4A | BYTE4B | SHORT2_TO_FLOAT2 | UV => v.uvs.push([f32::from(i16_at(b, 0)) / codec.uv_factor, f32::from(i16_at(b, 2)) / codec.uv_factor]),
                UV_PAIR => {
                    v.uvs.push([f32::from(i16_at(b, 0)) / codec.uv_factor, f32::from(i16_at(b, 2)) / codec.uv_factor]);
                    v.uvs.push([f32::from(i16_at(b, 4)) / codec.uv_factor, f32::from(i16_at(b, 6)) / codec.uv_factor]);
                }
                _ => return Err(VertexError::Unsupported(member_label(m))),
            },
            sem::BONE_INDICES => {
                v.bone_indices = Some(match m.ty {
                    SHORT_BONE_INDICES => [u16_at(b, 0), u16_at(b, 2), u16_at(b, 4), u16_at(b, 6)],
                    _ => [u16::from(b[0]), u16::from(b[1]), u16::from(b[2]), u16::from(b[3])],
                });
            }
            sem::BONE_WEIGHTS => {
                v.bone_weights = Some(match m.ty {
                    BYTE4A => [f32::from(b[0] as i8) / 127.0, f32::from(b[1] as i8) / 127.0, f32::from(b[2] as i8) / 127.0, f32::from(b[3] as i8) / 127.0],
                    BYTE4C => [f32::from(b[0]) / 255.0, f32::from(b[1]) / 255.0, f32::from(b[2]) / 255.0, f32::from(b[3]) / 255.0],
                    SHORT2_TO_FLOAT2 => [short_norm(i16_at(b, 0)), short_norm(i16_at(b, 2)), 0.0, 0.0],
                    _ => [short_norm(i16_at(b, 0)), short_norm(i16_at(b, 2)), short_norm(i16_at(b, 4)), short_norm(i16_at(b, 6))],
                });
            }
            sem::VERTEX_COLOR => v.has_color = true,
            _ => {}
        }
    }
    Ok(v)
}

fn uv_to_short(v: f32, codec: Codec, what: &str) -> Result<i16> {
    let s = (v * codec.uv_factor).round();
    if !s.is_finite() || !(-32768.0..=32767.0).contains(&s) {
        return Err(VertexError::OutOfRange(format!("texture coordinate {v} of {what} does not fit a 16-bit number at factor {}", codec.uv_factor)));
    }
    Ok(s as i16)
}

/// Writes the known members of `values` over a copy of `template` (a vertex of the model that is replaced, with the layout's
/// size). A value the layout has no member for is ignored; a member the values have nothing for keeps the template's bytes.
pub fn encode(layout: &Layout, template: &[u8], values: &Values, codec: Codec) -> Result<Vec<u8>> {
    use ty::*;
    let mut out = template.to_vec();
    let mut tangent_slot = 0usize;
    let mut uv_slot = 0usize;
    for m in &layout.members {
        if !member_supported(m) {
            continue;
        }
        let b = slice_mut(&mut out, m)?;
        match m.semantic {
            sem::POSITION => {
                if let Some(p) = values.position {
                    for (k, x) in p.iter().enumerate() {
                        put_f32(b, k * 4, *x);
                    }
                }
            }
            sem::NORMAL => {
                if let Some(n) = values.normal {
                    write_vector(m, b, n, None)?;
                }
            }
            sem::TANGENT => {
                if let Some(t) = values.tangents.get(tangent_slot) {
                    write_vector(m, b, [t[0], t[1], t[2]], Some(t[3]))?;
                }
                tangent_slot += 1;
            }
            sem::BITANGENT => {
                if let Some(t) = values.bitangent {
                    write_vector(m, b, t, None)?;
                }
            }
            sem::UV => {
                let sets = if matches!(m.ty, FLOAT4 | UV_PAIR) { 2 } else { 1 };
                for k in 0..sets {
                    let Some(uv) = values.uvs.get(uv_slot + k).or_else(|| values.uvs.first()) else { continue };
                    match m.ty {
                        FLOAT2 | FLOAT4 => {
                            put_f32(b, k * 8, uv[0]);
                            put_f32(b, k * 8 + 4, uv[1]);
                        }
                        _ => {
                            put_i16(b, k * 4, uv_to_short(uv[0], codec, "u")?);
                            put_i16(b, k * 4 + 2, uv_to_short(uv[1], codec, "v")?);
                        }
                    }
                }
                uv_slot += sets;
            }
            sem::BONE_INDICES => {
                if let Some(i) = values.bone_indices {
                    for (k, x) in i.iter().enumerate() {
                        if m.ty == SHORT_BONE_INDICES {
                            b[k * 2..k * 2 + 2].copy_from_slice(&x.to_le_bytes());
                        } else {
                            b[k] = u8::try_from(*x).map_err(|_| VertexError::OutOfRange(format!("bone number {x} does not fit one byte")))?;
                        }
                    }
                }
            }
            sem::BONE_WEIGHTS => {
                if let Some(w) = values.bone_weights {
                    match m.ty {
                        BYTE4A => {
                            for (k, x) in w.iter().enumerate() {
                                b[k] = (x * 127.0).round().clamp(0.0, 127.0) as u8;
                            }
                        }
                        BYTE4C => {
                            for (k, x) in w.iter().enumerate() {
                                b[k] = (x * 255.0).round().clamp(0.0, 255.0) as u8;
                            }
                        }
                        SHORT2_TO_FLOAT2 => {
                            for (k, x) in w.iter().take(2).enumerate() {
                                put_i16(b, k * 2, to_short_norm(*x));
                            }
                        }
                        _ => {
                            for (k, x) in w.iter().enumerate() {
                                put_i16(b, k * 2, to_short_norm(*x));
                            }
                        }
                    }
                }
            }
            sem::VERTEX_COLOR if values.has_color => {
                if m.ty == FLOAT4 {
                    for k in 0..4 {
                        put_f32(b, k * 4, 1.0);
                    }
                } else {
                    b.fill(255);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// A vertex that is white, opaque and bound with full weight to the first bone, around a position, a normal, a tangent and a
/// texture coordinate: what the model swap writes for every vertex of the new shape.
pub fn rigid_vertex(position: [f32; 3], normal: [f32; 3], tangent: [f32; 4], bitangent: [f32; 3], uv: [f32; 2], bone: u16) -> Values {
    Values { position: Some(position), normal: Some(normal), tangents: vec![tangent, tangent], bitangent: Some(bitangent), uvs: vec![uv, uv], bone_indices: Some([bone, 0, 0, 0]), bone_weights: Some([1.0, 0.0, 0.0, 0.0]), has_color: true }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flver::Layout;

    fn layout(members: &[(u32, u32, i32)]) -> Layout {
        let mut offset = 0;
        Layout {
            members: members
                .iter()
                .map(|&(ty, semantic, index)| {
                    let m = LayoutMember { unk00: 0, struct_offset: offset, ty, semantic, index };
                    offset += type_size(ty).unwrap() as i32;
                    m
                })
                .collect(),
        }
    }

    const CODEC: Codec = Codec { uv_factor: 2048.0 };

    #[test]
    fn known_members_are_listed_and_unknown_ones_are_named() {
        let l = layout(&[(ty::FLOAT3, sem::POSITION, 0), (ty::BYTE4C, sem::NORMAL, 0), (ty::BYTE4E, sem::BONE_INDICES, 0), (0x2E, sem::BONE_WEIGHTS, 0)]);
        assert_eq!(unsupported_members(&l), vec!["BoneWeights Short4toFloat4B".to_string()]);
        assert!(unsupported_members(&layout(&[(ty::FLOAT3, sem::POSITION, 0), (ty::UV_PAIR, sem::UV, 0), (ty::SHORT4_TO_FLOAT4A, sem::BONE_WEIGHTS, 0)])).is_empty());
    }

    #[test]
    fn a_typical_weapon_layout_round_trips_through_encode_and_decode() {
        let l = layout(&[
            (ty::FLOAT3, sem::POSITION, 0),
            (ty::BYTE4C, sem::NORMAL, 0),
            (ty::BYTE4C, sem::TANGENT, 0),
            (ty::BYTE4C, sem::BITANGENT, 0),
            (ty::BYTE4C, sem::VERTEX_COLOR, 0),
            (ty::UV_PAIR, sem::UV, 0),
            (ty::BYTE4B, sem::BONE_INDICES, 0),
            (ty::SHORT4_TO_FLOAT4A, sem::BONE_WEIGHTS, 0),
        ]);
        let size = l.size().unwrap();
        // a template with recognisable padding in the fourth byte of the three vectors
        let mut template = vec![0u8; size];
        template[15] = 0x77; // normal W
        template[19] = 0x66; // tangent W
        template[23] = 0x55; // bitangent W
        let v = rigid_vertex([1.5, -2.25, 3.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.25, -0.5], 3);
        let bytes = encode(&l, &template, &v, CODEC).unwrap();
        assert_eq!(bytes.len(), size);
        assert_eq!((bytes[15], bytes[19], bytes[23]), (0x77, 0x66, 0x55), "the padding the template had is kept");
        let back = decode(&l, &bytes, CODEC).unwrap();
        assert_eq!(back.position, Some([1.5, -2.25, 3.0]));
        let n = back.normal.unwrap();
        assert!(n[0].abs() < 0.01 && (n[1] - 1.0).abs() < 0.01 && n[2].abs() < 0.01, "{n:?}");
        assert_eq!(back.uvs, vec![[0.25, -0.5], [0.25, -0.5]]);
        assert_eq!(back.bone_indices, Some([3, 0, 0, 0]));
        let w = back.bone_weights.unwrap();
        assert!((w[0] - 1.0).abs() < 1e-4 && w[1] == 0.0, "{w:?}");
        assert!(back.has_color);
        // encoding what was decoded gives the same bytes (the check the model swap makes on the game's own vertices)
        let again = encode(&l, &bytes, &back, CODEC).unwrap();
        assert_eq!(again, bytes);
    }

    #[test]
    fn the_other_storage_types_round_trip_too() {
        let l = layout(&[
            (ty::FLOAT3, sem::POSITION, 0),
            (ty::FLOAT3, sem::NORMAL, 0),
            (ty::FLOAT4, sem::TANGENT, 0),
            (ty::SHORT2_TO_FLOAT2, sem::UV, 0),
            (ty::SHORT_BONE_INDICES, sem::BONE_INDICES, 0),
            (ty::BYTE4C, sem::BONE_WEIGHTS, 0),
            (ty::FLOAT4, sem::VERTEX_COLOR, 0),
        ]);
        let template = vec![0u8; l.size().unwrap()];
        let v = rigid_vertex([0.5, 0.25, -1.0], [0.6, 0.0, 0.8], [0.0, 1.0, 0.0, -1.0], [0.0; 3], [1.0, 0.5], 300);
        let bytes = encode(&l, &template, &v, CODEC).unwrap();
        let back = decode(&l, &bytes, CODEC).unwrap();
        assert_eq!(back.position, v.position);
        assert_eq!(back.normal, v.normal);
        assert_eq!(back.tangents, vec![[0.0, 1.0, 0.0, -1.0]]);
        assert_eq!(back.uvs, vec![[1.0, 0.5]]);
        assert_eq!(back.bone_indices, Some([300, 0, 0, 0]));
        assert_eq!(back.bone_weights, Some([1.0, 0.0, 0.0, 0.0]));
        assert_eq!(encode(&l, &bytes, &back, CODEC).unwrap(), bytes);
        // a bone number that cannot be stored in one byte is refused
        let small = layout(&[(ty::BYTE4B, sem::BONE_INDICES, 0)]);
        assert!(matches!(encode(&small, &[0; 4], &v, CODEC), Err(VertexError::OutOfRange(_))));
    }

    #[test]
    fn texture_coordinates_outside_the_range_of_16_bits_are_refused_not_clipped() {
        let l = layout(&[(ty::SHORT2_TO_FLOAT2, sem::UV, 0)]);
        let v = Values { uvs: vec![[20.0, 0.0]], ..Values::default() };
        assert!(matches!(encode(&l, &[0; 4], &v, CODEC), Err(VertexError::OutOfRange(_))));
        let ok = Values { uvs: vec![[15.9, -15.9]], ..Values::default() };
        let bytes = encode(&l, &[0; 4], &ok, CODEC).unwrap();
        let back = decode(&l, &bytes, CODEC).unwrap();
        assert!((back.uvs[0][0] - 15.9).abs() < 0.001 && (back.uvs[0][1] + 15.9).abs() < 0.001);
    }

    #[test]
    fn members_that_are_not_known_keep_the_template_bytes_and_a_short_vertex_is_an_error() {
        let l = layout(&[(ty::FLOAT3, sem::POSITION, 0), (ty::SHORT2_TO_FLOAT2, 99, 0)]);
        let mut template = vec![0u8; 16];
        template[12..16].copy_from_slice(&[9, 8, 7, 6]);
        let v = Values { position: Some([1.0, 2.0, 3.0]), ..Values::default() };
        let bytes = encode(&l, &template, &v, CODEC).unwrap();
        assert_eq!(&bytes[12..], &[9, 8, 7, 6]);
        assert!(decode(&l, &[0; 8], CODEC).is_err());
        assert!(encode(&l, &[0; 8], &v, CODEC).is_err());
        assert_eq!(Codec::for_version(0x20014).uv_factor, 2048.0);
        assert_eq!(Codec::for_version(0x2000C).uv_factor, 1024.0);
    }

    #[test]
    fn byte_conventions_are_symmetric_and_clamped() {
        for b in 0..=254u8 {
            assert_eq!(to_byte_norm(byte_norm(b)), b);
        }
        assert_eq!(to_byte_norm(2.0), 254);
        assert_eq!(to_byte_norm(-2.0), 0);
        for s in [-32767i16, -1, 0, 1, 12345, 32767] {
            assert_eq!(to_short_norm(short_norm(s)), s);
        }
        for s in [0u16, 1, 32767, 65534] {
            assert_eq!(to_ushort_norm(ushort_norm(s)), s);
        }
    }
}
