//! Developer tool: one line per sub mesh of a Space Marine 2 template - which object it belongs to, its detail level, its
//! material and texture, counts, and its bounds before and after the object's own transform.
//!   cargo run -p ashen-sm2 --example weapon_summary -- <file.tpl> <file.tpl_data>
use ashen_sm2::mesh::{bounds, decode_sub_mesh};
use ashen_sm2::tpl::Template;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: weapon_summary <file.tpl> <file.tpl_data>");
        std::process::exit(64);
    }
    let tpl = std::fs::read(&args[0]).expect("read the .tpl");
    let data = std::fs::read(&args[1]).expect("read the .tpl_data");
    let t = Template::parse(&tpl).expect("parse the .tpl");
    let g = t.geometry.as_ref().expect("the template has geometry");
    for (i, lr) in g.lod_roots.iter().enumerate() {
        println!("lod root {i}: objects {:?} max lod indices {:?} max distances {:?}", lr.object_ids, lr.max_lod_indices, lr.max_distances);
    }
    for (i, s) in g.sub_meshes.iter().enumerate() {
        let node = g.objects.iter().find(|o| i32::from(o.id) == i32::from(s.node));
        let name = node.and_then(|o| o.name.as_deref()).unwrap_or("-");
        let tex = s.material.iter().find(|(k, _)| k == "shadingMtl_Tex").map(|(_, v)| v.show()).unwrap_or_default();
        let mtl = s.material.iter().find(|(k, _)| k == "shadingMtl_Mtl").map(|(_, v)| v.show()).unwrap_or_default();
        let m = decode_sub_mesh(g, &data, i);
        let shown = match &m {
            Ok(m) => format!("{} verts {} tris bounds {:?}", m.positions.len(), m.triangles.len(), bounds(m).map(|(lo, hi)| (lo.map(|v| (v * 1000.0).round() / 1000.0), hi.map(|v| (v * 1000.0).round() / 1000.0)))),
            Err(e) => format!("ERROR {e}"),
        };
        let model = node.and_then(|o| o.model).map(|m| [m[12], m[13], m[14]]);
        println!("sub {i:3} node {:3} {:<28} tex {tex:<34} mtl {mtl:<22} {shown} node-model-translation {model:?}", s.node, name);
    }
}
