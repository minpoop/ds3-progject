//! Developer tool: looks inside a Dark Souls III weapon container (DCX holding a BND4) that is given as an argument and says,
//! for every model and texture file in it, whether writing it again gives the very same bytes - and where it does not.
//!   cargo run -p ashen-setup --example ds3_inspect -- <container .partsbnd.dcx or decoded BND4> [--dump <folder>] [--describe]
//! Reads only the file it is given; `--dump` also writes the files inside it (the decoded container's members) to the folder.
use ashen_ds3data::bnd4::Bnd4;
use ashen_ds3data::dcx;
use ashen_ds3data::flver::Flver;
use ashen_ds3data::tpf::Tpf;

/// Runs of differing bytes between two byte strings of the same length: (offset, length), at most `limit` of them.
fn differing_runs(a: &[u8], b: &[u8], limit: usize) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < a.len().min(b.len()) && runs.len() < limit {
        if a[i] == b[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < a.len().min(b.len()) && a[i] != b[i] {
            i += 1;
        }
        runs.push((start, i - start));
    }
    runs
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: ds3_inspect <container> [--dump <folder>]");
        std::process::exit(64);
    };
    let dump = args.iter().position(|a| a == "--dump").and_then(|i| args.get(i + 1)).cloned();
    let raw = std::fs::read(path).expect("read the container");
    let (inner, info) = match dcx::decode(&raw) {
        Ok((inner, info)) => (inner, Some(info)),
        Err(_) => (raw.clone(), None),
    };
    println!("{path}: {} bytes, {} after the DCX layer ({})", raw.len(), inner.len(), info.as_ref().map_or("no DCX".to_string(), |i| i.variant_name().to_string()));
    let bnd = Bnd4::parse(&inner).expect("a BND4");
    println!("BND4 {} files, layout verified {}", bnd.files.len(), bnd.layout.verified);
    for f in &bnd.files {
        let name = f.name.clone().unwrap_or_default();
        let bytes = bnd.file_bytes(&inner, f.index).expect("file bytes");
        println!("#{} id {:?} {} ({} bytes)", f.index, f.id, name, bytes.len());
        if let Some(dir) = &dump {
            std::fs::create_dir_all(dir).expect("create the folder");
            let leaf = name.rsplit(['\\', '/']).next().unwrap_or("file");
            std::fs::write(format!("{dir}/{leaf}"), bytes).expect("write the member");
        }
        let lower = name.to_lowercase();
        if lower.ends_with(".flver") {
            match Flver::parse(bytes) {
                Err(e) => println!("   FLVER does not parse: {e}"),
                Ok(model) => {
                    println!("   header: face count {} total {}", model.header.face_count, model.header.total_face_count);
                    if args.iter().any(|a| a == "--describe") {
                        for line in model.describe() {
                            println!("      {line}");
                        }
                    }
                    for (mi, mesh) in model.meshes.iter().enumerate() {
                        let vertices = mesh.vertex_buffers.first().map_or(0, |b| b.vertex_count);
                        for (fi, fs) in mesh.face_sets.iter().enumerate() {
                            let (mut tris, mut degenerate) = (0usize, 0usize);
                            if fs.triangle_strip {
                                for w in fs.indices.windows(3) {
                                    if vertices < 65535 && (w[0] == 0xFFFF || w[1] == 0xFFFF || w[2] == 0xFFFF) {
                                        continue;
                                    }
                                    tris += 1;
                                    if w[0] == w[1] || w[1] == w[2] || w[0] == w[2] {
                                        degenerate += 1;
                                    }
                                }
                            } else {
                                for c in fs.indices.chunks_exact(3) {
                                    tris += 1;
                                    if c[0] == c[1] || c[1] == c[2] || c[0] == c[2] {
                                        degenerate += 1;
                                    }
                                }
                            }
                            println!("      mesh {mi} face set {fi}: flags {:#x} {} indices {} -> {tris} triangles, {degenerate} degenerate; {}-bit", fs.flags, if fs.triangle_strip { "strip" } else { "list" }, fs.indices.len(), fs.index_bits);
                        }
                    }
                    match model.write() {
                    Err(e) => println!("   FLVER cannot be written again: {e}"),
                    Ok(again) if again == bytes => println!("   FLVER written again: byte-identical"),
                    Ok(again) => {
                        println!("   FLVER written again DIFFERS: {} bytes instead of {}", again.len(), bytes.len());
                        for (at, len) in differing_runs(&again, bytes, 24) {
                            let show = |s: &[u8]| s[at..(at + len.min(16)).min(s.len())].iter().map(|b| format!("{b:02x}")).collect::<String>();
                            println!("      at {at:#x} ({len} bytes): ours {} / the game's {}", show(&again), show(bytes));
                        }
                    }
                    }
                }
            }
        } else if lower.ends_with(".tpf") {
            match Tpf::parse(bytes) {
                Err(e) => println!("   TPF does not parse: {e}"),
                Ok(tpf) => {
                    println!("   TPF {} textures, written again: {}", tpf.textures.len(), if tpf.write() == bytes { "byte-identical" } else { "DIFFERS" });
                    if args.iter().any(|a| a == "--describe") {
                        for line in tpf.describe() {
                            println!("      {line}");
                        }
                    }
                }
            }
        }
    }
}
