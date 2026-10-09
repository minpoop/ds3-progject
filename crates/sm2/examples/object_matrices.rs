//! Developer tool: prints the transform matrices of some objects of a template: object_matrices <file.tpl> <id> [<id> ...]
use ashen_sm2::tpl::Template;

fn show(m: &[f32; 16]) -> String {
    (0..4).map(|r| format!("[{:8.4} {:8.4} {:8.4} {:8.4}]", m[r * 4], m[r * 4 + 1], m[r * 4 + 2], m[r * 4 + 3])).collect::<Vec<_>>().join(" ")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tpl = std::fs::read(&args[0]).expect("read the .tpl");
    let t = Template::parse(&tpl).expect("parse the .tpl");
    let g = t.geometry.as_ref().expect("geometry");
    for id in args[1..].iter().filter_map(|a| a.parse::<i16>().ok()) {
        let Some(o) = g.objects.iter().find(|o| o.id == id) else { continue };
        println!("object {id} {:?} parent {}", o.name, o.parent);
        println!("  local {}", o.local.as_ref().map_or("-".to_string(), show));
        println!("  model {}", o.model.as_ref().map_or("-".to_string(), show));
        println!("  bbox {:?}", o.bbox);
    }
}
