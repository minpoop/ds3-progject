//! Brings a Space Marine 2 weapon shape into a Dark Souls III weapon's space.
//!
//! * [`Sm2Shape::load`] reads the full-detail model of a template (the merged sub mesh) and finds the point the hand holds it by.
//! * [`Placement::fit`] decides how the shape is turned and scaled: the three axes of the new shape are matched to the three
//!   axes of the weapon it replaces by their lengths (longest to longest), the sides by where the bulk of each shape lies
//!   relative to the grip, and the size so that the lengths agree. Both models have their origin at the grip, so the grip
//!   stays where the game expects the hand.
//! * [`to_new_shape`] turns it all into the shape [`ashen_ds3data::modelswap`] puts into the model.
//! * [`overlay_png`] draws the weapon that is replaced and the new shape on top of it, in three views, so that anybody can
//!   look at the result before the game ever loads it.
//!
//! Nothing here reads a game file by itself; callers give it the decoded data.
use ashen_ds3data::flver::Flver;
use ashen_ds3data::modelswap::{biggest_mesh, NewShape};
use ashen_ds3data::vertex::{self, Codec};
use ashen_sm2::mesh::{decode_sub_mesh, DecodedMesh};
use ashen_sm2::texture::Image as PngImage;
use ashen_sm2::tpl::Template;

/// The full-detail shape of a Space Marine 2 weapon, positions relative to the grip.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sm2Shape {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub triangles: Vec<[u32; 3]>,
    /// Where the grip was found, for the report.
    pub grip_source: String,
    /// The texture its material names.
    pub texture: Option<String>,
}

fn center(lo: [f32; 3], hi: [f32; 3]) -> [f32; 3] {
    [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]
}

fn bounds_of(points: &[[f32; 3]]) -> Option<([f32; 3], [f32; 3])> {
    let first = points.first()?;
    let (mut lo, mut hi) = (*first, *first);
    for p in points {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    Some((lo, hi))
}

impl Sm2Shape {
    /// Reads the sub meshes of the full-detail model and joins them. Fails with a reason when the template has no such
    /// model or it does not decode.
    pub fn load(template: &Template, data: &[u8]) -> Result<Sm2Shape, String> {
        let Some(g) = &template.geometry else { return Err("the template has no geometry".to_string()) };
        let subs = template.full_detail_sub_meshes();
        if subs.is_empty() {
            return Err("no sub mesh belongs to the object that detail level 0 names".to_string());
        }
        let mut merged = DecodedMesh::default();
        for &i in &subs {
            let m = decode_sub_mesh(g, data, i).map_err(|e| format!("sub mesh {i} cannot be read: {e}"))?;
            let n = m.positions.len();
            if n == 0 || m.triangles.is_empty() {
                continue;
            }
            let base = merged.positions.len() as u32;
            merged.positions.extend(&m.positions);
            // a stream the sub mesh does not have is filled in, so that the lists stay the same length
            merged.normals.extend(if m.normals.len() == n { m.normals.clone() } else { vec![[0.0, 1.0, 0.0]; n] });
            merged.uvs.extend(if m.uvs.len() == n { m.uvs.clone() } else { vec![[0.0, 0.0]; n] });
            merged.tangents.extend(if m.tangents.len() == n { m.tangents.clone() } else { vec![[1.0, 0.0, 0.0, 1.0]; n] });
            merged.triangles.extend(m.triangles.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
        }
        let Some((lo, hi)) = bounds_of(&merged.positions) else { return Err("the full-detail model has no vertices".to_string()) };
        let mesh_center = center(lo, hi);
        // the hand holds the weapon at the origin of the model space; the merged mesh sits in the space of its own parts, which
        // is displaced from it. The rigid body box (named rb_wpn) is in model space and surrounds the same weapon, so the
        // difference of the two centres is the displacement.
        let rb = g.objects.iter().find(|o| o.name.as_deref() == Some("rb_wpn")).and_then(|o| o.bbox);
        let (grip, grip_source) = match rb {
            Some(b) => {
                let c = center([b[0], b[1], b[2]], [b[3], b[4], b[5]]);
                ([mesh_center[0] - c[0], mesh_center[1] - c[1], mesh_center[2] - c[2]], "the centre of the rigid body box (rb_wpn) against the centre of the mesh".to_string())
            }
            None => (mesh_center, "the centre of the mesh (the template has no rigid body box)".to_string()),
        };
        let positions: Vec<[f32; 3]> = merged.positions.iter().map(|p| [p[0] - grip[0], p[1] - grip[1], p[2] - grip[2]]).collect();
        let texture = template.full_detail_texture_names().into_iter().next();
        Ok(Sm2Shape { positions, normals: merged.normals, tangents: merged.tangents, uvs: merged.uvs, triangles: merged.triangles, grip_source, texture })
    }

    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        bounds_of(&self.positions)
    }
}

/// What the Dark Souls III weapon that is replaced looks like.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ds3Reference {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// The positions, normals and triangles of the biggest mesh of a model, or why they cannot be had.
pub fn ds3_reference(model: &Flver) -> Result<Ds3Reference, String> {
    let host = biggest_mesh(model).ok_or("the model has no mesh")?;
    let mesh = &model.meshes[host];
    let buffer = mesh.vertex_buffers.first().ok_or("the biggest mesh has no vertices")?;
    let layout = usize::try_from(buffer.layout_index).ok().and_then(|i| model.layouts.get(i)).ok_or("the vertex buffer names a layout that does not exist")?;
    let size = usize::try_from(buffer.vertex_size).ok().filter(|s| *s > 0).ok_or("the vertex size is zero")?;
    let codec = Codec::for_version(model.header.version);
    let mut out = Ds3Reference::default();
    for bytes in buffer.data.chunks_exact(size) {
        let v = vertex::decode(layout, bytes, codec).map_err(|e| e.to_string())?;
        out.positions.push(v.position.ok_or("the layout has no position")?);
        out.normals.push(v.normal.unwrap_or([0.0, 1.0, 0.0]));
    }
    out.triangles = model.triangles(host);
    Ok(out)
}

/// How a shape is turned and scaled: `out[i] = sign_i * in[axis_i] * scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub axis: [usize; 3],
    pub sign: [f32; 3],
    pub scale: f32,
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn centroid(points: &[[f32; 3]]) -> [f32; 3] {
    let n = points.len().max(1) as f32;
    let mut c = [0f32; 3];
    for p in points {
        for a in 0..3 {
            c[a] += p[a] / n;
        }
    }
    c
}

fn order_by_length(extent: [f32; 3]) -> [usize; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|a, b| extent[*b].total_cmp(&extent[*a]).then(a.cmp(b)));
    idx
}

/// The share (0..1) of triangles whose winding gives a normal on the side of the vertex normals.
pub fn winding_score(positions: &[[f32; 3]], normals: &[[f32; 3]], triangles: &[[u32; 3]]) -> f32 {
    let (mut agree, mut total) = (0usize, 0usize);
    for t in triangles {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let (Some(pa), Some(pb), Some(pc)) = (positions.get(a), positions.get(b), positions.get(c)) else { continue };
        let n = cross3(sub3(*pb, *pa), sub3(*pc, *pa));
        let avg = [0, 1, 2].map(|k| normals.get(a).map_or(0.0, |v| v[k]) + normals.get(b).map_or(0.0, |v| v[k]) + normals.get(c).map_or(0.0, |v| v[k]));
        let d = dot3(n, avg);
        if d.abs() > 1e-18 {
            total += 1;
            if d > 0.0 {
                agree += 1;
            }
        }
    }
    if total == 0 {
        0.5
    } else {
        agree as f32 / total as f32
    }
}

impl Placement {
    pub fn point(&self, p: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|i| self.sign[i] * p[self.axis[i]] * self.scale)
    }

    pub fn direction(&self, v: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|i| self.sign[i] * v[self.axis[i]])
    }

    /// Whether the turn keeps the handedness (a mirror image would flip it).
    pub fn is_proper(&self) -> bool {
        let m = [0, 1, 2].map(|i| {
            let mut row = [0f32; 3];
            row[self.axis[i]] = self.sign[i];
            row
        });
        dot3(cross3(m[0], m[1]), m[2]) > 0.0
    }

    /// Matches the axes of `shape` to those of the weapon `reference` by length, the sides by the bulk's position relative to
    /// the grip, and scales the longest side to the longest side of the reference times `length_factor`.
    pub fn fit(shape: &Sm2Shape, reference: &Ds3Reference, length_factor: f32) -> Result<(Placement, Vec<String>), String> {
        let (slo, shi) = shape.bounds().ok_or("the new shape has no vertices")?;
        let (dlo, dhi) = bounds_of(&reference.positions).ok_or("the weapon that is replaced has no vertices")?;
        let es = sub3(shi, slo);
        let ed = sub3(dhi, dlo);
        let (os, od) = (order_by_length(es), order_by_length(ed));
        let (cs, cd) = (centroid(&shape.positions), centroid(&reference.positions));
        let mut axis = [0usize; 3];
        let mut sign = [1f32; 3];
        for k in 0..2 {
            let (s, d) = (os[k], od[k]);
            axis[d] = s;
            let (bs, bd) = (cs[s], cd[d]);
            // the bulk of a weapon is on one side of the grip (a blade lies beyond the hand); when both shapes have it clearly, match it
            let clear = bs.abs() > 0.03 * es[s] && bd.abs() > 0.03 * ed[d];
            sign[d] = if clear && bs * bd < 0.0 { -1.0 } else { 1.0 };
        }
        axis[od[2]] = os[2];
        let mut p = Placement { axis, sign, scale: 1.0 };
        if !p.is_proper() {
            sign[od[2]] = -1.0;
            p = Placement { axis, sign, scale: 1.0 };
        }
        let longest_new = es[os[0]];
        let longest_old = ed[od[0]];
        if longest_new <= 1e-6 || longest_old <= 1e-6 {
            return Err("a shape without length".to_string());
        }
        p.scale = (longest_old / longest_new * length_factor).clamp(0.2, 5.0);
        let name = |a: usize| ["x", "y", "z"][a];
        let lines = vec![
            format!(
                "placement: the new shape's {} (length {:.3}) becomes the weapon's {} (length {:.3}), {} becomes {}, {} becomes {}; signs {:?}; scale {:.3}; a proper turn: {}",
                name(os[0]),
                es[os[0]],
                name(od[0]),
                ed[od[0]],
                name(os[1]),
                name(od[1]),
                name(os[2]),
                name(od[2]),
                p.sign,
                p.scale,
                p.is_proper()
            ),
            format!("the bulk of the new shape lies at ({:.3}, {:.3}, {:.3}) from the grip, that of the weapon at ({:.3}, {:.3}, {:.3})", cs[0], cs[1], cs[2], cd[0], cd[1], cd[2]),
        ];
        Ok((p, lines))
    }
}

/// The shape in the weapon's space, with the triangles turned around when the two games wind them the other way.
pub fn to_new_shape(shape: &Sm2Shape, placement: &Placement, reference: &Ds3Reference, flip_v: bool) -> (NewShape, Vec<String>) {
    let positions: Vec<[f32; 3]> = shape.positions.iter().map(|p| placement.point(*p)).collect();
    let normals: Vec<[f32; 3]> = shape.normals.iter().map(|n| placement.direction(*n)).collect();
    let tangents: Vec<[f32; 4]> = shape
        .tangents
        .iter()
        .map(|t| {
            let d = placement.direction([t[0], t[1], t[2]]);
            [d[0], d[1], d[2], t[3]]
        })
        .collect();
    let uvs: Vec<[f32; 2]> = shape.uvs.iter().map(|uv| [uv[0], if flip_v { 1.0 - uv[1] } else { uv[1] }]).collect();
    let ours = winding_score(&positions, &normals, &shape.triangles);
    let theirs = winding_score(&reference.positions, &reference.normals, &reference.triangles);
    let flip = (ours > 0.5) != (theirs > 0.5);
    let triangles: Vec<[u32; 3]> = if flip { shape.triangles.iter().map(|t| [t[0], t[2], t[1]]).collect() } else { shape.triangles.clone() };
    let lines = vec![format!(
        "winding: {:.0}% of the new triangles face the way their normals point, {:.0}% of the weapon's; triangles {}; texture coordinates {}",
        ours * 100.0,
        theirs * 100.0,
        if flip { "turned around" } else { "kept" },
        if flip_v { "flipped upside down (the picture's v runs the other way)" } else { "kept" }
    )];
    // tangents were rotated as directions; a turned-around winding does not change them
    (NewShape { positions, normals, tangents, uvs, triangles }, lines)
}

// ------------------------------------------------------------------------------------------------ the picture

const VIEW_W: usize = 420;
const VIEW_H: usize = 420;

/// Three orthographic views side by side (front, side, top): the weapon that is replaced as a flat orange silhouette and the
/// new shape shaded grey on top of it.
pub fn overlay_png(reference: &Ds3Reference, new: &NewShape) -> Result<Vec<u8>, String> {
    let (rlo, rhi) = bounds_of(&reference.positions).ok_or("nothing to draw")?;
    let (nlo, nhi) = bounds_of(&new.positions).ok_or("nothing to draw")?;
    let lo = [0, 1, 2].map(|a| rlo[a].min(nlo[a]));
    let hi = [0, 1, 2].map(|a| rhi[a].max(nhi[a]));
    let mut img = vec![[24u8, 24, 28, 255]; 3 * VIEW_W * VIEW_H];
    // (horizontal axis, vertical axis, depth axis)
    for (panel, (h, v, d)) in [(0usize, 1usize, 2usize), (2, 1, 0), (0, 2, 1)].into_iter().enumerate() {
        let (rh, rv) = ((hi[h] - lo[h]).max(1e-6), (hi[v] - lo[v]).max(1e-6));
        let scale = 0.9 * (VIEW_W as f32 / rh).min(VIEW_H as f32 / rv);
        let (ch, cv) = ((lo[h] + hi[h]) / 2.0, (lo[v] + hi[v]) / 2.0);
        let project = |p: &[f32; 3]| (VIEW_W as f32 / 2.0 + (p[h] - ch) * scale, VIEW_H as f32 / 2.0 - (p[v] - cv) * scale, p[d]);
        let mut depth = vec![f32::NEG_INFINITY; VIEW_W * VIEW_H];
        let mut draw = |positions: &[[f32; 3]], normals: Option<&[[f32; 3]]>, triangles: &[[u32; 3]], color: [f32; 3], flat: bool| {
            let light = [0.4f32, 0.7, 0.6];
            let ln = (light[0] * light[0] + light[1] * light[1] + light[2] * light[2]).sqrt();
            for t in triangles {
                let (Some(pa), Some(pb), Some(pc)) = (positions.get(t[0] as usize), positions.get(t[1] as usize), positions.get(t[2] as usize)) else { continue };
                let (a, b, c) = (project(pa), project(pb), project(pc));
                let min_x = a.0.min(b.0).min(c.0).floor().max(0.0) as usize;
                let max_x = (a.0.max(b.0).max(c.0).ceil().max(0.0) as usize).min(VIEW_W - 1);
                let min_y = a.1.min(b.1).min(c.1).floor().max(0.0) as usize;
                let max_y = (a.1.max(b.1).max(c.1).ceil().max(0.0) as usize).min(VIEW_H - 1);
                let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
                if area.abs() < 1e-9 {
                    continue;
                }
                for y in min_y..=max_y {
                    for x in min_x..=max_x {
                        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                        let w0 = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) / area;
                        let w1 = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                            continue;
                        }
                        let z = w0 * a.2 + w1 * b.2 + w2 * c.2;
                        if !flat && z <= depth[y * VIEW_W + x] {
                            continue;
                        }
                        if !flat {
                            depth[y * VIEW_W + x] = z;
                        }
                        let shade = match normals {
                            Some(ns) if !flat => {
                                let mut n = [0f32; 3];
                                for (w, i) in [(w0, t[0]), (w1, t[1]), (w2, t[2])] {
                                    if let Some(vn) = ns.get(i as usize) {
                                        for k in 0..3 {
                                            n[k] += w * vn[k];
                                        }
                                    }
                                }
                                let len = dot3(n, n).sqrt().max(1e-6);
                                (0.3 + 0.7 * (dot3(n, light) / (len * ln)).abs()).clamp(0.0, 1.0)
                            }
                            _ => 1.0,
                        };
                        let at = panel * VIEW_W + (y * 3 * VIEW_W) + x;
                        img[at] = [(color[0] * shade) as u8, (color[1] * shade) as u8, (color[2] * shade) as u8, 255];
                    }
                }
            }
        };
        draw(&reference.positions, None, &reference.triangles, [200.0, 110.0, 40.0], true);
        draw(&new.positions, Some(&new.normals), &new.triangles, [205.0, 210.0, 220.0], false);
    }
    let rgba: Vec<u8> = img.iter().flatten().copied().collect();
    PngImage { width: 3 * VIEW_W as u32, height: VIEW_H as u32, rgba }.to_png().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ashen_sm2::testing::made_up_weapon;

    /// A long thin box along `axis` from `from` to `to`, with outward normals and counter-clockwise triangles.
    fn bar(axis: usize, from: f32, to: f32) -> Ds3Reference {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut positions = Vec::new();
        for &a in &[from, to] {
            for &(x, y) in &[(-0.05f32, -0.05f32), (0.05, -0.05), (0.05, 0.05), (-0.05, 0.05)] {
                let mut p = [0f32; 3];
                p[axis] = a;
                p[u] = x;
                p[v] = y;
                positions.push(p);
            }
        }
        let centre = [0.0f32; 3];
        let normals = positions.iter().map(|p| {
            let d = sub3(*p, [if axis == 0 { p[0] } else { centre[0] }, if axis == 1 { p[1] } else { centre[1] }, if axis == 2 { p[2] } else { centre[2] }]);
            let l = dot3(d, d).sqrt().max(1e-9);
            [d[0] / l, d[1] / l, d[2] / l]
        });
        let normals: Vec<[f32; 3]> = normals.collect();
        // side faces of the box, wound counter-clockwise seen from outside when (axis, u, v) is a right-handed set
        let mut triangles = Vec::new();
        for k in 0..4u32 {
            let (a, b) = (k, (k + 1) % 4);
            triangles.push([a, b, 4 + b]);
            triangles.push([a, 4 + b, 4 + a]);
        }
        Ds3Reference { positions, normals, triangles }
    }

    fn shape_from(r: &Ds3Reference) -> Sm2Shape {
        Sm2Shape { positions: r.positions.clone(), normals: r.normals.clone(), tangents: vec![[1.0, 0.0, 0.0, 1.0]; r.positions.len()], uvs: vec![[0.5, 0.25]; r.positions.len()], triangles: r.triangles.clone(), grip_source: "test".to_string(), texture: Some("tex".to_string()) }
    }

    #[test]
    fn the_long_axis_of_the_new_shape_becomes_the_long_axis_of_the_weapon_with_the_bulk_on_the_same_side() {
        // the weapon points up the y axis from the grip; the new shape points along z, and so does its bulk
        let weapon = bar(1, -0.1, 0.9);
        let new = shape_from(&bar(2, -0.2, 1.5));
        let (p, lines) = Placement::fit(&new, &weapon, 1.0).unwrap();
        assert_eq!(p.axis[1], 2, "z of the new shape becomes y of the weapon");
        assert_eq!(p.sign[1], 1.0);
        assert!(p.is_proper());
        assert!((p.scale - 1.0 / 1.7).abs() < 1e-4, "{}", p.scale);
        let tip = p.point([0.0, 0.0, 1.5]);
        assert!(tip[1] > 0.8 && tip[0].abs() < 1e-6 && tip[2].abs() < 1e-6, "{tip:?}");
        assert!(lines[0].contains("the new shape's z (length 1.700) becomes the weapon's y (length 1.000)"), "{lines:?}");
        // the bulk on the other side of the grip turns the long axis around
        let behind = shape_from(&bar(2, -1.5, 0.2));
        let (p, _) = Placement::fit(&behind, &weapon, 1.0).unwrap();
        assert_eq!(p.sign[1], -1.0);
        assert!(p.is_proper(), "the third sign makes up for it: never a mirror image");
        // and the length factor
        let (p, _) = Placement::fit(&new, &weapon, 0.5).unwrap();
        assert!((p.scale - 0.5 / 1.7).abs() < 1e-4);
    }

    #[test]
    fn a_shape_without_length_or_a_weapon_without_vertices_is_refused() {
        let weapon = bar(1, -0.1, 0.9);
        assert!(Placement::fit(&Sm2Shape::default(), &weapon, 1.0).is_err());
        assert!(Placement::fit(&shape_from(&weapon), &Ds3Reference::default(), 1.0).is_err());
    }

    #[test]
    fn triangles_are_turned_around_only_when_the_two_games_wind_them_differently() {
        let weapon = bar(1, -0.1, 0.9);
        assert!(winding_score(&weapon.positions, &weapon.normals, &weapon.triangles) > 0.9 || winding_score(&weapon.positions, &weapon.normals, &weapon.triangles) < 0.1);
        let new = shape_from(&bar(2, -0.2, 1.5));
        let (p, _) = Placement::fit(&new, &weapon, 1.0).unwrap();
        let (shape, lines) = to_new_shape(&new, &p, &weapon, true);
        // a proper turn keeps the winding, and both bars are wound the same way: nothing is turned around
        assert!(lines[0].contains("triangles kept"), "{lines:?}");
        assert_eq!(shape.triangles, new.triangles);
        assert_eq!(shape.uvs[0], [0.5, 0.75], "v is flipped");
        // a weapon wound the other way gets the new triangles turned around
        let mut other = weapon.clone();
        for t in &mut other.triangles {
            t.swap(1, 2);
        }
        let (shape, lines) = to_new_shape(&new, &p, &other, false);
        assert!(lines[0].contains("triangles turned around"), "{lines:?}");
        assert_eq!(shape.triangles[0], [new.triangles[0][0], new.triangles[0][2], new.triangles[0][1]]);
        assert_eq!(shape.uvs[0], [0.5, 0.25]);
    }

    #[test]
    fn a_made_up_template_loads_with_its_grip_and_texture() {
        let (tpl, data) = made_up_weapon();
        let template = Template::parse(&tpl).unwrap();
        let shape = Sm2Shape::load(&template, &data).unwrap();
        assert_eq!((shape.positions.len(), shape.triangles.len()), (4, 2));
        assert_eq!(shape.texture.as_deref(), Some("square_tex"));
        // no rigid body box in the made-up file: the centre of the mesh is the grip
        assert!(shape.grip_source.contains("centre of the mesh"));
        let (lo, hi) = shape.bounds().unwrap();
        assert_eq!((lo, hi), ([-2.0, -2.0, 0.0], [2.0, 2.0, 0.0]));
        let (plain, _) = ashen_sm2::testing::made_up_model();
        let without_levels = Template::parse(&ashen_sm2::testing::made_up_model().0).unwrap();
        drop(plain);
        assert!(Sm2Shape::load(&without_levels, &data).is_err());
    }

    #[test]
    fn the_overlay_is_a_png_of_three_views() {
        let weapon = bar(1, -0.1, 0.9);
        let new = shape_from(&bar(2, -0.2, 1.5));
        let (p, _) = Placement::fit(&new, &weapon, 1.0).unwrap();
        let (shape, _) = to_new_shape(&new, &p, &weapon, false);
        let png = overlay_png(&weapon, &shape).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert!(png.len() > 500);
    }
}
