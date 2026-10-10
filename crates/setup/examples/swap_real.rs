//! Developer tool: runs the whole weapon swap of `ds3-models` on files that were copied out of the two games (the way the
//! optional model-file zip of the test kit has them) and writes what comes out.
//!   cargo run --release -p ashen-setup --example swap_real -- <sm2 .tpl> <sm2 .tpl_data> <sm2 picture .pct.resource or "-"> <ds3 .partsbnd.dcx> <out folder> [weapon id of the weapons sheet]
//! Writes `<out>/new.partsbnd.dcx` and `<out>/overlay.png` and prints the report lines. The picture's mip files are looked
//! for next to the descriptor. With a weapon id (`chainsword`, `bolt_pistol`) the sheet's `model_*` placement hints for it are
//! used, as `ds3-models` does. Reads only the files it is given.
use ashen_common::weapons::sheet_weapons;
use ashen_ds3data::bnd4::Bnd4;
use ashen_ds3data::dcx;
use ashen_ds3data::dds::Image;
use ashen_ds3data::flver::Flver;
use ashen_setup::weaponmodel::{make_weapon_container, Sm2Shape};
use ashen_sm2::texture::{decode, TexDesc};
use ashen_sm2::tpl::Template;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 5 {
        eprintln!("usage: swap_real <sm2 .tpl> <sm2 .tpl_data> <sm2 picture .pct.resource | -> <ds3 container> <out folder>");
        std::process::exit(64);
    }
    let template = Template::parse(&std::fs::read(&args[0]).expect("read the .tpl")).expect("parse the .tpl");
    let shape = Sm2Shape::load(&template, &std::fs::read(&args[1]).expect("read the .tpl_data")).expect("load the shape");
    println!("sm2: {} vertices, {} triangles, grip from {}, texture {:?}", shape.positions.len(), shape.triangles.len(), shape.grip_source, shape.texture);
    let picture = if args[2] == "-" {
        None
    } else {
        let text = std::fs::read_to_string(&args[2]).expect("read the picture descriptor");
        let desc = TexDesc::parse(&text).expect("parse the descriptor");
        // the mip files sit next to the descriptor (a zip unpacked on Linux may have a backslash as part of every file name,
        // so the prefix is simply everything up to and including the last separator of either kind)
        let cut = args[2].rfind(['/', '\\']).map_or(0, |i| i + 1);
        let base = &args[2][..cut];
        let top = std::fs::read(format!("{base}{}", desc.mip_maps[0])).expect("read the top mip");
        let (w, h) = desc.mip_dims(0);
        let img = decode(desc.format, w, h, &top).expect("decode the picture");
        println!("picture: {}x{}", img.width, img.height);
        Some(Image::new(img.width as usize, img.height as usize, img.rgba).expect("image"))
    };
    let raw = std::fs::read(&args[3]).expect("read the DS3 container");
    let (decoded, info) = dcx::decode(&raw).expect("decode the DCX");
    let out = std::path::PathBuf::from(&args[4]);
    std::fs::create_dir_all(&out).expect("create the folder");
    let hints = args.get(5).and_then(|id| sheet_weapons().into_iter().find(|w| &w.id == id)).map(|w| w.model).unwrap_or_default();
    let made = match make_weapon_container(&decoded, &shape, picture.as_ref(), 1.0, true, &hints, &mut |l| println!("  {l}")) {
        Ok(m) => m,
        Err(why) => {
            println!("NOT DONE: {why}");
            std::process::exit(2);
        }
    };
    if let Some(png) = &made.picture {
        std::fs::write(out.join("overlay.png"), png).expect("write the picture");
    }
    let packed = dcx::encode(&made.container, &info).expect("pack the DCX");
    std::fs::write(out.join("new.partsbnd.dcx"), &packed).expect("write the container");
    println!("wrote {} bytes (the game's was {})", packed.len(), raw.len());
    // read it back the way the game would: DCX, BND4, every model
    let (back, _) = dcx::decode(&packed).expect("the new DCX decodes");
    let bnd = Bnd4::parse(&back).expect("the new BND4 parses");
    for f in &bnd.files {
        let bytes = bnd.file_bytes(&back, f.index).unwrap();
        let name = f.name.clone().unwrap_or_default();
        if name.to_lowercase().ends_with(".flver") {
            let m = Flver::parse(bytes).expect("the new model parses");
            println!("  {name}: {} bytes, {} mesh(es), {} vertices, header faces {}/{}, written again identical: {}", bytes.len(), m.meshes.len(), m.meshes.iter().map(|x| x.vertex_buffers[0].vertex_count).sum::<i32>(), m.header.face_count, m.header.total_face_count, m.write().map(|w| w == bytes).unwrap_or(false));
        } else {
            println!("  {name}: {} bytes", bytes.len());
        }
    }
}
