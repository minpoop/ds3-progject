//! Wwise `.wem` audio files: a RIFF container whose `fmt ` chunk names the codec. This reads the header (so a report
//! can say what every file is) and decodes the codecs that are plain PCM. Other codecs are reported by name and are
//! added once the real files show which ones Space Marine 2 uses.
use anyhow::{anyhow, bail, Result};

#[derive(Debug, Clone, Default)]
pub struct WemInfo {
    /// `wFormatTag`: 0x0001 PCM, 0x0002 ADPCM, 0x0069 IMA ADPCM, 0xFFFE extensible, 0xFFFF Wwise Vorbis,
    /// 0x3039/0x3040/0x3041 Wwise Opus
    pub format_tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub avg_bytes_per_sec: u32,
    pub block_align: u16,
    pub bits_per_sample: u16,
    pub fmt_extra: Vec<u8>,
    pub data_offset: usize,
    pub data_len: usize,
    pub chunks: Vec<(String, usize)>,
    /// where the `fmt ` chunk body starts and how long the chunk says it is
    pub fmt_offset: usize,
    pub fmt_size: usize,
    /// (body offset, length) of a separate `vorb` chunk, when the file has one
    pub vorb: Option<(usize, usize)>,
}

pub fn codec_name(tag: u16) -> &'static str {
    match tag {
        0x0001 => "PCM",
        0x0002 => "ADPCM",
        0x0069 => "IMA ADPCM",
        0xFFFE => "extensible (PCM or other)",
        0xFFFF => "Wwise Vorbis",
        0x3039 | 0x3040 | 0x3041 => "Wwise Opus",
        _ => "unknown codec",
    }
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl WemInfo {
    pub fn parse(wem: &[u8]) -> Result<WemInfo> {
        if wem.len() < 12 || &wem[..4] != b"RIFF" || &wem[8..12] != b"WAVE" {
            bail!("not a RIFF/WAVE file (starts with {:02x?})", &wem[..wem.len().min(8)]);
        }
        let mut info = WemInfo::default();
        let mut p = 12;
        let mut have_fmt = false;
        while p + 8 <= wem.len() {
            let tag = String::from_utf8_lossy(&wem[p..p + 4]).to_string();
            let len = u32le(wem, p + 4).unwrap() as usize;
            let body = p + 8;
            let end = body.saturating_add(len).min(wem.len());
            info.chunks.push((tag.clone(), len));
            match tag.as_str() {
                "fmt " => {
                    info.format_tag = u16le(wem, body).ok_or_else(|| anyhow!("short fmt chunk"))?;
                    info.channels = u16le(wem, body + 2).unwrap_or(0);
                    info.sample_rate = u32le(wem, body + 4).unwrap_or(0);
                    info.avg_bytes_per_sec = u32le(wem, body + 8).unwrap_or(0);
                    info.block_align = u16le(wem, body + 12).unwrap_or(0);
                    info.bits_per_sample = u16le(wem, body + 14).unwrap_or(0);
                    if end > body + 16 {
                        info.fmt_extra = wem[body + 16..end].to_vec();
                    }
                    info.fmt_offset = body;
                    info.fmt_size = len;
                    have_fmt = true;
                }
                "vorb" => info.vorb = Some((body, len)),
                "data" => {
                    info.data_offset = body;
                    info.data_len = end - body;
                }
                _ => {}
            }
            p = end + (len & 1); // chunks are word aligned
        }
        if !have_fmt {
            bail!("no fmt chunk");
        }
        Ok(info)
    }

    /// Length in seconds, estimated from the average byte rate (exact for PCM).
    pub fn approx_seconds(&self) -> f64 {
        if self.avg_bytes_per_sec == 0 {
            0.0
        } else {
            self.data_len as f64 / self.avg_bytes_per_sec as f64
        }
    }
}

/// Decoded audio: interleaved signed 16-bit samples.
#[derive(Debug, Clone)]
pub struct Pcm {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

impl Pcm {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / (self.channels.max(1) as f64 * self.sample_rate.max(1) as f64)
    }
    pub fn peak(&self) -> i16 {
        self.samples.iter().map(|s| s.saturating_abs()).max().unwrap_or(0)
    }
}

/// Decode a wem into PCM: plain PCM and Wwise Vorbis (what Space Marine 2 uses). Any other codec returns an error that
/// names it.
pub fn decode(wem: &[u8]) -> Result<Pcm> {
    let info = WemInfo::parse(wem)?;
    if info.format_tag == 0xFFFF {
        return crate::wwvorbis::decode(wem);
    }
    let data = wem.get(info.data_offset..info.data_offset + info.data_len).ok_or_else(|| anyhow!("data chunk is cut off"))?;
    let is_pcm = info.format_tag == 0x0001 || (info.format_tag == 0xFFFE && info.fmt_extra.len() >= 24 && u16le(&info.fmt_extra, 8 + 8) == Some(1));
    if !is_pcm {
        bail!("{} (format tag 0x{:04X}) is not decoded yet", codec_name(info.format_tag), info.format_tag);
    }
    let samples: Vec<i16> = match info.bits_per_sample {
        16 => data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect(),
        8 => data.iter().map(|&b| ((b as i16) - 128) << 8).collect(),
        24 => data.chunks_exact(3).map(|b| i16::from_le_bytes([b[1], b[2]])).collect(),
        32 => data.chunks_exact(4).map(|b| i16::from_le_bytes([b[2], b[3]])).collect(),
        other => bail!("PCM with {other} bits per sample is not supported"),
    };
    Ok(Pcm { channels: info.channels.max(1), sample_rate: info.sample_rate, samples })
}

/// A standard 16-bit PCM `.wav` file.
pub fn wav_bytes(pcm: &Pcm) -> Vec<u8> {
    let data_len = pcm.samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&pcm.channels.to_le_bytes());
    out.extend_from_slice(&pcm.sample_rate.to_le_bytes());
    out.extend_from_slice(&(pcm.sample_rate * pcm.channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(pcm.channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for s in &pcm.samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(any(test, feature = "testing"))]
pub fn test_wem(tag: u16, channels: u16, rate: u32, bits: u16, extra: &[u8], data: &[u8]) -> Vec<u8> {
    let mut fmt = Vec::new();
    fmt.extend(tag.to_le_bytes());
    fmt.extend(channels.to_le_bytes());
    fmt.extend(rate.to_le_bytes());
    fmt.extend((rate * channels as u32 * bits as u32 / 8).to_le_bytes());
    fmt.extend((channels * bits / 8).to_le_bytes());
    fmt.extend(bits.to_le_bytes());
    fmt.extend_from_slice(extra);
    let mut out = b"RIFF".to_vec();
    out.extend(((4 + 8 + fmt.len() + 8 + data.len()) as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend((fmt.len() as u32).to_le_bytes());
    out.extend(fmt);
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headers_and_estimates_length() {
        let data = vec![0u8; 44100 * 2];
        let w = test_wem(1, 1, 44100, 16, &[], &data);
        let info = WemInfo::parse(&w).unwrap();
        assert_eq!((info.format_tag, info.channels, info.sample_rate), (1, 1, 44100));
        assert_eq!(info.data_len, data.len());
        assert!((info.approx_seconds() - 1.0).abs() < 1e-9);
        assert_eq!(codec_name(0xFFFF), "Wwise Vorbis");
    }

    #[test]
    fn decodes_pcm_round_trip_to_wav() {
        let samples: Vec<i16> = vec![0, 1000, -1000, i16::MAX, i16::MIN, 5];
        let raw: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let w = test_wem(1, 2, 48000, 16, &[], &raw);
        let pcm = decode(&w).unwrap();
        assert_eq!((pcm.channels, pcm.sample_rate), (2, 48000));
        assert_eq!(pcm.samples, samples);
        assert_eq!(pcm.peak(), i16::MAX);
        let wav = wav_bytes(&pcm);
        let again = decode(&wav).unwrap();
        assert_eq!(again.samples, samples);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + raw.len());
    }

    #[test]
    fn other_codecs_are_named_in_the_error() {
        let w = test_wem(0x3041, 2, 48000, 0, &[0u8; 8], &[1, 2, 3, 4]);
        let err = decode(&w).unwrap_err().to_string();
        assert!(err.contains("Wwise Opus") && err.contains("3041"), "{err}");
        // a damaged Wwise Vorbis file is an error too, not a panic
        let v = test_wem(0xFFFF, 2, 48000, 0, &[0u8; 8], &[1, 2, 3, 4]);
        assert!(decode(&v).is_err());
    }

    #[test]
    fn rejects_garbage_and_truncated_files() {
        assert!(WemInfo::parse(b"OggS....").is_err());
        assert!(WemInfo::parse(b"RIFF\x04\0\0\0WAVE").is_err(), "no fmt chunk");
        let mut w = test_wem(1, 1, 8000, 16, &[], &[0u8; 100]);
        w.truncate(w.len() - 40);
        let info = WemInfo::parse(&w).unwrap();
        assert!(info.data_len < 100, "data length is clamped to what is there");
    }
}
