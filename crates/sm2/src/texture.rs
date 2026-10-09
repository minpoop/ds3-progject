//! Space Marine 2 textures: a small text descriptor (`*.pct.resource`, YAML) plus raw block-compressed pixel data
//! (`*.pct_mip`, i.e. a DDS with its header stripped). Format ids and the one-file-per-mip layout come from the
//! MIT-licensed vash2pid/texmipper; the decoding is bcdec_rs (MIT), a port of bcdec (MIT/Unlicense).
use anyhow::{anyhow, bail, Result};
use serde_yaml_ng::Value;

/// `SM2TextureFormat` ids (only the ones that matter here; the full list has 65 entries).
pub mod fmt {
    pub const ARGB8888: u32 = 0;
    pub const A8: u32 = 3;
    pub const OXT1: u32 = 12;
    pub const AXT1: u32 = 13;
    pub const XT2: u32 = 14;
    pub const XT3: u32 = 15;
    pub const XT4: u32 = 16;
    pub const XT5: u32 = 17;
    pub const XRGB8888: u32 = 22;
    pub const DXN: u32 = 36;
    pub const DXT5A: u32 = 37;
    pub const R8U: u32 = 46;
    pub const BC6U: u32 = 49;
    pub const BC6S: u32 = 50;
    pub const BC7: u32 = 51;
    pub const BC7A: u32 = 52;
}

pub fn format_name(f: u32) -> String {
    match f {
        0 => "ARGB8888",
        2 => "RGB888",
        3 => "A8",
        12 => "OXT1(BC1)",
        13 => "AXT1(BC1)",
        14 => "XT2(BC2)",
        15 => "XT3(BC2)",
        16 => "XT4(BC3)",
        17 => "XT5(BC3)",
        22 => "XRGB8888",
        36 => "DXN(BC5)",
        37 => "DXT5A(BC4)",
        38 => "RGBA16161616F",
        40 => "ARGB16161616U",
        46 => "R8U",
        49 => "BC6U",
        50 => "BC6S",
        51 => "BC7",
        52 => "BC7A",
        54 => "R16G16",
        55 => "R16",
        other => return format!("format#{other}"),
    }
    .to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MipLevel {
    pub offset: u64,
    pub size: u64,
}

/// The fields of a `.pct.resource` descriptor that this project needs. Unknown fields are ignored and missing ones
/// are zero or empty: the real files decide, this is deliberately tolerant.
#[derive(Debug, Clone, Default)]
pub struct TexDesc {
    pub format: u32,
    pub sx: u32,
    pub sy: u32,
    pub sz: u32,
    pub n_faces: u32,
    pub n_mip_map: u32,
    pub face_size: u64,
    pub size: u64,
    pub mip_levels: Vec<MipLevel>,
    pub mip_maps: Vec<String>,
    pub tex_name: String,
    pub tex_type: String,
    pub link_td: String,
    pub pct: String,
}

fn num(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64().or_else(|| n.as_i64().map(|i| i.max(0) as u64)).or_else(|| n.as_f64().map(|f| f as u64)),
        Value::String(s) => {
            let t = s.trim();
            if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                u64::from_str_radix(h, 16).ok()
            } else {
                t.parse().ok()
            }
        }
        Value::Bool(b) => Some(*b as u64),
        _ => None,
    }
}

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

impl TexDesc {
    pub fn parse(yaml: &str) -> Result<TexDesc> {
        let root: Value = serde_yaml_ng::from_str(yaml.trim_start_matches('\u{feff}')).map_err(|e| anyhow!("descriptor is not valid YAML: {e}"))?;
        let Value::Mapping(root) = root else { bail!("descriptor is not a mapping") };
        let get = |m: &serde_yaml_ng::Mapping, k: &str| m.get(Value::String(k.to_string())).cloned();
        let header = match get(&root, "header") {
            Some(Value::Mapping(h)) => h,
            _ => bail!("descriptor has no `header`"),
        };
        let hn = |k: &str| get(&header, k).as_ref().and_then(num).unwrap_or(0);
        let mut mip_levels = Vec::new();
        if let Some(Value::Sequence(seq)) = get(&header, "mipLevel") {
            for item in seq {
                if let Value::Mapping(m) = item {
                    mip_levels.push(MipLevel {
                        offset: get(&m, "offset").as_ref().and_then(num).unwrap_or(0),
                        size: get(&m, "size").as_ref().and_then(num).unwrap_or(0),
                    });
                }
            }
        }
        let mut mip_maps = Vec::new();
        if let Some(Value::Sequence(seq)) = get(&root, "mipMaps") {
            for item in seq {
                if let Value::String(s) = item {
                    mip_maps.push(s);
                }
            }
        }
        Ok(TexDesc {
            format: hn("format") as u32,
            sx: hn("sx") as u32,
            sy: hn("sy") as u32,
            sz: hn("sz") as u32,
            n_faces: hn("nFaces") as u32,
            n_mip_map: hn("nMipMap") as u32,
            face_size: hn("faceSize"),
            size: hn("size"),
            mip_levels,
            mip_maps,
            tex_name: text(get(&root, "texName").as_ref()),
            tex_type: text(get(&root, "texType").as_ref()),
            link_td: text(get(&root, "linkTd").as_ref()),
            pct: text(get(&root, "pct").as_ref()),
        })
    }

    /// Pixel size of mip `level` (0 is the largest).
    pub fn mip_dims(&self, level: u32) -> (u32, u32) {
        ((self.sx >> level).max(1), (self.sy >> level).max(1))
    }
}

/// (pixels per block edge, bytes per block) - for plain pixel formats the "block" is one pixel.
pub fn block_info(format: u32) -> Option<(u32, u32)> {
    Some(match format {
        fmt::OXT1 | fmt::AXT1 | fmt::DXT5A => (4, 8),
        fmt::XT2 | fmt::XT3 | fmt::XT4 | fmt::XT5 | fmt::DXN | fmt::BC6U | fmt::BC6S | fmt::BC7 | fmt::BC7A => (4, 16),
        fmt::ARGB8888 | fmt::XRGB8888 => (1, 4),
        fmt::R8U | fmt::A8 => (1, 1),
        _ => return None,
    })
}

/// Bytes one mip of `w` x `h` occupies in a `.pct_mip` file.
pub fn mip_byte_size(format: u32, w: u32, h: u32) -> Option<u64> {
    let (edge, bytes) = block_info(format)?;
    Some(w.div_ceil(edge) as u64 * h.div_ceil(edge) as u64 * bytes as u64)
}

/// A decoded picture: RGBA, 8 bits per channel, row-major, no padding.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn to_png(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header()?;
            w.write_image_data(&self.rgba)?;
        }
        Ok(out)
    }

    /// Box-filter shrink so that neither side is larger than `max_side` (never enlarges).
    pub fn shrink_to(&self, max_side: u32) -> Image {
        let big = self.width.max(self.height);
        if big <= max_side || max_side == 0 {
            return self.clone();
        }
        let f = big.div_ceil(max_side);
        let (nw, nh) = ((self.width / f).max(1), (self.height / f).max(1));
        let mut out = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0u32; 4];
                let mut n = 0u32;
                for dy in 0..f {
                    for dx in 0..f {
                        let (sx, sy) = (x * f + dx, y * f + dy);
                        if sx < self.width && sy < self.height {
                            let i = ((sy * self.width + sx) * 4) as usize;
                            for c in 0..4 {
                                acc[c] += self.rgba[i + c] as u32;
                            }
                            n += 1;
                        }
                    }
                }
                let o = ((y * nw + x) * 4) as usize;
                for c in 0..4 {
                    out[o + c] = (acc[c] / n.max(1)) as u8;
                }
            }
        }
        Image { width: nw, height: nh, rgba: out }
    }

    /// Mean colour and alpha, a cheap way to notice an all-black or all-transparent decode.
    pub fn mean_rgba(&self) -> [u8; 4] {
        let n = (self.rgba.len() / 4).max(1) as u64;
        let mut acc = [0u64; 4];
        for px in self.rgba.chunks_exact(4) {
            for c in 0..4 {
                acc[c] += px[c] as u64;
            }
        }
        [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, (acc[3] / n) as u8]
    }
}

/// Decode one mip level of `format` (raw bytes from a `.pct_mip`) into RGBA.
pub fn decode(format: u32, width: u32, height: u32, data: &[u8]) -> Result<Image> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        bail!("implausible texture size {width}x{height}");
    }
    let need = mip_byte_size(format, width, height).ok_or_else(|| anyhow!("unsupported texture format {}", format_name(format)))?;
    if (data.len() as u64) < need {
        bail!("{} {width}x{height} needs {need} bytes but the mip has {}", format_name(format), data.len());
    }
    let (w, h) = (width as usize, height as usize);
    let rgba = match format {
        fmt::ARGB8888 => data[..w * h * 4].chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect(),
        fmt::XRGB8888 => data[..w * h * 4].chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect(),
        fmt::R8U => data[..w * h].iter().flat_map(|&v| [v, v, v, 255]).collect(),
        fmt::A8 => data[..w * h].iter().flat_map(|&v| [255, 255, 255, v]).collect(),
        _ => decode_blocks(format, w, h, data)?,
    };
    Ok(Image { width, height, rgba })
}

fn decode_blocks(format: u32, w: usize, h: usize, data: &[u8]) -> Result<Vec<u8>> {
    let (pw, ph) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
    let (bx_count, by_count) = (pw / 4, ph / 4);
    let (_, block_bytes) = block_info(format).ok_or_else(|| anyhow!("unsupported texture format {}", format_name(format)))?;
    let block_bytes = block_bytes as usize;
    let mut padded = vec![255u8; pw * ph * 4];
    for by in 0..by_count {
        for bx in 0..bx_count {
            let src = &data[(by * bx_count + bx) * block_bytes..][..block_bytes];
            let dst = &mut padded[(by * 4 * pw + bx * 4) * 4..];
            let pitch = pw * 4;
            match format {
                fmt::OXT1 | fmt::AXT1 => bcdec_rs::bc1(src, dst, pitch),
                fmt::XT2 | fmt::XT3 => bcdec_rs::bc2(src, dst, pitch),
                fmt::XT4 | fmt::XT5 => bcdec_rs::bc3(src, dst, pitch),
                fmt::BC7 | fmt::BC7A => bcdec_rs::bc7(src, dst, pitch),
                fmt::DXT5A => {
                    let mut px = [0u8; 16];
                    bcdec_rs::bc4(src, &mut px, 4, false);
                    for y in 0..4 {
                        for x in 0..4 {
                            let v = px[y * 4 + x];
                            let o = y * pitch + x * 4;
                            dst[o..o + 4].copy_from_slice(&[v, v, v, 255]);
                        }
                    }
                }
                fmt::DXN => {
                    // two-channel tangent-space normal map: rebuild blue so the preview looks like a normal map
                    let mut px = [0u8; 32];
                    bcdec_rs::bc5(src, &mut px, 8, false);
                    for y in 0..4 {
                        for x in 0..4 {
                            let (r, g) = (px[y * 8 + x * 2], px[y * 8 + x * 2 + 1]);
                            let (nx, ny) = (r as f32 / 127.5 - 1.0, g as f32 / 127.5 - 1.0);
                            let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
                            let o = y * pitch + x * 4;
                            dst[o..o + 4].copy_from_slice(&[r, g, ((nz * 0.5 + 0.5) * 255.0) as u8, 255]);
                        }
                    }
                }
                fmt::BC6U | fmt::BC6S => {
                    let mut px = [0f32; 48];
                    bcdec_rs::bc6h_float(src, &mut px, 12, format == fmt::BC6S);
                    for y in 0..4 {
                        for x in 0..4 {
                            let o = y * pitch + x * 4;
                            for c in 0..3 {
                                // plain Reinhard + gamma: this is only a preview of an HDR texture
                                let v = px[y * 12 + x * 3 + c].max(0.0);
                                let t = (v / (1.0 + v)).powf(1.0 / 2.2);
                                dst[o + c] = (t * 255.0 + 0.5) as u8;
                            }
                            dst[o + 3] = 255;
                        }
                    }
                }
                _ => bail!("unsupported texture format {}", format_name(format)),
            }
        }
    }
    if pw == w && ph == h {
        return Ok(padded);
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        out.extend_from_slice(&padded[y * pw * 4..y * pw * 4 + w * 4]);
    }
    Ok(out)
}

/// A DDS file for one mip (so the picture opens in ordinary tools). BC1-BC5 use the classic FourCC header,
/// BC7 and BC6H use the DX10 extension.
pub fn dds_bytes(format: u32, w: u32, h: u32, data: &[u8]) -> Result<Vec<u8>> {
    let need = mip_byte_size(format, w, h).ok_or_else(|| anyhow!("unsupported texture format {}", format_name(format)))? as usize;
    if data.len() < need {
        bail!("mip data is smaller than the format needs");
    }
    // (fourcc, dxgi format when the DX10 extension header is needed)
    let (fourcc, dxgi): (&[u8; 4], u32) = match format {
        fmt::OXT1 | fmt::AXT1 => (b"DXT1", 0),
        fmt::XT2 | fmt::XT3 => (b"DXT3", 0),
        fmt::XT4 | fmt::XT5 => (b"DXT5", 0),
        fmt::DXT5A => (b"ATI1", 0),
        fmt::DXN => (b"ATI2", 0),
        fmt::BC7 | fmt::BC7A => (b"DX10", 98),
        fmt::BC6U => (b"DX10", 95),
        fmt::BC6S => (b"DX10", 96),
        fmt::ARGB8888 | fmt::XRGB8888 | fmt::R8U | fmt::A8 => bail!("plain pixel formats are written as PNG only"),
        _ => bail!("unsupported texture format {}", format_name(format)),
    };
    fn put(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let mut out = Vec::with_capacity(148 + need);
    out.extend_from_slice(b"DDS ");
    put(&mut out, 124); // header size
    put(&mut out, 0x1 | 0x2 | 0x4 | 0x1000 | 0x80000); // CAPS | HEIGHT | WIDTH | PIXELFORMAT | LINEARSIZE
    put(&mut out, h);
    put(&mut out, w);
    put(&mut out, need as u32);
    put(&mut out, 0); // depth
    put(&mut out, 1); // mip count
    for _ in 0..11 {
        put(&mut out, 0);
    }
    put(&mut out, 32); // pixel format size
    put(&mut out, 0x4); // FOURCC
    out.extend_from_slice(fourcc);
    for _ in 0..5 {
        put(&mut out, 0);
    }
    put(&mut out, 0x1000); // caps: TEXTURE
    for _ in 0..4 {
        put(&mut out, 0);
    }
    if dxgi != 0 {
        put(&mut out, dxgi);
        put(&mut out, 3); // 2D
        put(&mut out, 0);
        put(&mut out, 1);
        put(&mut out, 0);
    }
    out.extend_from_slice(&data[..need]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESC: &str = r#"
header:
  faceSize: 8
  format: 12
  mipLevel:
  - offset: 0
    size: 8
  - offset: 8
    size: 8
  nFaces: 1
  nMipMap: 2
  sign: 123456789
  size: 16
  sx: 4
  sy: 4
  sz: 1
linkTd: pct/foo.td
mipMaps:
- foo_d_0.pct_mip
- foo_d_1.pct_mip
texName: foo_d
texType: color
pct: foo
unknownField: [1, 2, 3]
"#;

    #[test]
    fn parses_a_descriptor_and_ignores_unknown_fields() {
        let d = TexDesc::parse(DESC).unwrap();
        assert_eq!((d.format, d.sx, d.sy, d.n_mip_map), (12, 4, 4, 2));
        assert_eq!(d.mip_levels, vec![MipLevel { offset: 0, size: 8 }, MipLevel { offset: 8, size: 8 }]);
        assert_eq!(d.mip_maps, ["foo_d_0.pct_mip", "foo_d_1.pct_mip"]);
        assert_eq!(d.tex_name, "foo_d");
        assert_eq!(d.mip_dims(0), (4, 4));
        assert_eq!(d.mip_dims(5), (1, 1));
        assert!(TexDesc::parse("not: a descriptor").is_err());
        assert!(TexDesc::parse("- a\n- b").is_err());
    }

    #[test]
    fn mip_sizes_match_the_block_layout() {
        assert_eq!(mip_byte_size(fmt::OXT1, 4, 4), Some(8));
        assert_eq!(mip_byte_size(fmt::XT5, 8, 8), Some(64));
        assert_eq!(mip_byte_size(fmt::BC7, 1, 1), Some(16));
        assert_eq!(mip_byte_size(fmt::DXN, 4096, 4096), Some(16 << 20));
        assert_eq!(mip_byte_size(fmt::ARGB8888, 3, 2), Some(24));
        assert_eq!(mip_byte_size(99, 4, 4), None);
    }

    #[test]
    fn decodes_bc1_blocks_to_the_right_pixels() {
        // block 1: red (565 0xF800) vs blue (0x001F), all indices 0 -> red. block 2: all indices 1 -> blue.
        let red = [0x00, 0xF8, 0x1F, 0x00, 0, 0, 0, 0];
        let blue = [0x00, 0xF8, 0x1F, 0x00, 0x55, 0x55, 0x55, 0x55];
        let mut data = Vec::new();
        data.extend_from_slice(&red);
        data.extend_from_slice(&blue);
        let img = decode(fmt::OXT1, 8, 4, &data).unwrap();
        assert_eq!((img.width, img.height), (8, 4));
        assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);
        let (row, col) = (0usize, 4usize);
        let right = (row * 8 + col) * 4;
        assert_eq!(&img.rgba[right..right + 4], &[0, 0, 255, 255]);
        let last = ((3 * 8 + 7) * 4) as usize;
        assert_eq!(&img.rgba[last..last + 4], &[0, 0, 255, 255]);
    }

    #[test]
    fn crops_sizes_that_are_not_multiples_of_four() {
        let img = decode(fmt::OXT1, 5, 3, &[0u8; 16]).unwrap();
        assert_eq!((img.width, img.height), (5, 3));
        assert_eq!(img.rgba.len(), 5 * 3 * 4);
    }

    #[test]
    fn bc3_bc4_bc5_bc7_and_plain_formats_decode_without_panicking() {
        for f in [fmt::XT2, fmt::XT5, fmt::DXN, fmt::DXT5A, fmt::BC7, fmt::BC6U] {
            let n = mip_byte_size(f, 8, 8).unwrap() as usize;
            let img = decode(f, 8, 8, &vec![0x5Au8; n]).unwrap();
            assert_eq!(img.rgba.len(), 8 * 8 * 4, "{}", format_name(f));
        }
        let bgra = [10u8, 20, 30, 40];
        assert_eq!(decode(fmt::ARGB8888, 1, 1, &bgra).unwrap().rgba, [30, 20, 10, 40]);
        assert_eq!(decode(fmt::XRGB8888, 1, 1, &bgra).unwrap().rgba, [30, 20, 10, 255]);
        assert_eq!(decode(fmt::R8U, 1, 1, &[9]).unwrap().rgba, [9, 9, 9, 255]);
    }

    #[test]
    fn bc4_endpoints_decode_to_full_range() {
        // endpoints 255 and 0, all indices 0 -> every pixel is endpoint 0 = 255
        let img = decode(fmt::DXT5A, 4, 4, &[255, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(img.rgba.chunks_exact(4).all(|p| p == [255, 255, 255, 255]));
    }

    #[test]
    fn errors_are_clear() {
        assert!(decode(fmt::OXT1, 8, 8, &[0u8; 8]).unwrap_err().to_string().contains("needs"));
        assert!(decode(99, 4, 4, &[0u8; 64]).unwrap_err().to_string().contains("unsupported"));
        assert!(decode(fmt::OXT1, 0, 4, &[]).is_err());
    }

    #[test]
    fn png_and_dds_output_are_well_formed() {
        let img = decode(fmt::OXT1, 4, 4, &[0x00, 0xF8, 0x1F, 0x00, 0, 0, 0, 0]).unwrap();
        let png = img.to_png().unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let dds = dds_bytes(fmt::OXT1, 4, 4, &[0u8; 8]).unwrap();
        assert_eq!(&dds[..4], b"DDS ");
        assert_eq!(dds.len(), 128 + 8);
        assert_eq!(&dds[84..88], b"DXT1");
        let dds7 = dds_bytes(fmt::BC7, 4, 4, &[0u8; 16]).unwrap();
        assert_eq!(dds7.len(), 148 + 16);
        assert_eq!(u32::from_le_bytes(dds7[128..132].try_into().unwrap()), 98);
        assert!(dds_bytes(fmt::ARGB8888, 1, 1, &[0; 4]).is_err());
    }

    #[test]
    fn shrinking_keeps_the_aspect_and_never_enlarges() {
        let img = Image { width: 100, height: 50, rgba: vec![200; 100 * 50 * 4] };
        let small = img.shrink_to(25);
        assert_eq!((small.width, small.height), (25, 12));
        assert!(small.rgba.iter().all(|&v| v == 200));
        assert_eq!(img.shrink_to(1000).width, 100);
    }
}

/// A texture found in the paks: its descriptor, which mip level the bytes are, and the raw bytes of that mip.
#[derive(Debug, Clone)]
pub struct LoadedTexture {
    pub desc: TexDesc,
    pub level: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    /// the `.pct_mip` the bytes came from
    pub file: String,
}

impl LoadedTexture {
    pub fn decode(&self) -> Result<Image> {
        decode(self.desc.format, self.width, self.height, &self.data)
    }
}

/// Find the data file a descriptor names: as written, then below `pct/`, then by file name anywhere in the paks.
pub fn find_mip_file(paks: &crate::pak::PakSet, name: &str) -> Option<String> {
    let norm = name.replace('\\', "/");
    for cand in [norm.clone(), format!("pct/{norm}")] {
        if paks.contains(&cand) {
            return Some(cand);
        }
    }
    let tail = format!("/{}", norm.rsplit('/').next().unwrap_or(&norm).to_ascii_lowercase());
    paks.find(|k| k.ends_with(&tail)).into_iter().next()
}

/// Read the largest mip that is present for the texture described by `resource` (an entry name such as
/// `pct/foo_d_0.pct.resource`). The data may be one file per mip or one file holding every mip: both are tried and
/// the byte size decides.
pub fn load_top(paks: &mut crate::pak::PakSet, resource: &str) -> Result<LoadedTexture> {
    let text = paks.read_text(resource, 1 << 20)?;
    let desc = TexDesc::parse(&text).map_err(|e| anyhow!("{resource}: {e}"))?;
    if desc.mip_maps.is_empty() {
        bail!("{resource}: descriptor lists no mip files");
    }
    let levels = desc.n_mip_map.max(desc.mip_levels.len() as u32).max(desc.mip_maps.len() as u32).max(1);
    // keep the position of every listed file (file i is mip i), even when this install does not ship some of them
    let mut files: Vec<Option<(String, Vec<u8>)>> = Vec::new();
    for m in &desc.mip_maps {
        files.push(match find_mip_file(paks, m) {
            Some(f) => {
                let bytes = paks.read(&f, 1 << 30)?;
                Some((f, bytes))
            }
            None => None,
        });
    }
    if files.iter().all(|f| f.is_none()) {
        bail!("{resource}: none of its mip files ({}) is in any pak", desc.mip_maps.join(", "));
    }
    for level in 0..levels {
        let (w, h) = desc.mip_dims(level);
        let Some(want) = mip_byte_size(desc.format, w, h) else {
            bail!("{resource}: unsupported texture format {}", format_name(desc.format));
        };
        // 1. one file per mip
        if let Some(Some((name, bytes))) = files.get(level as usize) {
            if bytes.len() as u64 == want {
                return Ok(LoadedTexture { desc, level, width: w, height: h, data: bytes.clone(), file: name.clone() });
            }
        }
        // 2. one file with every mip, located by the descriptor's offsets
        if let Some(ml) = desc.mip_levels.get(level as usize) {
            if ml.size == want {
                for (name, bytes) in files.iter().flatten() {
                    let (o, n) = (ml.offset as usize, ml.size as usize);
                    if o.checked_add(n).is_some_and(|e| e <= bytes.len()) && bytes.len() as u64 > want {
                        return Ok(LoadedTexture { desc: desc.clone(), level, width: w, height: h, data: bytes[o..o + n].to_vec(), file: name.clone() });
                    }
                }
            }
        }
    }
    let sizes: Vec<String> = files.iter().flatten().map(|(n, b)| format!("{n}={}", b.len())).collect();
    bail!(
        "{resource}: no mip file matches the size {}x{} {} needs ({} bytes); files: {}",
        desc.sx,
        desc.sy,
        format_name(desc.format),
        mip_byte_size(desc.format, desc.sx, desc.sy).unwrap_or(0),
        sizes.join(", ")
    )
}

#[cfg(test)]
mod load_tests {
    use super::*;
    use crate::pak::PakSet;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn pak(path: &std::path::Path, entries: &[(&str, Vec<u8>)]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (n, d) in entries {
            z.start_file(*n, SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
            z.write_all(d).unwrap();
        }
        z.finish().unwrap();
    }

    fn desc(files: &[&str], levels: &[(u64, u64)], sx: u32) -> String {
        let mut s = format!("header:\n  format: 12\n  nMipMap: {}\n  sx: {sx}\n  sy: {sx}\n  mipLevel:\n", levels.len());
        for (o, n) in levels {
            s += &format!("  - offset: {o}\n    size: {n}\n");
        }
        s += "mipMaps:\n";
        for f in files {
            s += &format!("- {f}\n");
        }
        s
    }

    #[test]
    fn loads_one_file_per_mip() {
        let dir = tempfile::tempdir().unwrap();
        let red = vec![0x00u8, 0xF8, 0x1F, 0x00, 0, 0, 0, 0];
        pak(&dir.path().join("resources.pak"), &[("pct/t_d_0.pct.resource", desc(&["t_d_0.pct_mip", "t_d_1.pct_mip"], &[(0, 8), (8, 8)], 8).into_bytes())]);
        // 8x8 BC1 is 32 bytes (top mip), 4x4 is 8 bytes
        let top: Vec<u8> = red.iter().cycle().take(32).copied().collect();
        pak(&dir.path().join("default.pak"), &[("pct/t_d_0.pct_mip", top), ("pct/t_d_1.pct_mip", red.clone())]);
        let mut set = PakSet::open_dir(dir.path()).unwrap();
        let t = load_top(&mut set, "pct/t_d_0.pct.resource").unwrap();
        assert_eq!((t.level, t.width, t.height, t.data.len()), (0, 8, 8, 32));
        let img = t.decode().unwrap();
        assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn loads_one_file_holding_every_mip_by_offset() {
        let dir = tempfile::tempdir().unwrap();
        let red = [0x00u8, 0xF8, 0x1F, 0x00, 0, 0, 0, 0];
        let blue = [0x00u8, 0xF8, 0x1F, 0x00, 0x55, 0x55, 0x55, 0x55];
        let mut all: Vec<u8> = Vec::new();
        for _ in 0..4 {
            all.extend(blue); // 8x8 = four blocks of blue
        }
        all.extend(red); // then the 4x4 mip
        pak(&dir.path().join("resources.pak"), &[("pct/t.pct.resource", desc(&["t.pct_mip"], &[(0, 32), (32, 8)], 8).into_bytes())]);
        pak(&dir.path().join("default.pak"), &[("pct/t.pct_mip", all)]);
        let mut set = PakSet::open_dir(dir.path()).unwrap();
        let t = load_top(&mut set, "pct/t.pct.resource").unwrap();
        assert_eq!((t.level, t.width), (0, 8));
        assert_eq!(&t.decode().unwrap().rgba[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn skips_mips_that_are_not_shipped_and_finds_files_outside_pct() {
        let dir = tempfile::tempdir().unwrap();
        // the descriptor says 8x8 but the only file is the 4x4 mip (the big ones are not in this install)
        let red = vec![0x00u8, 0xF8, 0x1F, 0x00, 0, 0, 0, 0];
        pak(&dir.path().join("resources.pak"), &[("pct/t.pct.resource", desc(&["t_1.pct_mip", "t_2.pct_mip"], &[(0, 32), (0, 8)], 8).into_bytes())]);
        pak(&dir.path().join("default.pak"), &[("somewhere/else/t_2.pct_mip", red.clone())]);
        let mut set = PakSet::open_dir(dir.path()).unwrap();
        let t = load_top(&mut set, "pct/t.pct.resource").unwrap();
        assert_eq!((t.level, t.width, t.height), (1, 4, 4));
        assert_eq!(t.file, "somewhere/else/t_2.pct_mip");
        assert_eq!(&t.decode().unwrap().rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn explains_when_nothing_matches() {
        let dir = tempfile::tempdir().unwrap();
        pak(&dir.path().join("resources.pak"), &[("pct/t.pct.resource", desc(&["t.pct_mip"], &[(0, 32)], 8).into_bytes())]);
        pak(&dir.path().join("default.pak"), &[("pct/t.pct_mip", vec![0u8; 5])]);
        let mut set = PakSet::open_dir(dir.path()).unwrap();
        let err = load_top(&mut set, "pct/t.pct.resource").unwrap_err().to_string();
        assert!(err.contains("no mip file matches") && err.contains("t.pct_mip=5"), "{err}");
        assert!(load_top(&mut set, "pct/none.pct.resource").is_err());
    }
}
