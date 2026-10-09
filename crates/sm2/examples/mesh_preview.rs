//! Developer tool: decodes sub meshes of a Space Marine 2 model and writes a Wavefront OBJ and a three-view picture of them.
//!   cargo run -p ashen-sm2 --example mesh_preview -- <file.tpl> <file.tpl_data> <out folder> [sub mesh numbers ...]
//! Without numbers the largest sub mesh is used. Reads the two files, writes only into the output folder.
use ashen_sm2::mesh::{bounds, decode_sub_mesh, to_obj, DecodedMesh};
use ashen_sm2::tpl::Template;
use std::io::BufWriter;

const W: usize = 640;
const H: usize = 360;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: mesh_preview <file.tpl> <file.tpl_data> <out folder> [sub mesh numbers ...]");
        std::process::exit(64);
    }
    let tpl = std::fs::read(&args[0]).expect("read the .tpl");
    let data = std::fs::read(&args[1]).expect("read the .tpl_data");
    let out_dir = std::path::PathBuf::from(&args[2]);
    std::fs::create_dir_all(&out_dir).expect("create the output folder");
    let template = Template::parse(&tpl).expect("parse the .tpl");
    let g = template.geometry.as_ref().expect("the template has geometry");
    let mut wanted: Vec<usize> = args[3..].iter().filter_map(|a| a.parse().ok()).collect();
    if wanted.is_empty() {
        let biggest = g.sub_meshes.iter().enumerate().max_by_key(|(_, s)| s.face_count).map(|(i, _)| i).expect("no sub meshes");
        wanted.push(biggest);
    }
    let mut merged = DecodedMesh::default();
    for index in &wanted {
        let m = decode_sub_mesh(g, &data, *index).unwrap_or_else(|e| panic!("sub mesh {index}: {e}"));
        let base = merged.positions.len() as u32;
        println!("sub mesh {index}: {} vertices, {} triangles, uv {}, tangents {}, bones {}, bounds {:?}", m.positions.len(), m.triangles.len(), m.uvs.len(), m.tangents.len(), m.bones.len(), bounds(&m));
        merged.positions.extend(&m.positions);
        merged.normals.extend(&m.normals);
        merged.uvs.extend(&m.uvs);
        merged.triangles.extend(m.triangles.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }
    let stem = std::path::Path::new(&args[0]).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "model".to_string());
    std::fs::write(out_dir.join(format!("{stem}.obj")), to_obj(&merged, &stem)).expect("write the obj");
    let image = render_three_views(&merged);
    let file = std::fs::File::create(out_dir.join(format!("{stem}-views.png"))).expect("create the png");
    let mut enc = png::Encoder::new(BufWriter::new(file), (3 * W) as u32, H as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().expect("png header").write_image_data(&image).expect("png data");
    println!("wrote {stem}.obj and {stem}-views.png in {}", out_dir.display());
}

/// Three orthographic views side by side: side (length across, height up), top (length across, width up), front.
fn render_three_views(m: &DecodedMesh) -> Vec<u8> {
    let mut img = vec![24u8; 3 * W * H * 3];
    let Some((lo, hi)) = bounds(m) else { return img };
    // (horizontal axis, vertical axis, depth axis, flip depth)
    for (panel, (h, v, d)) in [(2usize, 1usize, 0usize), (2, 0, 1), (0, 1, 2)].into_iter().enumerate() {
        let (rh, rv) = (hi[h] - lo[h], hi[v] - lo[v]);
        let scale = 0.9 * (W as f32 / rh.max(1e-6)).min(H as f32 / rv.max(1e-6));
        let (ch, cv) = ((lo[h] + hi[h]) / 2.0, (lo[v] + hi[v]) / 2.0);
        let mut depth = vec![f32::NEG_INFINITY; W * H];
        let project = |p: &[f32; 3]| -> (f32, f32, f32) { (W as f32 / 2.0 + (p[h] - ch) * scale, H as f32 / 2.0 - (p[v] - cv) * scale, p[d]) };
        let light = {
            let l = [0.4f32, 0.7, 0.6];
            let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
            [l[0] / n, l[1] / n, l[2] / n]
        };
        for t in &m.triangles {
            let (a, b, c) = (project(&m.positions[t[0] as usize]), project(&m.positions[t[1] as usize]), project(&m.positions[t[2] as usize]));
            let min_x = a.0.min(b.0).min(c.0).floor().max(0.0) as usize;
            let max_x = (a.0.max(b.0).max(c.0).ceil() as usize).min(W - 1);
            let min_y = a.1.min(b.1).min(c.1).floor().max(0.0) as usize;
            let max_y = (a.1.max(b.1).max(c.1).ceil() as usize).min(H - 1);
            let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            if area.abs() < 1e-6 {
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
                    if z <= depth[y * W + x] {
                        continue;
                    }
                    depth[y * W + x] = z;
                    // lit by the interpolated vertex normal
                    let mut n = [0f32; 3];
                    for (w, i) in [(w0, t[0]), (w1, t[1]), (w2, t[2])] {
                        let vn = m.normals[i as usize];
                        for k in 0..3 {
                            n[k] += w * vn[k];
                        }
                    }
                    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
                    let lit = ((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / len).abs();
                    let shade = (0.25 + 0.75 * lit).clamp(0.0, 1.0);
                    let at = (y * 3 * W + panel * W + x) * 3;
                    img[at] = (200.0 * shade) as u8;
                    img[at + 1] = (205.0 * shade) as u8;
                    img[at + 2] = (215.0 * shade) as u8;
                }
            }
        }
    }
    img
}
