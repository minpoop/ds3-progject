//! `TPF`: the container for the textures of a Dark Souls III model (`*.tpf` inside `wp_a_0200.partsbnd.dcx`). PC files only.
//!
//! Layout as documented by the Souls modding community:
//!
//! ```text
//! 0x00 "TPF\0"  0x04 u32 data size  0x08 u32 texture count  0x0C u8 platform (0 = PC), u8 flag 2, u8 encoding (1 = UTF-16),
//!      u8 0
//! 0x10 per texture a header: u32 file offset, u32 file size, u8 format, u8 type (0 texture, 1 cube map, 2 volume), u8 mip
//!      maps, u8 flags 1, u32 name offset, i32 float struct flag (1: i32 unknown, i32 byte length, floats follow)
//! then the names (UTF-16, zero terminated), then the files (each a DDS file, aligned to 4 bytes)
//! ```
//!
//! Every texture's bytes are kept exactly as stored, so a TPF that was read can be written back unchanged.
use crate::util::{get, u16_le, u32_le};
use std::fmt;

const MAX_TEXTURES: usize = 4096;
const MAX_NAME_UNITS: usize = 512;
const MAX_FILE: usize = 1 << 28;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TpfError {
    NotTpf,
    Unsupported(String),
    Truncated { what: &'static str },
    Malformed(String),
}

impl fmt::Display for TpfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TpfError::NotTpf => write!(f, "not a TPF file"),
            TpfError::Unsupported(what) => write!(f, "not supported: {what}"),
            TpfError::Truncated { what } => write!(f, "the TPF file is cut off ({what})"),
            TpfError::Malformed(why) => write!(f, "damaged TPF file: {why}"),
        }
    }
}

impl std::error::Error for TpfError {}

type Result<T> = std::result::Result<T, TpfError>;

#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub name: String,
    /// The game's format number of the texture (an index into its own table, not the DDS format).
    pub format: u8,
    pub kind: u8,
    pub mipmaps: u8,
    pub flags1: u8,
    pub float_struct: Option<(i32, Vec<f32>)>,
    /// The DDS file.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tpf {
    pub flag2: u8,
    pub encoding: u8,
    pub textures: Vec<Texture>,
}

fn utf16_at(b: &[u8], at: usize) -> Result<String> {
    let mut units = Vec::new();
    let mut p = at;
    loop {
        let u = u16_le(b, p).ok_or(TpfError::Truncated { what: "a texture name" })?;
        if u == 0 {
            break;
        }
        units.push(u);
        if units.len() > MAX_NAME_UNITS {
            return Err(TpfError::Malformed("a texture name without an end".to_string()));
        }
        p += 2;
    }
    Ok(String::from_utf16_lossy(&units))
}

impl Tpf {
    pub fn parse(b: &[u8]) -> Result<Tpf> {
        if !b.starts_with(b"TPF\0") {
            return Err(TpfError::NotTpf);
        }
        if b.len() < 0x10 {
            return Err(TpfError::Truncated { what: "header" });
        }
        let count = u32_le(b, 8).map(|c| c as usize).filter(|c| *c <= MAX_TEXTURES).ok_or(TpfError::Malformed("an impossible texture count".to_string()))?;
        let (platform, flag2, encoding, zero) = (b[0xC], b[0xD], b[0xE], b[0xF]);
        if platform != 0 {
            return Err(TpfError::Unsupported(format!("a TPF of platform {platform} (only PC files are read)")));
        }
        if zero != 0 || flag2 > 3 {
            return Err(TpfError::Malformed("unexpected header bytes".to_string()));
        }
        if encoding != 1 {
            return Err(TpfError::Unsupported(format!("TPF names in encoding {encoding} (Dark Souls III uses UTF-16)")));
        }
        let mut textures = Vec::with_capacity(count);
        let mut p = 0x10usize;
        for _ in 0..count {
            let file_offset = u32_le(b, p).ok_or(TpfError::Truncated { what: "a texture header" })? as usize;
            let file_size = u32_le(b, p + 4).ok_or(TpfError::Truncated { what: "a texture header" })? as usize;
            let hdr = get(b, p + 8, 4).ok_or(TpfError::Truncated { what: "a texture header" })?;
            let (format, kind, mipmaps, flags1) = (hdr[0], hdr[1], hdr[2], hdr[3]);
            let name_offset = u32_le(b, p + 12).ok_or(TpfError::Truncated { what: "a texture header" })? as usize;
            let has_float = u32_le(b, p + 16).ok_or(TpfError::Truncated { what: "a texture header" })?;
            p += 20;
            if !matches!(flags1, 0 | 1 | 0x80) {
                return Err(TpfError::Unsupported(format!("a texture with flags {flags1} (compressed textures are not read)")));
            }
            let float_struct = match has_float {
                0 => None,
                1 => {
                    let unk = u32_le(b, p).ok_or(TpfError::Truncated { what: "a float struct" })? as i32;
                    let len = u32_le(b, p + 4).ok_or(TpfError::Truncated { what: "a float struct" })? as usize;
                    if !len.is_multiple_of(4) || len > 4096 {
                        return Err(TpfError::Malformed("an impossible float struct length".to_string()));
                    }
                    let raw = get(b, p + 8, len).ok_or(TpfError::Truncated { what: "a float struct" })?;
                    p += 8 + len;
                    Some((unk, raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()))
                }
                _ => return Err(TpfError::Malformed("an unknown float struct flag".to_string())),
            };
            if file_size > MAX_FILE {
                return Err(TpfError::Malformed("a texture that is too big".to_string()));
            }
            let bytes = get(b, file_offset, file_size).ok_or(TpfError::Truncated { what: "texture data" })?.to_vec();
            textures.push(Texture { name: utf16_at(b, name_offset)?, format, kind, mipmaps, flags1, float_struct, bytes });
        }
        Ok(Tpf { flag2, encoding, textures })
    }

    /// Writes the TPF as the game's tools do: headers, names, then the files aligned to 4 bytes.
    pub fn write(&self) -> Vec<u8> {
        let mut o: Vec<u8> = Vec::new();
        o.extend(b"TPF\0");
        o.extend(0u32.to_le_bytes());
        o.extend((self.textures.len() as u32).to_le_bytes());
        o.extend([0, self.flag2, self.encoding, 0]);
        let mut header_at = Vec::new();
        for t in &self.textures {
            header_at.push(o.len());
            o.extend(0u32.to_le_bytes()); // offset
            o.extend(0u32.to_le_bytes()); // size
            o.extend([t.format, t.kind, t.mipmaps, t.flags1]);
            o.extend(0u32.to_le_bytes()); // name offset
            o.extend(u32::from(t.float_struct.is_some()).to_le_bytes());
            if let Some((unk, values)) = &t.float_struct {
                o.extend(unk.to_le_bytes());
                o.extend((values.len() as u32 * 4).to_le_bytes());
                for v in values {
                    o.extend(v.to_le_bytes());
                }
            }
        }
        for (i, t) in self.textures.iter().enumerate() {
            let at = o.len() as u32;
            o[header_at[i] + 12..header_at[i] + 16].copy_from_slice(&at.to_le_bytes());
            for u in t.name.encode_utf16() {
                o.extend(u.to_le_bytes());
            }
            o.extend([0, 0]);
        }
        let data_start = o.len();
        for (i, t) in self.textures.iter().enumerate() {
            if !t.bytes.is_empty() {
                while !o.len().is_multiple_of(4) {
                    o.push(0);
                }
            }
            let at = o.len() as u32;
            o[header_at[i]..header_at[i] + 4].copy_from_slice(&at.to_le_bytes());
            o[header_at[i] + 4..header_at[i] + 8].copy_from_slice(&(t.bytes.len() as u32).to_le_bytes());
            o.extend(&t.bytes);
        }
        let size = (o.len() - data_start) as u32;
        o[4..8].copy_from_slice(&size.to_le_bytes());
        o
    }
}

impl Tpf {
    /// The position of the texture with this name (names are compared without regard to upper and lower case).
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.textures.iter().position(|t| t.name.eq_ignore_ascii_case(name))
    }

    /// Puts another `.dds` file into texture `index`, and the number of mip levels its header names; the game's format
    /// number, the type and the flags of the entry stay.
    pub fn replace_dds(&mut self, index: usize, dds: Vec<u8>) -> Result<()> {
        let info = dds_info(&dds).ok_or_else(|| TpfError::Malformed("the new file is not a DDS file".to_string()))?;
        let levels = u8::try_from(info.mip_levels.max(1)).map_err(|_| TpfError::Malformed("too many mip levels".to_string()))?;
        let t = self.textures.get_mut(index).ok_or_else(|| TpfError::Malformed(format!("there is no texture {index}")))?;
        t.bytes = dds;
        t.mipmaps = levels;
        Ok(())
    }

    /// One line per texture for reports: name, the game's format number, type, mip maps, flags and what the DDS header says.
    pub fn describe(&self) -> Vec<String> {
        let mut out = vec![format!("TPF with {} textures (flag 2: {}, names in encoding {})", self.textures.len(), self.flag2, self.encoding)];
        for t in &self.textures {
            let dds = dds_info(&t.bytes).map_or("not a DDS file".to_string(), |d| {
                format!(
                    "DDS {}x{}, {} mip levels, {}{}{}, {} bits per pixel",
                    d.width,
                    d.height,
                    d.mip_levels,
                    if d.four_cc.is_empty() { "uncompressed".to_string() } else { d.four_cc.clone() },
                    d.dxgi_format.map_or(String::new(), |f| format!(" (DXGI format {f})")),
                    if d.cube_map { ", cube map" } else { "" },
                    d.bits_per_pixel
                )
            });
            out.push(format!("  {}: format {} type {} mip maps {} flags {} float struct {:?}; {} bytes; {dds}", t.name, t.format, t.kind, t.mipmaps, t.flags1, t.float_struct, t.bytes.len()));
        }
        out
    }
}

/// What the start of a DDS file says (for reports and checks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdsInfo {
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    /// The four letters of the pixel format (`DXT1`, `DXT5`, `DX10`, ...), or empty for an uncompressed one.
    pub four_cc: String,
    /// The DXGI format of a `DX10` file.
    pub dxgi_format: Option<u32>,
    pub cube_map: bool,
    pub bits_per_pixel: u32,
}

pub fn dds_info(b: &[u8]) -> Option<DdsInfo> {
    if !b.starts_with(b"DDS ") || u32_le(b, 4)? != 124 {
        return None;
    }
    let flags = u32_le(b, 8)?;
    let height = u32_le(b, 12)?;
    let width = u32_le(b, 16)?;
    let mips = if flags & 0x20000 != 0 { u32_le(b, 28)? } else { 1 };
    let pf_flags = u32_le(b, 80)?;
    let four_cc = if pf_flags & 4 != 0 { String::from_utf8_lossy(get(b, 84, 4)?).to_string() } else { String::new() };
    let bits = u32_le(b, 88)?;
    let caps2 = u32_le(b, 112)?;
    let dxgi_format = if four_cc == "DX10" { u32_le(b, 128) } else { None };
    Some(DdsInfo { width, height, mip_levels: mips, four_cc, dxgi_format, cube_map: caps2 & 0x200 != 0, bits_per_pixel: bits })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dds(four_cc: &[u8; 4], w: u32, h: u32, mips: u32, payload: usize) -> Vec<u8> {
        let mut d = vec![0u8; 128 + payload];
        d[..4].copy_from_slice(b"DDS ");
        d[4..8].copy_from_slice(&124u32.to_le_bytes());
        d[8..12].copy_from_slice(&0x0002_100Fu32.to_le_bytes());
        d[12..16].copy_from_slice(&h.to_le_bytes());
        d[16..20].copy_from_slice(&w.to_le_bytes());
        d[28..32].copy_from_slice(&mips.to_le_bytes());
        d[76..80].copy_from_slice(&32u32.to_le_bytes());
        d[80..84].copy_from_slice(&4u32.to_le_bytes());
        d[84..88].copy_from_slice(four_cc);
        d
    }

    fn sample() -> Tpf {
        Tpf {
            flag2: 3,
            encoding: 1,
            textures: vec![
                Texture { name: "wp_a_9999_a".to_string(), format: 1, kind: 0, mipmaps: 3, flags1: 0, float_struct: None, bytes: dds(b"DXT5", 64, 64, 3, 5461) },
                Texture { name: "wp_a_9999_n".to_string(), format: 9, kind: 0, mipmaps: 1, flags1: 0, float_struct: Some((0, vec![1.0, 2.5])), bytes: dds(b"DXT1", 16, 16, 1, 129) },
            ],
        }
    }

    #[test]
    fn a_tpf_writes_and_reads_back_the_same_and_aligns_its_files() {
        let tpf = sample();
        let bytes = tpf.write();
        assert_eq!(&bytes[..4], b"TPF\0");
        let back = Tpf::parse(&bytes).unwrap();
        assert_eq!(back, tpf);
        assert_eq!(back.write(), bytes);
        for t in &back.textures {
            assert!(t.bytes.starts_with(b"DDS "));
        }
        // the data size field counts the files with their padding
        let data_size = u32_le(&bytes, 4).unwrap() as usize;
        assert!(data_size >= tpf.textures.iter().map(|t| t.bytes.len()).sum::<usize>());
        assert!(data_size < bytes.len());
    }

    #[test]
    fn dds_headers_are_described() {
        let info = dds_info(&dds(b"DXT5", 64, 32, 3, 0)).unwrap();
        assert_eq!((info.width, info.height, info.mip_levels, info.four_cc.as_str(), info.dxgi_format, info.cube_map), (64, 32, 3, "DXT5", None, false));
        let mut dx10 = dds(b"DX10", 8, 8, 1, 32);
        dx10[128..132].copy_from_slice(&98u32.to_le_bytes());
        assert_eq!(dds_info(&dx10).unwrap().dxgi_format, Some(98));
        assert_eq!(dds_info(b"not a dds file"), None);
        assert_eq!(dds_info(&[]), None);
    }

    #[test]
    fn what_is_not_a_pc_tpf_is_refused_and_damage_never_panics() {
        assert_eq!(Tpf::parse(b"DDS xxxxxxxxxxxxxxxxxxxx"), Err(TpfError::NotTpf));
        let mut x = sample().write();
        x[0xC] = 2;
        assert!(matches!(Tpf::parse(&x), Err(TpfError::Unsupported(_))));
        let mut x = sample().write();
        x[0xE] = 0;
        assert!(matches!(Tpf::parse(&x), Err(TpfError::Unsupported(_))));
        let bytes = sample().write();
        for cut in 0..bytes.len() {
            assert!(Tpf::parse(&bytes[..cut]).is_err(), "cut at {cut}");
        }
        for i in 0..bytes.len() {
            let mut x = bytes.clone();
            x[i] ^= 0xFF;
            let _ = Tpf::parse(&x);
        }
        let mut x = bytes.clone();
        x[8..12].copy_from_slice(&0x7FFF_FFFFu32.to_le_bytes());
        assert!(Tpf::parse(&x).is_err());
    }
}
