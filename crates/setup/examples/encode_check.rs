//! Developer tool: how well do the texture encoders hold a real picture?
//!   cargo run --release -p ashen-setup --example encode_check -- <folder with the exported pct files> <texture name> [out.png]
//! Decodes the Space Marine 2 picture, encodes it as BC1 and BC3 like the model swap does, decodes that and reports the error.
use ashen_ds3data::dds::{decode_bc1_block, decode_bc4_block, encode_dds, Format, Image, Spec};
use ashen_sm2::texture::{decode, TexDesc};

fn load(folder: &str, name: &str) -> Image {
    let text = std::fs::read_to_string(format!("{folder}/{name}.pct.resource")).expect("descriptor");
    let desc = TexDesc::parse(&text).expect("parse descriptor");
    let top = std::fs::read(format!("{folder}/{}", desc.mip_maps[0])).expect("top mip");
    let img = decode(desc.format, desc.mip_dims(0).0, desc.mip_dims(0).1, &top).expect("decode");
    Image::new(img.width as usize, img.height as usize, img.rgba).expect("image")
}

fn psnr(a: &[u8], b: &[u8], channels: usize) -> f64 {
    let mut se = 0f64;
    let mut n = 0f64;
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for c in 0..channels {
            se += (f64::from(pa[c]) - f64::from(pb[c])).powi(2);
            n += 1.0;
        }
    }
    10.0 * (255.0f64 * 255.0 / (se / n).max(1e-9)).log10()
}

fn decode_back(dds: &[u8], w: usize, h: usize, bc3: bool) -> Vec<u8> {
    let header = 128;
    let mut out = vec![255u8; w * h * 4];
    let block = if bc3 { 16 } else { 8 };
    let bw = w.div_ceil(4);
    for (i, b) in dds[header..header + bw * h.div_ceil(4) * block].chunks_exact(block).enumerate() {
        let (bx, by) = (i % bw, i / bw);
        let colors = decode_bc1_block(&b[block - 8..]);
        let alpha = if bc3 { decode_bc4_block(&b[..8]) } else { [255u8; 16] };
        for k in 0..16 {
            let (x, y) = (bx * 4 + k % 4, by * 4 + k / 4);
            if x < w && y < h {
                let at = (y * w + x) * 4;
                out[at..at + 3].copy_from_slice(&colors[k]);
                out[at + 3] = alpha[k];
            }
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let img = load(&args[0], &args[1]);
    println!("{}x{} picture, average colour {:?}", img.width, img.height, ashen_ds3data::dds::mean_color(&encode_dds(Spec { format: Format::Bc3, dxgi: None }, &img, 1).unwrap()));
    for (name, format, bc3) in [("BC1", Format::Bc1, false), ("BC3", Format::Bc3, true)] {
        let t = std::time::Instant::now();
        let dds = encode_dds(Spec { format, dxgi: None }, &img, 11).unwrap();
        let took = t.elapsed();
        let back = decode_back(&dds, img.width, img.height, bc3);
        println!("{name}: {} bytes with 11 levels, {:.1} s, PSNR of the top level {:.1} dB (colour), file {} bytes", dds.len(), took.as_secs_f32(), psnr(&img.rgba, &back, 3), dds.len());
        if let (true, Some(out)) = (bc3, args.get(2)) {
            let png = ashen_sm2::texture::Image { width: img.width as u32, height: img.height as u32, rgba: back }.to_png().unwrap();
            std::fs::write(out, png).unwrap();
        }
    }
}
