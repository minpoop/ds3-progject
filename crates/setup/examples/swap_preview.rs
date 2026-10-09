//! Developer tool: runs the whole model swap on files that are given as arguments and writes what comes out.
//!   cargo run -p ashen-setup --example swap_preview -- <sm2 .tpl> <sm2 .tpl_data> <ds3 .partsbnd.dcx or decoded BND4> <out folder> [length factor]
//! Writes `<out>/overlay.png` (the weapon that is replaced in orange, the new shape on top), `<out>/new.partsbnd.dcx` (or
//! `new.bnd4`) and prints the report. Reads only the files it is given.
use ashen_ds3data::dcx;
use ashen_ds3data::flver::Flver;
use ashen_ds3data::bnd4::Bnd4;
use ashen_ds3data::weaponswap::swap_container;
use ashen_setup::weaponmodel::{ds3_reference, overlay_png, to_new_shape, Placement, Sm2Shape};
use ashen_sm2::tpl::Template;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: swap_preview <sm2 .tpl> <sm2 .tpl_data> <ds3 container> <out folder> [length factor]");
        std::process::exit(64);
    }
    let tpl = std::fs::read(&args[0]).expect("read the .tpl");
    let data = std::fs::read(&args[1]).expect("read the .tpl_data");
    let ds3 = std::fs::read(&args[2]).expect("read the DS3 container");
    let out = std::path::PathBuf::from(&args[3]);
    let factor: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    std::fs::create_dir_all(&out).expect("create the output folder");

    let (bnd_bytes, dcx_info) = match dcx::decode(&ds3) {
        Ok((inner, info)) => (inner, Some(info)),
        Err(_) => (ds3.clone(), None),
    };
    let bnd = Bnd4::parse(&bnd_bytes).expect("the DS3 file is a BND4");
    let flver_at = bnd.files.iter().find(|f| f.name.as_deref().is_some_and(|n| n.to_lowercase().ends_with(".flver"))).expect("a .flver inside").index;
    let model = Flver::parse(bnd.file_bytes(&bnd_bytes, flver_at).expect("model bytes")).expect("parse the model");
    for l in model.describe().iter().take(12) {
        println!("ds3: {l}");
    }
    let reference = ds3_reference(&model).expect("read the weapon that is replaced");
    let template = Template::parse(&tpl).expect("parse the .tpl");
    let shape = Sm2Shape::load(&template, &data).expect("load the new shape");
    println!("sm2: {} vertices, {} triangles, grip from {}, texture {:?}", shape.positions.len(), shape.triangles.len(), shape.grip_source, shape.texture);
    let (placement, lines) = Placement::fit(&shape, &reference, factor).expect("fit");
    for l in &lines {
        println!("{l}");
    }
    let (new_shape, lines) = to_new_shape(&shape, &placement, &reference, true);
    for l in &lines {
        println!("{l}");
    }
    std::fs::write(out.join("overlay.png"), overlay_png(&reference, &new_shape).expect("draw")).expect("write the picture");
    match swap_container(&bnd_bytes, new_shape, None) {
        Err(e) => println!("SWAP REFUSED: {e}"),
        Ok(result) => {
            for l in &result.lines {
                println!("swap: {l}");
            }
            match dcx_info {
                Some(info) => std::fs::write(out.join("new.partsbnd.dcx"), dcx::encode(&result.container, &info).expect("encode DCX")).expect("write"),
                None => std::fs::write(out.join("new.bnd4"), &result.container).expect("write"),
            }
            println!("wrote the new container ({} bytes)", result.container.len());
        }
    }
}
