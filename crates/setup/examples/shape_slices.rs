//! Developer tool: where is the handle of a weapon? Prints, for a Dark Souls III weapon container and for a Space Marine 2
//! template, the extent of the model in slices along each axis, so the hold point can be compared by eye.
//!   cargo run --release -p ashen-setup --example shape_slices -- <ds3 container> <sm2 .tpl> <sm2 .tpl_data>
use ashen_ds3data::bnd4::Bnd4;
use ashen_ds3data::dcx;
use ashen_ds3data::flver::Flver;
use ashen_ds3data::weaponswap::host_model;
use ashen_setup::weaponmodel::Sm2Shape;
use ashen_sm2::tpl::Template;

fn slices(name: &str, points: &[[f32; 3]]) {
    println!("== {name}: {} points", points.len());
    for axis in 0..3 {
        let lo = points.iter().map(|p| p[axis]).fold(f32::MAX, f32::min);
        let hi = points.iter().map(|p| p[axis]).fold(f32::MIN, f32::max);
        println!("  along axis {axis} ({lo:.3} .. {hi:.3}); per slice: count, the other two axes' ranges, mean of them");
        let n = 16;
        for s in 0..n {
            let a = lo + (hi - lo) * s as f32 / n as f32;
            let b = lo + (hi - lo) * (s + 1) as f32 / n as f32;
            let inside: Vec<&[f32; 3]> = points.iter().filter(|p| p[axis] >= a && (p[axis] < b || (s == n - 1 && p[axis] <= b))).collect();
            if inside.is_empty() {
                println!("    {a:+.3}..{b:+.3}: -");
                continue;
            }
            let (o1, o2) = ((axis + 1) % 3, (axis + 2) % 3);
            let range = |o: usize| (inside.iter().map(|p| p[o]).fold(f32::MAX, f32::min), inside.iter().map(|p| p[o]).fold(f32::MIN, f32::max));
            let mean = |o: usize| inside.iter().map(|p| p[o]).sum::<f32>() / inside.len() as f32;
            let (r1, r2) = (range(o1), range(o2));
            println!("    {a:+.3}..{b:+.3}: {:>5}  axis{o1} {:+.3}..{:+.3} (mean {:+.3})  axis{o2} {:+.3}..{:+.3} (mean {:+.3})", inside.len(), r1.0, r1.1, mean(o1), r2.0, r2.1, mean(o2));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: shape_slices <ds3 container> <sm2 .tpl> <sm2 .tpl_data>");
        std::process::exit(64);
    }
    let raw = std::fs::read(&args[0]).expect("read the DS3 container");
    let (inner, _) = dcx::decode(&raw).expect("decode");
    let bnd = Bnd4::parse(&inner).expect("bnd4");
    let (at, _) = host_model(&bnd).expect("the weapon model");
    let model = Flver::parse(bnd.file_bytes(&inner, at).unwrap()).expect("flver");
    let mesh = ashen_ds3data::modelswap::biggest_mesh(&model).unwrap();
    slices("DS3 weapon", &model.positions(mesh).expect("positions"));
    let template = Template::parse(&std::fs::read(&args[1]).expect("tpl")).expect("parse");
    let shape = Sm2Shape::load(&template, &std::fs::read(&args[2]).expect("tpl_data")).expect("shape");
    slices("SM2 weapon (relative to the grip as the program finds it)", &shape.positions);
}
