//! Developer tool: draws the full-detail mesh of a Space Marine 2 template with its picture on it, once with the texture
//! coordinates as decoded and once with v flipped, from the side, so that the right way up can be seen.
//!   cargo run -p ashen-sm2 --example textured_preview -- <file.tpl> <file.tpl_data> <folder with the exported pct files> <texture name> <out.png> [axis h v d]
use ashen_sm2::mesh::decode_sub_mesh;
use ashen_sm2::texture::{decode, Image, TexDesc};
use ashen_sm2::tpl::Template;

const W: usize = 900;
const H: usize = 420;

fn load(folder: &str, name: &str) -> (usize, usize, Vec<u8>) {
    let text = std::fs::read_to_string(format!("{folder}/{name}.pct.resource")).expect("descriptor");
    let desc = TexDesc::parse(&text).expect("parse descriptor");
    let top = std::fs::read(format!("{folder}/{}", desc.mip_maps[0])).expect("top mip");
    let img = decode(desc.format, desc.mip_dims(0).0, desc.mip_dims(0).1, &top).expect("decode");
    (img.width as usize, img.height as usize, img.rgba)
}

fn sample(tex: &(usize, usize, Vec<u8>), u: f32, v: f32) -> [f32; 3] {
    let (w, h, px) = (tex.0, tex.1, &tex.2);
    let x = (u.rem_euclid(1.0) * w as f32).min(w as f32 - 1.0) as usize;
    let y = (v.rem_euclid(1.0) * h as f32).min(h as f32 - 1.0) as usize;
    let p = &px[(y * w + x) * 4..];
    [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tpl = std::fs::read(&args[0]).unwrap();
    let data = std::fs::read(&args[1]).unwrap();
    let t = Template::parse(&tpl).unwrap();
    let g = t.geometry.as_ref().unwrap();
    let tex = load(&args[2], &args[3]);
    let (ha, va, da): (usize, usize, usize) = if args.len() >= 9 { (args[6].parse().unwrap(), args[7].parse().unwrap(), args[8].parse().unwrap()) } else { (2, 1, 0) };
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for i in t.full_detail_sub_meshes() {
        let m = decode_sub_mesh(g, &data, i).unwrap();
        let base = positions.len() as u32;
        positions.extend(&m.positions);
        normals.extend(&m.normals);
        uvs.extend(&m.uvs);
        tris.extend(m.triangles.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &positions {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    let mut out = vec![[24u8, 24, 28]; W * 2 * H];
    for (panel, flip) in [false, true].into_iter().enumerate() {
        let (rh, rv) = (hi[ha] - lo[ha], hi[va] - lo[va]);
        let scale = 0.92 * (W as f32 / rh).min(H as f32 / rv);
        let (ch, cv) = ((lo[ha] + hi[ha]) / 2.0, (lo[va] + hi[va]) / 2.0);
        let project = |p: &[f32; 3]| (W as f32 / 2.0 + (p[ha] - ch) * scale, H as f32 / 2.0 - (p[va] - cv) * scale, p[da]);
        let mut depth = vec![f32::NEG_INFINITY; W * H];
        let light = [0.3f32, 0.8, 0.5];
        for tri in &tris {
            let idx = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
            let pr: Vec<(f32, f32, f32)> = idx.iter().map(|&i| project(&positions[i])).collect();
            let area = (pr[1].0 - pr[0].0) * (pr[2].1 - pr[0].1) - (pr[1].1 - pr[0].1) * (pr[2].0 - pr[0].0);
            if area.abs() < 1e-9 {
                continue;
            }
            let (minx, maxx) = (pr.iter().map(|q| q.0).fold(f32::MAX, f32::min).floor().max(0.0) as usize, (pr.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(W - 1));
            let (miny, maxy) = (pr.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor().max(0.0) as usize, (pr.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(H - 1));
            for y in miny..=maxy {
                for x in minx..=maxx {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((pr[1].0 - px) * (pr[2].1 - py) - (pr[1].1 - py) * (pr[2].0 - px)) / area;
                    let w1 = ((pr[2].0 - px) * (pr[0].1 - py) - (pr[2].1 - py) * (pr[0].0 - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = w0 * pr[0].2 + w1 * pr[1].2 + w2 * pr[2].2;
                    if z <= depth[y * W + x] {
                        continue;
                    }
                    depth[y * W + x] = z;
                    let (mut u, mut v, mut n) = (0f32, 0f32, [0f32; 3]);
                    for (w, i) in [(w0, idx[0]), (w1, idx[1]), (w2, idx[2])] {
                        u += w * uvs[i][0];
                        v += w * uvs[i][1];
                        for k in 0..3 {
                            n[k] += w * normals[i][k];
                        }
                    }
                    let v = if flip { 1.0 - v } else { v };
                    let c = sample(&tex, u, v);
                    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
                    let lit = (0.35 + 0.65 * ((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / len).abs()).clamp(0.0, 1.0);
                    out[y * 2 * W + panel * W + x] = [(c[0] * lit * 1.8).min(255.0) as u8, (c[1] * lit * 1.8).min(255.0) as u8, (c[2] * lit * 1.8).min(255.0) as u8];
                }
            }
        }
    }
    let rgba: Vec<u8> = out.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    std::fs::write(&args[4], Image { width: 2 * W as u32, height: H as u32, rgba }.to_png().unwrap()).unwrap();
    println!("wrote {} (left: v as decoded, right: v flipped)", args[4]);
}
