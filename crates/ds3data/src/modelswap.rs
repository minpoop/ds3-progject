//! Puts a new shape into a Dark Souls III weapon model while everything else of the model stays the game's own: the bones,
//! the dummy points the game uses to hold and aim the weapon, the materials (shader and texture names) and the layout of
//! the vertices. The new shape replaces the geometry of the biggest mesh and the other meshes are dropped.
//!
//! The work is checked at every step and fails closed (an error says why, nothing half-made is returned):
//!
//! 1. the layout of the model's vertices must be one that [`crate::vertex`] knows member by member;
//! 2. the way that code reads the vertices must make sense on the game's own vertices (unit normals, texture coordinates in
//!    range, weights adding up to one) and writing them again must give the very same bytes - this is the check that the
//!    community's notes about the vertex formats are right for this player's files;
//! 3. the result is written, read back and its vertices are decoded again and compared with what was meant.
use crate::flver::{BoundingBox, FaceSet, Flver, Mesh, VertexBuffer};
use crate::vertex::{self, Codec, Values};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapError {
    /// The new shape is not usable (counts that do not fit, indices past the end, too many vertices).
    BadShape(String),
    /// The model cannot take a new shape (no mesh, several vertex buffers, a vertex member that is not known).
    CannotUse(String),
    /// The vertex notes do not fit this player's file: what the game's own vertices look like when read that way.
    ConventionsDoNotFit(String),
    /// The result did not read back as intended.
    CheckFailed(String),
}

impl fmt::Display for SwapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SwapError::BadShape(s) => write!(f, "the new shape cannot be used: {s}"),
            SwapError::CannotUse(s) => write!(f, "the model cannot take a new shape: {s}"),
            SwapError::ConventionsDoNotFit(s) => write!(f, "the vertex format of this model is not read the way expected: {s}"),
            SwapError::CheckFailed(s) => write!(f, "the new model did not pass its own check: {s}"),
        }
    }
}

impl std::error::Error for SwapError {}

type Result<T> = std::result::Result<T, SwapError>;

/// The most vertices of the new shape (16-bit indices; the game's weapons are far below this).
pub const MAX_VERTICES: usize = 65_000;

/// A shape in the weapon's own space (metres, the axes of the model file).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewShape {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// x, y, z and the handedness (1 or -1); empty to compute them from the texture coordinates.
    pub tangents: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub triangles: Vec<[u32; 3]>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

fn normalized(a: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let l = length(a);
    if l > 1e-12 && l.is_finite() {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        fallback
    }
}

impl NewShape {
    /// Checks the counts and the indices, and fills in what is missing: unit normals, tangents from the texture coordinates.
    pub fn prepared(mut self) -> Result<NewShape> {
        let n = self.positions.len();
        if n == 0 || self.triangles.is_empty() {
            return Err(SwapError::BadShape("it has no vertices or no triangles".to_string()));
        }
        if n > MAX_VERTICES {
            return Err(SwapError::BadShape(format!("{n} vertices are more than the {MAX_VERTICES} the model format used here takes")));
        }
        if self.normals.len() != n || self.uvs.len() != n || !(self.tangents.is_empty() || self.tangents.len() == n) {
            return Err(SwapError::BadShape(format!("{n} positions, {} normals, {} texture coordinates and {} tangents do not fit each other", self.normals.len(), self.uvs.len(), self.tangents.len())));
        }
        if self.positions.iter().flatten().chain(self.normals.iter().flatten()).chain(self.uvs.iter().flatten()).any(|v| !v.is_finite()) {
            return Err(SwapError::BadShape("a value is not a number".to_string()));
        }
        if let Some(bad) = self.triangles.iter().flatten().find(|&&i| i as usize >= n) {
            return Err(SwapError::BadShape(format!("a triangle uses vertex {bad}, but there are only {n}")));
        }
        for v in &mut self.normals {
            *v = normalized(*v, [0.0, 1.0, 0.0]);
        }
        if self.tangents.is_empty() {
            self.tangents = compute_tangents(&self.positions, &self.normals, &self.uvs, &self.triangles);
        }
        Ok(self)
    }

    /// The box around the positions.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &self.positions {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        (lo, hi)
    }
}

/// Per-vertex tangents from the triangles' texture coordinate derivatives, made orthogonal to the normal.
pub fn compute_tangents(positions: &[[f32; 3]], normals: &[[f32; 3]], uvs: &[[f32; 2]], triangles: &[[u32; 3]]) -> Vec<[f32; 4]> {
    let n = positions.len();
    let mut tan = vec![[0f32; 3]; n];
    let mut bit = vec![[0f32; 3]; n];
    for t in triangles {
        let (i0, i1, i2) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let (e1, e2) = (sub(positions[i1], positions[i0]), sub(positions[i2], positions[i0]));
        let (du1, dv1) = (uvs[i1][0] - uvs[i0][0], uvs[i1][1] - uvs[i0][1]);
        let (du2, dv2) = (uvs[i2][0] - uvs[i0][0], uvs[i2][1] - uvs[i0][1]);
        let det = du1 * dv2 - du2 * dv1;
        if det.abs() < 1e-20 {
            continue;
        }
        let r = 1.0 / det;
        let sdir = [(e1[0] * dv2 - e2[0] * dv1) * r, (e1[1] * dv2 - e2[1] * dv1) * r, (e1[2] * dv2 - e2[2] * dv1) * r];
        let tdir = [(e2[0] * du1 - e1[0] * du2) * r, (e2[1] * du1 - e1[1] * du2) * r, (e2[2] * du1 - e1[2] * du2) * r];
        for &i in &[i0, i1, i2] {
            for a in 0..3 {
                tan[i][a] += sdir[a];
                bit[i][a] += tdir[a];
            }
        }
    }
    (0..n)
        .map(|i| {
            let nrm = normals[i];
            let t = tan[i];
            // Gram-Schmidt: the part of the tangent that is not along the normal
            let d = dot(nrm, t);
            let ortho = normalized([t[0] - nrm[0] * d, t[1] - nrm[1] * d, t[2] - nrm[2] * d], any_perpendicular(nrm));
            let w = if dot(cross(nrm, ortho), bit[i]) < 0.0 { -1.0 } else { 1.0 };
            [ortho[0], ortho[1], ortho[2], w]
        })
        .collect()
}

fn any_perpendicular(n: [f32; 3]) -> [f32; 3] {
    let axis = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    normalized(cross(n, axis), [1.0, 0.0, 0.0])
}

/// The result of reading the game's own vertices the expected way.
#[derive(Debug, Clone, PartialEq)]
pub struct ConventionReport {
    pub vertices: usize,
    /// Share (0..1) of vertices whose normal has unit length, of those that have one.
    pub normals_unit: f32,
    pub tangents_unit: f32,
    pub weights_sum_one: f32,
    pub uvs_in_range: f32,
    /// Share of vertices that give the very same bytes after reading and writing.
    pub bytes_identical: f32,
    pub first_mismatch: Option<String>,
}

impl ConventionReport {
    pub fn lines(&self) -> Vec<String> {
        let pct = |v: f32| format!("{:.1}%", v * 100.0);
        vec![format!(
            "reading the game's own {} vertices the expected way: normals of unit length {}, tangents {}, weights adding up to one {}, texture coordinates within +-16 {}, written back to the very same bytes {}",
            self.vertices,
            pct(self.normals_unit),
            pct(self.tangents_unit),
            pct(self.weights_sum_one),
            pct(self.uvs_in_range),
            pct(self.bytes_identical)
        )]
    }

    /// Whether the numbers say the notes fit: nearly every vertex reads sensibly and writes back identically.
    pub fn fits(&self) -> bool {
        self.normals_unit >= 0.95 && self.tangents_unit >= 0.95 && self.weights_sum_one >= 0.95 && self.uvs_in_range >= 0.99 && self.bytes_identical >= 0.99
    }
}

/// Reads every vertex of `buffer` the expected way and says how sensible the values are.
pub fn check_conventions(model: &Flver, buffer: &VertexBuffer) -> Result<ConventionReport> {
    let layout = usize::try_from(buffer.layout_index).ok().and_then(|i| model.layouts.get(i)).ok_or_else(|| SwapError::CannotUse("the vertex buffer names a layout that does not exist".to_string()))?;
    let size = usize::try_from(buffer.vertex_size).ok().filter(|s| *s > 0).ok_or_else(|| SwapError::CannotUse("the vertex size is zero".to_string()))?;
    let codec = Codec::for_version(model.header.version);
    let n = buffer.data.len() / size;
    let (mut normals, mut with_normal) = (0usize, 0usize);
    let (mut tangents, mut with_tangent) = (0usize, 0usize);
    let (mut weights, mut with_weights) = (0usize, 0usize);
    let (mut uvs_ok, mut with_uv) = (0usize, 0usize);
    let mut identical = 0usize;
    let mut first_mismatch = None;
    for (k, bytes) in buffer.data.chunks_exact(size).enumerate() {
        let mut v = vertex::decode(layout, bytes, codec).map_err(|e| SwapError::CannotUse(e.to_string()))?;
        if let Some(nrm) = v.normal {
            with_normal += 1;
            if (length(nrm) - 1.0).abs() < 0.08 {
                normals += 1;
            }
        }
        if let Some(t) = v.tangents.first() {
            with_tangent += 1;
            if (length([t[0], t[1], t[2]]) - 1.0).abs() < 0.08 {
                tangents += 1;
            }
        }
        if let Some(w) = v.bone_weights {
            with_weights += 1;
            if (w.iter().sum::<f32>() - 1.0).abs() < 0.05 {
                weights += 1;
            }
        }
        if !v.uvs.is_empty() {
            with_uv += 1;
            if v.uvs.iter().flatten().all(|x| x.is_finite() && x.abs() <= 16.0) {
                uvs_ok += 1;
            }
        }
        v.has_color = false; // colours are not written back by value
        match vertex::encode(layout, bytes, &v, codec) {
            Ok(again) if again == bytes => identical += 1,
            Ok(again) => {
                if first_mismatch.is_none() {
                    first_mismatch = Some(format!("vertex {k}: read {} and written again {}", crate::util::hex(bytes), crate::util::hex(&again)));
                }
            }
            Err(e) => {
                if first_mismatch.is_none() {
                    first_mismatch = Some(format!("vertex {k}: {e}"));
                }
            }
        }
    }
    let share = |good: usize, of: usize| if of == 0 { 1.0 } else { good as f32 / of as f32 };
    Ok(ConventionReport {
        vertices: n,
        normals_unit: share(normals, with_normal),
        tangents_unit: share(tangents, with_tangent),
        weights_sum_one: share(weights, with_weights),
        uvs_in_range: share(uvs_ok, with_uv),
        bytes_identical: share(identical, n),
        first_mismatch,
    })
}

/// The index of the mesh with the most vertices.
pub fn biggest_mesh(model: &Flver) -> Option<usize> {
    model.meshes.iter().enumerate().max_by_key(|(_, m)| m.vertex_buffers.iter().map(|b| b.vertex_count.max(0)).sum::<i32>()).map(|(i, _)| i)
}

/// What a swap did, for the report.
#[derive(Debug, Clone, Default)]
pub struct SwapOutcome {
    pub model: Option<Flver>,
    pub lines: Vec<String>,
}

fn box_union(a: ([f32; 3], [f32; 3]), b: ([f32; 3], [f32; 3])) -> ([f32; 3], [f32; 3]) {
    let mut lo = a.0;
    let mut hi = a.1;
    for k in 0..3 {
        lo[k] = lo[k].min(b.0[k]);
        hi[k] = hi[k].max(b.1[k]);
    }
    (lo, hi)
}

/// Replaces the geometry of the biggest mesh with `shape` and drops the other meshes. See the module notes for the checks.
pub fn replace_geometry(model: &Flver, shape: NewShape) -> Result<SwapOutcome> {
    let shape = shape.prepared()?;
    let mut lines = Vec::new();
    let host_index = biggest_mesh(model).ok_or_else(|| SwapError::CannotUse("the model has no mesh".to_string()))?;
    let host = &model.meshes[host_index];
    if host.vertex_buffers.len() != 1 {
        return Err(SwapError::CannotUse(format!("the biggest mesh has {} vertex buffers (one is expected)", host.vertex_buffers.len())));
    }
    let original = &host.vertex_buffers[0];
    let layout_index = usize::try_from(original.layout_index).map_err(|_| SwapError::CannotUse("a negative layout number".to_string()))?;
    let layout = model.layouts.get(layout_index).ok_or_else(|| SwapError::CannotUse("the vertex buffer names a layout that does not exist".to_string()))?;
    let unknown = vertex::unsupported_members(layout);
    if !unknown.is_empty() {
        return Err(SwapError::CannotUse(format!("the vertex layout ({}) has members that are not known: {}", layout.describe(), unknown.join(", "))));
    }
    let size = usize::try_from(original.vertex_size).ok().filter(|s| *s > 0 && layout.size() == Some(*s)).ok_or_else(|| SwapError::CannotUse("the vertex size does not fit its layout".to_string()))?;
    if original.data.len() < size {
        return Err(SwapError::CannotUse("the biggest mesh has no vertices".to_string()));
    }
    lines.push(format!("host mesh {host_index} of {}: material {}, {} vertices of {size} bytes ({})", model.meshes.len(), host.material_index, original.data.len() / size, layout.describe()));

    // the game's own vertices must read the way the notes say
    let report = check_conventions(model, original)?;
    lines.extend(report.lines());
    if !report.fits() {
        return Err(SwapError::ConventionsDoNotFit(format!("{}{}", report.lines().join(" "), report.first_mismatch.as_ref().map_or(String::new(), |m| format!(" (first difference: {m})")))));
    }

    // the new vertices: the first original vertex is the template (bone numbers, weights, padding and colours stay its own)
    let codec = Codec::for_version(model.header.version);
    let template = &original.data[..size];
    let mut data = Vec::with_capacity(shape.positions.len() * size);
    for i in 0..shape.positions.len() {
        let t = shape.tangents[i];
        let bitangent = {
            let b = cross(shape.normals[i], [t[0], t[1], t[2]]);
            normalized([b[0] * t[3], b[1] * t[3], b[2] * t[3]], [0.0, 0.0, 1.0])
        };
        let values = Values { position: Some(shape.positions[i]), normal: Some(shape.normals[i]), tangents: vec![t, t], bitangent: Some(bitangent), uvs: vec![shape.uvs[i], shape.uvs[i]], bone_indices: None, bone_weights: None, has_color: false };
        data.extend(vertex::encode(layout, template, &values, codec).map_err(|e| SwapError::CannotUse(e.to_string()))?);
    }

    let count = shape.positions.len();
    let flat: Vec<u32> = shape.triangles.iter().flatten().copied().collect();
    let (lo, hi) = shape.bounds();
    let mut new_mesh: Mesh = host.clone();
    new_mesh.vertex_buffers = vec![VertexBuffer { layout_index: original.layout_index, vertex_size: original.vertex_size, vertex_count: count as i32, data }];
    new_mesh.face_sets = host
        .face_sets
        .iter()
        .map(|f| FaceSet { flags: f.flags, triangle_strip: false, cull_backfaces: f.cull_backfaces, unk06: f.unk06, indices: flat.clone(), index_bits: 16 })
        .collect();
    if new_mesh.face_sets.is_empty() {
        new_mesh.face_sets.push(FaceSet { flags: 0, triangle_strip: false, cull_backfaces: true, unk06: 0, indices: flat.clone(), index_bits: 16 });
    }
    if new_mesh.bounding_box.is_some() {
        new_mesh.bounding_box = Some(BoundingBox { min: lo, max: hi });
    }
    let mut out = model.clone();
    out.meshes = vec![new_mesh];
    out.header.bounding_box_min = lo;
    out.header.bounding_box_max = hi;
    // the bones the mesh is attached to enclose the new shape (the game may use their boxes to decide what to draw)
    let mut bones: Vec<usize> = host.bone_indices.iter().filter_map(|b| usize::try_from(*b).ok()).collect();
    if let Ok(n) = usize::try_from(host.node_index) {
        bones.push(n);
    }
    for b in bones {
        if let Some(node) = out.nodes.get_mut(b) {
            let merged = box_union((node.bounding_box_min, node.bounding_box_max), (lo, hi));
            node.bounding_box_min = merged.0;
            node.bounding_box_max = merged.1;
        }
    }
    lines.push(format!(
        "new shape: {count} vertices, {} triangles in {} face sets, box ({:.3}, {:.3}, {:.3}) to ({:.3}, {:.3}, {:.3}); the other {} meshes of the model are dropped",
        shape.triangles.len(),
        out.meshes[0].face_sets.len(),
        lo[0],
        lo[1],
        lo[2],
        hi[0],
        hi[1],
        hi[2],
        model.meshes.len() - 1
    ));

    // the check: write, read back, decode the vertices again
    let bytes = out.write().map_err(|e| SwapError::CheckFailed(format!("the model cannot be written: {e}")))?;
    let back = Flver::parse(&bytes).map_err(|e| SwapError::CheckFailed(format!("the written model cannot be read back: {e}")))?;
    if back.meshes.len() != 1 || back.meshes[0].vertex_buffers.first().map(|b| b.vertex_count) != Some(count as i32) {
        return Err(SwapError::CheckFailed("the vertex count after reading back is not the one written".to_string()));
    }
    if back.triangles(0).len() != shape.triangles.len() {
        return Err(SwapError::CheckFailed(format!("{} triangles after reading back instead of {}", back.triangles(0).len(), shape.triangles.len())));
    }
    let readback = &back.meshes[0].vertex_buffers[0];
    let mut worst_position = 0f32;
    let mut worst_uv = 0f32;
    for (i, bytes) in readback.data.chunks_exact(size).enumerate() {
        let v = vertex::decode(layout, bytes, codec).map_err(|e| SwapError::CheckFailed(e.to_string()))?;
        if let Some(p) = v.position {
            worst_position = worst_position.max(length(sub(p, shape.positions[i])));
        }
        if let Some(uv) = v.uvs.first() {
            worst_uv = worst_uv.max((uv[0] - shape.uvs[i][0]).abs()).max((uv[1] - shape.uvs[i][1]).abs());
        }
    }
    if worst_position > 1e-4 || worst_uv > 1.0 / 1024.0 {
        return Err(SwapError::CheckFailed(format!("after reading back the positions differ by up to {worst_position} and the texture coordinates by up to {worst_uv}")));
    }
    lines.push(format!("check: the model written and read back has the same vertices (positions within {worst_position:.1e}, texture coordinates within {worst_uv:.1e})"));
    Ok(SwapOutcome { model: Some(out), lines })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::flver::sample_flver;

    fn square() -> NewShape {
        NewShape {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 2.0]; 4],
            tangents: Vec::new(),
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        }
    }

    #[test]
    fn tangents_follow_the_texture_coordinates() {
        let s = square().prepared().unwrap();
        assert_eq!(s.normals[0], [0.0, 0.0, 1.0], "normals are made unit length");
        for t in &s.tangents {
            assert!((t[0] - 1.0).abs() < 1e-5 && t[1].abs() < 1e-5 && t[2].abs() < 1e-5, "{t:?}");
            assert_eq!(t[3], 1.0);
        }
        // mirrored texture coordinates flip the handedness
        let mut m = square();
        for uv in &mut m.uvs {
            uv[0] = 1.0 - uv[0];
        }
        let s = m.prepared().unwrap();
        assert!(s.tangents.iter().all(|t| t[3] == -1.0 && (t[0] + 1.0).abs() < 1e-5));
    }

    #[test]
    fn a_shape_that_does_not_fit_is_refused_with_a_reason() {
        let mut s = square();
        s.uvs.pop();
        assert!(matches!(s.prepared(), Err(SwapError::BadShape(m)) if m.contains("do not fit each other")));
        let mut s = square();
        s.triangles.push([0, 1, 9]);
        assert!(matches!(s.prepared(), Err(SwapError::BadShape(m)) if m.contains("vertex 9")));
        let mut s = square();
        s.positions[2][1] = f32::NAN;
        assert!(matches!(s.prepared(), Err(SwapError::BadShape(m)) if m.contains("not a number")));
        assert!(matches!(NewShape::default().prepared(), Err(SwapError::BadShape(_))));
    }

    #[test]
    fn the_biggest_mesh_gets_the_new_shape_and_the_rest_of_the_model_is_kept() {
        let model = sample_flver();
        let outcome = replace_geometry(&model, square()).unwrap_or_else(|e| panic!("{e}"));
        let out = outcome.model.unwrap();
        // one mesh with the four vertices and the two triangles, in every face set of the host
        assert_eq!(out.meshes.len(), 1);
        let m = &out.meshes[0];
        assert_eq!((m.material_index, m.bone_indices.clone()), (0, vec![0, 1]), "the host's material and bones stay");
        assert_eq!(m.vertex_buffers[0].vertex_count, 4);
        assert_eq!(m.face_sets.len(), 2);
        assert!(m.face_sets.iter().all(|f| !f.triangle_strip && f.indices == vec![0, 1, 2, 0, 2, 3]));
        assert_eq!(m.face_sets[1].flags, 0x0100_0000, "the flags of the face sets stay");
        // bones, dummies and materials are untouched; the boxes enclose the new shape
        assert_eq!((out.nodes.len(), out.dummies.clone(), out.materials.clone()), (model.nodes.len(), model.dummies.clone(), model.materials.clone()));
        assert_eq!((out.header.bounding_box_min, out.header.bounding_box_max), ([0.0; 3], [1.0, 1.0, 0.0]));
        assert_eq!(m.bounding_box, Some(BoundingBox { min: [0.0; 3], max: [1.0, 1.0, 0.0] }));
        assert!(out.nodes[0].bounding_box_max[2] >= 1.0, "a bone box only grows");
        // what is written reads back with the same positions
        let back = Flver::parse(&out.write().unwrap()).unwrap();
        assert_eq!(back.positions(0).unwrap(), square().positions);
        assert_eq!(back.triangles(0), vec![[0, 1, 2], [0, 2, 3]]);
        assert!(outcome.lines.iter().any(|l| l.contains("written back to the very same bytes 100.0%")), "{:?}", outcome.lines);
        assert!(outcome.lines.iter().any(|l| l.contains("check: the model written and read back")));
    }

    #[test]
    fn a_vertex_format_that_is_not_read_the_expected_way_stops_the_swap() {
        // normals that are not unit vectors under the expected reading: the notes do not fit this file
        let mut model = sample_flver();
        for vb in &mut model.meshes[0].vertex_buffers {
            for v in vb.data.chunks_exact_mut(28) {
                v[12..16].copy_from_slice(&[10, 10, 10, 10]);
            }
        }
        match replace_geometry(&model, square()) {
            Err(SwapError::ConventionsDoNotFit(m)) => assert!(m.contains("normals of unit length 0.0%"), "{m}"),
            other => panic!("{other:?}"),
        }
        // a layout with a member nobody knows
        let mut model = sample_flver();
        model.layouts[0].members[1].semantic = 77;
        assert!(matches!(replace_geometry(&model, square()), Err(SwapError::CannotUse(m)) if m.contains("not known")));
        // no mesh at all
        let mut model = sample_flver();
        model.meshes.clear();
        assert!(matches!(replace_geometry(&model, square()), Err(SwapError::CannotUse(m)) if m.contains("no mesh")));
    }
}
