//! Pictures for a texture container: a plain RGBA image, resizing, mip chains, the block-compressed formats the weapons'
//! textures use (BC1, BC3 and the one- and two-channel BC4, BC5) and `.dds` files made from them. All of it is plain Rust
//! and is tested by decoding what it writes.
//!
//! Two kinds of texture are made:
//!
//! * a real picture ([`encode_dds`]): BC1 or BC3, fitted per 4x4 block along the principal axis of the colours, refined
//!   with a least-squares step;
//! * a plain colour ([`constant_dds`]): every block is the same, in any of the formats (BC1, BC3, BC4, BC5 and BC7 as a
//!   constant block), which is how the maps a new look has no picture for (normals, specular) are made - in the format and
//!   size of the texture they replace.
use crate::tpf::DdsInfo;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdsError {
    BadImage(String),
    Unsupported(String),
}

impl fmt::Display for DdsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DdsError::BadImage(s) => write!(f, "bad image: {s}"),
            DdsError::Unsupported(s) => write!(f, "not supported: {s}"),
        }
    }
}

impl std::error::Error for DdsError {}

type Result<T> = std::result::Result<T, DdsError>;

/// The biggest picture side handled (a weapon texture is 1024 or 2048).
pub const MAX_SIDE: usize = 8192;

/// An RGBA picture, 8 bits per channel, top row first.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: usize, height: usize, rgba: Vec<u8>) -> Result<Image> {
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(DdsError::BadImage(format!("{width}x{height} is not a usable size")));
        }
        if rgba.len() != width * height * 4 {
            return Err(DdsError::BadImage(format!("{} bytes for {width}x{height} pixels", rgba.len())));
        }
        Ok(Image { width, height, rgba })
    }

    pub fn filled(width: usize, height: usize, color: [u8; 4]) -> Result<Image> {
        Image::new(width, height, color.iter().copied().cycle().take(width * height * 4).collect())
    }

    fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let at = (y * self.width + x) * 4;
        [self.rgba[at], self.rgba[at + 1], self.rgba[at + 2], self.rgba[at + 3]]
    }

    /// The picture at another size: every new pixel is the average of the old area it covers (a box filter, which for
    /// halving is the usual mip step); enlarging repeats pixels.
    pub fn resized(&self, width: usize, height: usize) -> Result<Image> {
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(DdsError::BadImage(format!("{width}x{height} is not a usable size")));
        }
        if (width, height) == (self.width, self.height) {
            return Ok(self.clone());
        }
        // horizontal pass, then vertical
        let across = average_axis(self.width, width);
        let down = average_axis(self.height, height);
        let mut mid = vec![0f32; width * self.height * 4];
        for y in 0..self.height {
            for (x, taps) in across.iter().enumerate() {
                for (sx, w) in taps {
                    let p = self.pixel(*sx, y);
                    for c in 0..4 {
                        mid[(y * width + x) * 4 + c] += f32::from(p[c]) * w;
                    }
                }
            }
        }
        let mut out = vec![0u8; width * height * 4];
        for (y, taps) in down.iter().enumerate() {
            for x in 0..width {
                for c in 0..4 {
                    let mut v = 0.0;
                    for (sy, w) in taps {
                        v += mid[(sy * width + x) * 4 + c] * w;
                    }
                    out[(y * width + x) * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        Image::new(width, height, out)
    }

    /// This picture and every smaller one down to one pixel (`levels` of them at most).
    pub fn mip_chain(&self, levels: usize) -> Result<Vec<Image>> {
        let mut chain = vec![self.clone()];
        while chain.len() < levels {
            let last = chain.last().expect("the chain is never empty");
            if last.width == 1 && last.height == 1 {
                break;
            }
            let next = last.resized((last.width / 2).max(1), (last.height / 2).max(1))?;
            chain.push(next);
        }
        Ok(chain)
    }

    /// The same picture with every alpha value set.
    pub fn with_alpha(mut self, alpha: u8) -> Image {
        for px in self.rgba.chunks_exact_mut(4) {
            px[3] = alpha;
        }
        self
    }
}

/// For each new position along an axis the old positions it averages with their weights (summing to one).
fn average_axis(old: usize, new: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = old as f32 / new as f32;
    (0..new)
        .map(|i| {
            let (from, to) = (i as f32 * scale, (i as f32 + 1.0) * scale);
            let first = from.floor() as usize;
            let last = ((to.ceil() as usize).max(first + 1)).min(old);
            let mut taps = Vec::new();
            let mut total = 0.0;
            for s in first..last {
                let w = (to.min(s as f32 + 1.0) - from.max(s as f32)).max(0.0);
                if w > 0.0 {
                    taps.push((s, w));
                    total += w;
                }
            }
            if taps.is_empty() {
                taps.push((first.min(old - 1), 1.0));
                total = 1.0;
            }
            for t in &mut taps {
                t.1 /= total;
            }
            taps
        })
        .collect()
}

// ------------------------------------------------------------------------------------------------ the formats

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bc1,
    Bc3,
    Bc4,
    Bc5,
    Bc7,
}

impl Format {
    /// Bytes of one 4x4 block.
    pub fn block_bytes(self) -> usize {
        match self {
            Format::Bc1 | Format::Bc4 => 8,
            _ => 16,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Bc1 => "BC1 (DXT1)",
            Format::Bc3 => "BC3 (DXT5)",
            Format::Bc4 => "BC4",
            Format::Bc5 => "BC5",
            Format::Bc7 => "BC7",
        }
    }

    fn legacy_four_cc(self) -> Option<&'static [u8; 4]> {
        Some(match self {
            Format::Bc1 => b"DXT1",
            Format::Bc3 => b"DXT5",
            Format::Bc4 => b"ATI1",
            Format::Bc5 => b"ATI2",
            Format::Bc7 => return None,
        })
    }

    /// Bytes of one level of `width` x `height` pixels.
    pub fn level_bytes(self, width: usize, height: usize) -> usize {
        width.div_ceil(4).max(1) * height.div_ceil(4).max(1) * self.block_bytes()
    }
}

/// How a `.dds` file is written: the block format and, when the file has the extended header, the DXGI number it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub format: Format,
    pub dxgi: Option<u32>,
}

impl Spec {
    /// The format of an existing file, so a replacement can be made the same way. `None` for a format not handled.
    pub fn from_info(info: &DdsInfo) -> Option<Spec> {
        let by_name = match info.four_cc.as_str() {
            "DXT1" => Some(Format::Bc1),
            "DXT4" | "DXT5" => Some(Format::Bc3),
            "ATI1" | "BC4U" => Some(Format::Bc4),
            "ATI2" | "BC5U" => Some(Format::Bc5),
            _ => None,
        };
        if let Some(format) = by_name {
            return Some(Spec { format, dxgi: None });
        }
        let dxgi = info.dxgi_format?;
        let format = match dxgi {
            70..=72 => Format::Bc1,
            76..=78 => Format::Bc3,
            79..=81 => Format::Bc4,
            82..=84 => Format::Bc5,
            97..=99 => Format::Bc7,
            _ => return None,
        };
        Some(Spec { format, dxgi: Some(dxgi) })
    }

    /// The same picture format in the plain old header (the one every tool reads) - for formats that have one.
    pub fn legacy(self) -> Option<Spec> {
        self.format.legacy_four_cc().map(|_| Spec { format: self.format, dxgi: None })
    }
}

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// The bytes of a `.dds` file: header and the levels (largest first). `levels` hold the block data of each level.
pub fn dds_file(spec: Spec, width: usize, height: usize, levels: &[Vec<u8>]) -> Result<Vec<u8>> {
    if levels.is_empty() {
        return Err(DdsError::BadImage("a texture without levels".to_string()));
    }
    for (i, l) in levels.iter().enumerate() {
        let (w, h) = ((width >> i).max(1), (height >> i).max(1));
        if l.len() != spec.format.level_bytes(w, h) {
            return Err(DdsError::BadImage(format!("level {i} has {} bytes, {} are needed for {w}x{h}", l.len(), spec.format.level_bytes(w, h))));
        }
    }
    let mut out = Vec::new();
    out.extend(b"DDS ");
    out.extend(le32(124));
    // caps, height, width, pixel format, mip map count, linear size
    out.extend(le32(0x1 | 0x2 | 0x4 | 0x1000 | 0x20000 | 0x80000));
    out.extend(le32(height as u32));
    out.extend(le32(width as u32));
    out.extend(le32(levels[0].len() as u32));
    out.extend(le32(0)); // depth
    out.extend(le32(levels.len() as u32));
    out.extend([0u8; 44]); // reserved
    out.extend(le32(32)); // pixel format size
    out.extend(le32(4)); // FOURCC
    match spec.dxgi {
        Some(_) => out.extend(b"DX10"),
        None => out.extend(spec.format.legacy_four_cc().ok_or_else(|| DdsError::Unsupported(format!("{} needs the extended header", spec.format.name())))?),
    }
    out.extend([0u8; 20]); // bit count and masks
    out.extend(le32(0x1000 | if levels.len() > 1 { 0x8 | 0x400000 } else { 0 })); // caps
    out.extend([0u8; 16]); // caps2..reserved
    if let Some(dxgi) = spec.dxgi {
        out.extend(le32(dxgi));
        out.extend(le32(3)); // 2D texture
        out.extend(le32(0));
        out.extend(le32(1));
        out.extend(le32(0));
    }
    for l in levels {
        out.extend(l);
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------ BC1 and BC3

/// The 5-bit or 6-bit number whose expansion to 8 bits is nearest to `v`.
fn nearest_level(v: f32, bits: u32) -> u16 {
    let max = (1u32 << bits) - 1;
    let guess = (v / 255.0 * max as f32).round().clamp(0.0, max as f32) as i64;
    let expand = |q: i64| -> f32 {
        let q = q as u32;
        (if bits == 5 { (q << 3) | (q >> 2) } else { (q << 2) | (q >> 4) }) as f32
    };
    (guess - 1..=guess + 1).filter(|q| *q >= 0 && *q <= i64::from(max)).min_by(|a, b| (expand(*a) - v).abs().total_cmp(&(expand(*b) - v).abs())).unwrap_or(guess) as u16
}

fn to565(c: [f32; 3]) -> u16 {
    (nearest_level(c[0], 5) << 11) | (nearest_level(c[1], 6) << 5) | nearest_level(c[2], 5)
}

fn from565(c: u16) -> [f32; 3] {
    let (r, g, b) = (u32::from(c >> 11) & 31, u32::from(c >> 5) & 63, u32::from(c) & 31);
    [((r << 3) | (r >> 2)) as f32, ((g << 2) | (g >> 4)) as f32, ((b << 3) | (b >> 2)) as f32]
}

fn palette(c0: u16, c1: u16) -> [[f32; 3]; 4] {
    let (a, b) = (from565(c0), from565(c1));
    // the decoders use integer arithmetic, (2a+b)/3 rounded down
    let third = |wa: u32, wb: u32| -> [f32; 3] { std::array::from_fn(|i| ((wa * a[i] as u32 + wb * b[i] as u32) / 3) as f32) };
    [a, b, third(2, 1), third(1, 2)]
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// The indices (0..3 into the palette) of the 16 pixels and the total squared error.
fn assign(pal: &[[f32; 3]; 4], px: &[[f32; 3]; 16]) -> ([u8; 16], f32) {
    let mut idx = [0u8; 16];
    let mut err = 0.0;
    for (i, p) in px.iter().enumerate() {
        let (mut best, mut best_d) = (0u8, f32::MAX);
        for (k, c) in pal.iter().enumerate() {
            let d = dist2(*c, *p);
            if d < best_d {
                best = k as u8;
                best_d = d;
            }
        }
        idx[i] = best;
        err += best_d;
    }
    (idx, err)
}

/// The block bytes for endpoints and indices: in 4-colour mode the first endpoint must be the bigger number, so the
/// endpoints are swapped (with the indices) when needed; two equal endpoints give a flat block.
fn pack_bc1(c0: u16, c1: u16, idx: &[u8; 16]) -> [u8; 8] {
    if c0 == c1 {
        // equal endpoints mean "three colours and clear": only the first colour may be used
        return pack_bc1_constant(c0);
    }
    let (c0, c1, map): (u16, u16, [u8; 4]) = if c0 > c1 { (c0, c1, [0, 1, 2, 3]) } else { (c1, c0, [1, 0, 3, 2]) };
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    let mut bits = 0u32;
    for (i, k) in idx.iter().enumerate() {
        bits |= u32::from(map[*k as usize]) << (2 * i);
    }
    out[4..8].copy_from_slice(&bits.to_le_bytes());
    out
}

/// The 8 bytes of a BC1 block for 16 pixels (alpha is not looked at).
pub fn bc1_block(px: &[[u8; 4]; 16]) -> [u8; 8] {
    let pts: [[f32; 3]; 16] = std::array::from_fn(|i| [f32::from(px[i][0]), f32::from(px[i][1]), f32::from(px[i][2])]);
    let first = pts[0];
    if pts.iter().all(|p| *p == first) {
        // a flat block: the nearest representable colour, all indices 0
        let c = to565(first);
        return pack_bc1_constant(c);
    }
    let mean = {
        let mut m = [0f32; 3];
        for p in &pts {
            for a in 0..3 {
                m[a] += p[a] / 16.0;
            }
        }
        m
    };
    // covariance and the principal axis by power iteration
    let mut cov = [[0f32; 3]; 3];
    for p in &pts {
        let d = [p[0] - mean[0], p[1] - mean[1], p[2] - mean[2]];
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j];
            }
        }
    }
    let mut axis = [0.57735f32, 0.57735, 0.57735];
    for _ in 0..8 {
        let n = [
            cov[0][0] * axis[0] + cov[0][1] * axis[1] + cov[0][2] * axis[2],
            cov[1][0] * axis[0] + cov[1][1] * axis[1] + cov[1][2] * axis[2],
            cov[2][0] * axis[0] + cov[2][1] * axis[1] + cov[2][2] * axis[2],
        ];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l < 1e-9 {
            break;
        }
        axis = [n[0] / l, n[1] / l, n[2] / l];
    }
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for p in &pts {
        let t = (p[0] - mean[0]) * axis[0] + (p[1] - mean[1]) * axis[1] + (p[2] - mean[2]) * axis[2];
        lo = lo.min(t);
        hi = hi.max(t);
    }
    let clamp = |c: [f32; 3]| [c[0].clamp(0.0, 255.0), c[1].clamp(0.0, 255.0), c[2].clamp(0.0, 255.0)];
    let mut e0 = clamp([mean[0] + axis[0] * hi, mean[1] + axis[1] * hi, mean[2] + axis[2] * hi]);
    let mut e1 = clamp([mean[0] + axis[0] * lo, mean[1] + axis[1] * lo, mean[2] + axis[2] * lo]);
    let mut best: Option<([u8; 8], f32)> = None;
    for _ in 0..4 {
        let (c0, c1) = (to565(e0), to565(e1));
        let pal = palette(c0, c1);
        let (idx, err) = assign(&pal, &pts);
        if best.as_ref().is_none_or(|(_, e)| err < *e) {
            best = Some((pack_bc1(c0, c1, &idx), err));
        }
        // least squares for the endpoints with these indices: weights of the first endpoint per index (the palette is a, b, 2a+b, a+2b)
        let w = [1.0f32, 0.0, 2.0 / 3.0, 1.0 / 3.0];
        let (mut aa, mut bb, mut ab) = (0f32, 0f32, 0f32);
        let (mut ax, mut bx) = ([0f32; 3], [0f32; 3]);
        for (p, k) in pts.iter().zip(idx.iter()) {
            let (wa, wb) = (w[*k as usize], 1.0 - w[*k as usize]);
            aa += wa * wa;
            bb += wb * wb;
            ab += wa * wb;
            for c in 0..3 {
                ax[c] += wa * p[c];
                bx[c] += wb * p[c];
            }
        }
        let det = aa * bb - ab * ab;
        if det.abs() < 1e-6 {
            break;
        }
        for c in 0..3 {
            e0[c] = ((ax[c] * bb - bx[c] * ab) / det).clamp(0.0, 255.0);
            e1[c] = ((bx[c] * aa - ax[c] * ab) / det).clamp(0.0, 255.0);
        }
    }
    best.map_or_else(|| pack_bc1_constant(to565(mean)), |(b, _)| b)
}

fn pack_bc1_constant(c: u16) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&c.to_le_bytes());
    out[2..4].copy_from_slice(&c.to_le_bytes());
    out
}

/// The 8 bytes of a BC4-style block (an 8-value alpha ramp) for 16 values.
pub fn bc4_block(v: &[u8; 16]) -> [u8; 8] {
    let (lo, hi) = (*v.iter().min().expect("sixteen values"), *v.iter().max().expect("sixteen values"));
    let mut out = [0u8; 8];
    out[0] = hi;
    out[1] = lo;
    if lo == hi {
        return out;
    }
    let (a0, a1) = (u32::from(hi), u32::from(lo));
    let pal: [u32; 8] = [a0, a1, (6 * a0 + a1) / 7, (5 * a0 + 2 * a1) / 7, (4 * a0 + 3 * a1) / 7, (3 * a0 + 4 * a1) / 7, (2 * a0 + 5 * a1) / 7, (a0 + 6 * a1) / 7];
    let mut bits = 0u64;
    for (i, x) in v.iter().enumerate() {
        let x = u32::from(*x);
        let k = (0..8).min_by_key(|k| pal[*k].abs_diff(x)).expect("eight entries") as u64;
        bits |= k << (3 * i);
    }
    out[2..8].copy_from_slice(&bits.to_le_bytes()[..6]);
    out
}

/// The 16 bytes of a BC3 block: the alpha ramp, then the colours.
pub fn bc3_block(px: &[[u8; 4]; 16]) -> [u8; 16] {
    let alpha: [u8; 16] = std::array::from_fn(|i| px[i][3]);
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&bc4_block(&alpha));
    out[8..].copy_from_slice(&bc1_block(px));
    out
}

/// The pixels of the 4x4 block at (`bx`, `by`); positions past the edge repeat the last pixel.
fn block_at(img: &Image, bx: usize, by: usize) -> [[u8; 4]; 16] {
    std::array::from_fn(|i| img.pixel((bx * 4 + i % 4).min(img.width - 1), (by * 4 + i / 4).min(img.height - 1)))
}

/// The block data of one level.
pub fn encode_level(img: &Image, format: Format) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(format.level_bytes(img.width, img.height));
    for by in 0..img.height.div_ceil(4) {
        for bx in 0..img.width.div_ceil(4) {
            let px = block_at(img, bx, by);
            match format {
                Format::Bc1 => out.extend(bc1_block(&px)),
                Format::Bc3 => out.extend(bc3_block(&px)),
                Format::Bc4 => out.extend(bc4_block(&std::array::from_fn(|i| px[i][0]))),
                Format::Bc5 => {
                    out.extend(bc4_block(&std::array::from_fn(|i| px[i][0])));
                    out.extend(bc4_block(&std::array::from_fn(|i| px[i][1])));
                }
                Format::Bc7 => return Err(DdsError::Unsupported("pictures cannot be encoded as BC7 (plain colours can)".to_string())),
            }
        }
    }
    Ok(out)
}

/// A `.dds` file of the picture and its mip levels, in the format of `spec`.
pub fn encode_dds(spec: Spec, img: &Image, levels: usize) -> Result<Vec<u8>> {
    let chain = img.mip_chain(levels)?;
    let blocks: Result<Vec<Vec<u8>>> = chain.iter().map(|l| encode_level(l, spec.format)).collect();
    dds_file(spec, img.width, img.height, &blocks?)
}

/// The 16 values of a BC4-style block (8 bytes).
pub fn decode_bc4_block(b: &[u8]) -> [u8; 16] {
    let (a0, a1) = (u32::from(b[0]), u32::from(b[1]));
    let pal: Vec<u32> = if a0 > a1 {
        let mut p = vec![a0, a1];
        p.extend((1..=6).map(|k| ((7 - k) * a0 + k * a1) / 7));
        p
    } else {
        let mut p = vec![a0, a1];
        p.extend((1..=4).map(|k| ((5 - k) * a0 + k * a1) / 5));
        p.extend([0, 255]);
        p
    };
    let mut bits = 0u64;
    for (i, x) in b[2..8].iter().enumerate() {
        bits |= u64::from(*x) << (8 * i);
    }
    std::array::from_fn(|i| pal[((bits >> (3 * i)) & 7) as usize] as u8)
}

/// The 16 colours of a BC1 block (8 bytes).
#[allow(clippy::needless_range_loop)]
pub fn decode_bc1_block(b: &[u8]) -> [[u8; 3]; 16] {
    let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
    let (a, c) = (from565(c0), from565(c1));
    let mut pal = [[0u8; 3]; 4];
    pal[0] = [a[0] as u8, a[1] as u8, a[2] as u8];
    pal[1] = [c[0] as u8, c[1] as u8, c[2] as u8];
    for k in 0..3 {
        let (x, y) = (u32::from(pal[0][k]), u32::from(pal[1][k]));
        let (p2, p3) = if c0 > c1 { (((2 * x + y) / 3) as u8, ((x + 2 * y) / 3) as u8) } else { (((x + y) / 2) as u8, 0) };
        pal[2][k] = p2;
        pal[3][k] = p3;
    }
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    std::array::from_fn(|i| pal[((bits >> (2 * i)) & 3) as usize])
}

/// The average colour of the top level of a `.dds` file in a format handled here (BC1, BC3, BC4, BC5), or `None`.
/// One-channel formats give the channel in red, the two-channel one red and green (the rest 0, alpha 255).
pub fn mean_color(dds: &[u8]) -> Option<[u8; 4]> {
    let info = crate::tpf::dds_info(dds)?;
    let spec = Spec::from_info(&info)?;
    let header = if info.four_cc == "DX10" { 148 } else { 128 };
    let (w, h) = (usize::try_from(info.width).ok()?, usize::try_from(info.height).ok()?);
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return None;
    }
    let data = dds.get(header..header + spec.format.level_bytes(w, h))?;
    let mut sum = [0u64; 4];
    let mut n = 0u64;
    for block in data.chunks_exact(spec.format.block_bytes()) {
        match spec.format {
            Format::Bc1 => {
                for c in decode_bc1_block(block) {
                    for k in 0..3 {
                        sum[k] += u64::from(c[k]);
                    }
                    sum[3] += 255;
                }
            }
            Format::Bc3 => {
                let alpha = decode_bc4_block(&block[..8]);
                for (c, a) in decode_bc1_block(&block[8..]).iter().zip(alpha.iter()) {
                    for k in 0..3 {
                        sum[k] += u64::from(c[k]);
                    }
                    sum[3] += u64::from(*a);
                }
            }
            Format::Bc4 => {
                for v in decode_bc4_block(block) {
                    sum[0] += u64::from(v);
                    sum[3] += 255;
                }
            }
            Format::Bc5 => {
                for (r, g) in decode_bc4_block(&block[..8]).iter().zip(decode_bc4_block(&block[8..]).iter()) {
                    sum[0] += u64::from(*r);
                    sum[1] += u64::from(*g);
                    sum[3] += 255;
                }
            }
            Format::Bc7 => return None,
        }
        n += 16;
    }
    (n > 0).then(|| std::array::from_fn(|k| ((sum[k] + n / 2) / n) as u8))
}

// ------------------------------------------------------------------------------------------------ a plain colour

/// One BC7 block of mode 6 that is the colour everywhere (exact when the four channels share their lowest bit, within one
/// level otherwise).
fn bc7_constant_block(c: [u8; 4]) -> [u8; 16] {
    let p = (c[1] & 1) as u128; // the shared low bit of both endpoints
    let mut bits: u128 = 1 << 6; // mode 6: six zero bits, then a one
    let mut at = 7;
    for ch in [0, 1, 2, 3] {
        let v = u128::from(c[ch] >> 1);
        // both endpoints of the channel carry the colour
        bits |= v << at;
        bits |= v << (at + 7);
        at += 14;
    }
    bits |= p << at;
    bits |= p << (at + 1);
    bits.to_le_bytes()
}

/// The one block that is `rgba` everywhere, in the format.
pub fn constant_block(format: Format, rgba: [u8; 4]) -> Vec<u8> {
    let px = [rgba; 16];
    match format {
        Format::Bc1 => bc1_block(&px).to_vec(),
        Format::Bc3 => bc3_block(&px).to_vec(),
        Format::Bc4 => bc4_block(&[rgba[0]; 16]).to_vec(),
        Format::Bc5 => [bc4_block(&[rgba[0]; 16]), bc4_block(&[rgba[1]; 16])].concat(),
        Format::Bc7 => bc7_constant_block(rgba).to_vec(),
    }
}

/// A `.dds` file of one colour with the size and number of levels given, in the format of `spec`.
pub fn constant_dds(spec: Spec, width: usize, height: usize, levels: usize, rgba: [u8; 4]) -> Result<Vec<u8>> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE || levels == 0 || levels > 16 {
        return Err(DdsError::BadImage(format!("{width}x{height} with {levels} levels is not usable")));
    }
    let block = constant_block(spec.format, rgba);
    let chain: Vec<Vec<u8>> = (0..levels)
        .map(|i| {
            let (w, h) = ((width >> i).max(1), (height >> i).max(1));
            block.iter().copied().cycle().take(spec.format.level_bytes(w, h)).collect()
        })
        .collect();
    dds_file(spec, width, height, &chain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tpf::dds_info;

    /// Mean squared error per channel value of a picture against its encoded and decoded form.
    fn mse_bc1(img: &Image) -> f64 {
        let mut total = 0f64;
        let mut n = 0f64;
        for by in 0..img.height / 4 {
            for bx in 0..img.width / 4 {
                let px = block_at(img, bx, by);
                let dec = decode_bc1_block(&bc1_block(&px));
                for (p, d) in px.iter().zip(dec.iter()) {
                    for k in 0..3 {
                        total += (f64::from(p[k]) - f64::from(d[k])).powi(2);
                        n += 1.0;
                    }
                }
            }
        }
        total / n
    }

    fn gradient(w: usize, h: usize) -> Image {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, ((x + y) * 255 / (w + h - 2)) as u8, 255]);
            }
        }
        Image::new(w, h, px).unwrap()
    }

    fn noise(w: usize, h: usize) -> Image {
        let mut s = 0x1234_5678u32;
        let px = (0..w * h * 4)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s >> 24) as u8
            })
            .collect();
        Image::new(w, h, px).unwrap()
    }

    #[test]
    fn bc1_keeps_smooth_pictures_close_and_flat_ones_exact() {
        // a ramp from one colour to another lies on a line in colour space, which is what a block can hold
        let ramp = {
            let mut px = Vec::new();
            for _ in 0..32 {
                for x in 0..32usize {
                    px.extend([(x * 8) as u8, (255 - x * 8) as u8, (40 + x * 5) as u8, 255]);
                }
            }
            Image::new(32, 32, px).unwrap()
        };
        assert!(mse_bc1(&ramp) < 6.0, "{}", mse_bc1(&ramp)); // about the noise of 5-6-5 bit endpoints (4.0)
        // a picture that changes in two colour directions per block loses the second one: bounded, not exact
        let g = gradient(32, 32);
        assert!(mse_bc1(&g) < 60.0, "{}", mse_bc1(&g));
        let flat = Image::filled(8, 8, [255, 255, 8, 255]).unwrap();
        assert_eq!(mse_bc1(&flat), 0.0, "a colour that 565 can hold is stored exactly");
        // noise is the worst case for a block format: only a sanity bound
        assert!(mse_bc1(&noise(32, 32)) < 6000.0);
        // one colour per 4x4 block is exact as well
        let mut px = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                px.extend(if (x / 4 + y / 4) % 2 == 0 { [255u8, 0, 0, 255] } else { [0, 255, 255, 255] });
            }
        }
        assert_eq!(mse_bc1(&Image::new(8, 8, px).unwrap()), 0.0);
    }

    #[test]
    fn bc4_ramps_hold_the_extremes_and_stay_close() {
        let v: [u8; 16] = std::array::from_fn(|i| (i * 17) as u8);
        let d = decode_bc4_block(&bc4_block(&v));
        assert_eq!((d[0], d[15]), (0, 255));
        assert!(v.iter().zip(d.iter()).all(|(a, b)| a.abs_diff(*b) <= 20), "{d:?}");
        assert_eq!(decode_bc4_block(&bc4_block(&[77; 16])), [77; 16]);
    }

    #[test]
    fn a_picture_becomes_a_dds_that_names_its_format_and_levels() {
        let img = gradient(64, 32);
        let spec = Spec { format: Format::Bc3, dxgi: None };
        let dds = encode_dds(spec, &img, 4).unwrap();
        let info = dds_info(&dds).unwrap();
        assert_eq!((info.width, info.height, info.mip_levels, info.four_cc.as_str()), (64, 32, 4, "DXT5"));
        assert_eq!(dds.len(), 128 + 16 * (16 * 8 + 8 * 4 + 4 * 2 + 2));
        assert_eq!(Spec::from_info(&info), Some(spec));
        // the extended header carries the DXGI number
        let dx = Spec { format: Format::Bc1, dxgi: Some(72) };
        let dds = encode_dds(dx, &img, 1).unwrap();
        let info = dds_info(&dds).unwrap();
        assert_eq!((info.four_cc.as_str(), info.dxgi_format), ("DX10", Some(72)));
        assert_eq!(Spec::from_info(&info), Some(dx));
        assert_eq!(dds.len(), 128 + 20 + 8 * 16 * 8);
        // BC7 pictures are not encoded; the old header cannot name BC7
        assert!(encode_dds(Spec { format: Format::Bc7, dxgi: Some(98) }, &img, 1).is_err());
        assert!(dds_file(Spec { format: Format::Bc7, dxgi: None }, 4, 4, &[vec![0; 16]]).is_err());
        assert_eq!(Spec { format: Format::Bc3, dxgi: Some(77) }.legacy(), Some(spec));
    }

    #[test]
    fn plain_colours_are_made_in_every_format_and_size() {
        for (format, dxgi) in [(Format::Bc1, None), (Format::Bc3, Some(77)), (Format::Bc4, None), (Format::Bc5, Some(83)), (Format::Bc7, Some(98))] {
            let spec = Spec { format, dxgi };
            let dds = constant_dds(spec, 16, 8, 3, [128, 128, 255, 255]).unwrap();
            let info = dds_info(&dds).unwrap();
            assert_eq!((info.width, info.height, info.mip_levels), (16, 8, 3), "{}", format.name());
            let header = if dxgi.is_some() { 148 } else { 128 };
            assert_eq!(dds.len(), header + format.level_bytes(16, 8) + format.level_bytes(8, 4) + format.level_bytes(4, 2), "{}", format.name());
        }
        // what the blocks hold
        assert_eq!(decode_bc4_block(&constant_block(Format::Bc5, [128, 129, 0, 0])[..8]), [128; 16]);
        assert_eq!(decode_bc4_block(&constant_block(Format::Bc5, [128, 129, 0, 0])[8..]), [129; 16]);
        let d = decode_bc1_block(&constant_block(Format::Bc1, [128, 128, 255, 255]));
        assert!(d.iter().all(|p| p[0].abs_diff(128) <= 4 && p[1].abs_diff(128) <= 2 && p[2] == 255), "{d:?}");
        let b7 = constant_block(Format::Bc7, [200, 100, 50, 255]);
        assert_eq!(b7[0] & 0x7F, 0x40, "mode 6 starts with six zero bits and a one");
        assert!(constant_dds(Spec { format: Format::Bc1, dxgi: None }, 0, 4, 1, [0; 4]).is_err());
    }

    #[test]
    fn the_average_colour_of_a_texture_is_read_back_from_its_blocks() {
        for format in [Format::Bc1, Format::Bc3, Format::Bc4, Format::Bc5] {
            let spec = Spec { format, dxgi: None }.legacy().unwrap_or(Spec { format, dxgi: Some(80) });
            let dds = constant_dds(spec, 16, 16, 3, [255, 0, 255, 255]).unwrap();
            let m = mean_color(&dds).unwrap_or_else(|| panic!("{}", format.name()));
            match format {
                Format::Bc1 | Format::Bc3 => assert_eq!(m, [255, 0, 255, 255], "{}", format.name()),
                Format::Bc4 => assert_eq!(m, [255, 0, 0, 255]),
                _ => assert_eq!(m, [255, 0, 0, 255]),
            }
        }
        // a picture: the average of a left-to-right ramp is its middle
        let ramp = {
            let mut px = Vec::new();
            for _ in 0..16 {
                for x in 0..16usize {
                    px.extend([(x * 17) as u8, (x * 17) as u8, (x * 17) as u8, 255]);
                }
            }
            Image::new(16, 16, px).unwrap()
        };
        let dds = encode_dds(Spec { format: Format::Bc1, dxgi: None }, &ramp, 1).unwrap();
        let m = mean_color(&dds).unwrap();
        assert!(m[0].abs_diff(128) <= 6 && m[1].abs_diff(128) <= 6, "{m:?}");
        assert_eq!(mean_color(&constant_dds(Spec { format: Format::Bc7, dxgi: Some(98) }, 8, 8, 1, [1, 2, 3, 4]).unwrap()), None, "BC7 is not decoded here");
        assert_eq!(mean_color(b"not a dds file"), None);
        assert_eq!(mean_color(&dds[..130]), None, "a file cut short");
    }

    #[test]
    fn resizing_averages_areas_and_keeps_flat_colours() {
        let img = Image::new(2, 2, vec![0, 0, 0, 255, 100, 100, 100, 255, 200, 200, 200, 255, 100, 100, 100, 255]).unwrap();
        let one = img.resized(1, 1).unwrap();
        assert_eq!(&one.rgba[..4], &[100, 100, 100, 255]);
        let flat = Image::filled(5, 3, [10, 20, 30, 40]).unwrap();
        for (w, h) in [(2, 2), (7, 9), (1, 1), (5, 3)] {
            let r = flat.resized(w, h).unwrap();
            assert!(r.rgba.chunks_exact(4).all(|p| p == [10, 20, 30, 40]), "{w}x{h}");
        }
        let chain = gradient(16, 16).mip_chain(10).unwrap();
        assert_eq!(chain.iter().map(|i| i.width).collect::<Vec<_>>(), vec![16, 8, 4, 2, 1]);
        assert!(Image::new(0, 4, vec![]).is_err() && Image::new(2, 2, vec![0; 3]).is_err());
        assert_eq!(gradient(4, 4).with_alpha(7).rgba[3], 7);
    }
}
