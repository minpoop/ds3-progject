//! Minimal `.wav` reader for the sound files the setup tool writes (and any ordinary PCM wav): 8, 16, 24 or 32 bit
//! integer samples, any channel count. Pure, so it is tested on any OS.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wav {
    pub channels: u16,
    pub sample_rate: u32,
    /// interleaved, signed 16-bit
    pub samples: Vec<i16>,
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Parse a RIFF/WAVE file. The error says what is wrong in plain words (it ends up in the log).
pub fn parse(b: &[u8]) -> Result<Wav, String> {
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut p = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // (tag, channels, rate, bits)
    let mut data: Option<&[u8]> = None;
    while p + 8 <= b.len() {
        let len = u32le(b, p + 4).ok_or("truncated chunk header")? as usize;
        let body = p + 8;
        let end = body.saturating_add(len).min(b.len());
        match &b[p..p + 4] {
            b"fmt " => {
                let tag = u16le(b, body).ok_or("short fmt chunk")?;
                let channels = u16le(b, body + 2).ok_or("short fmt chunk")?;
                let rate = u32le(b, body + 4).ok_or("short fmt chunk")?;
                let bits = u16le(b, body + 14).ok_or("short fmt chunk")?;
                // 0xFFFE = WAVE_FORMAT_EXTENSIBLE: the real format tag is the first two bytes of the sub-format
                let tag = if tag == 0xFFFE { u16le(b, body + 24).unwrap_or(tag) } else { tag };
                fmt = Some((tag, channels, rate, bits));
            }
            b"data" => data = Some(&b[body..end]),
            _ => {}
        }
        p = end + (len & 1);
    }
    let (tag, channels, sample_rate, bits) = fmt.ok_or("no fmt chunk")?;
    let data = data.ok_or("no data chunk")?;
    if tag != 1 {
        return Err(format!("format tag {tag} is not plain PCM"));
    }
    if channels == 0 || channels > 8 || sample_rate == 0 {
        return Err(format!("implausible format: {channels} channels at {sample_rate} Hz"));
    }
    let samples: Vec<i16> = match bits {
        8 => data.iter().map(|&x| ((x as i16) - 128) << 8).collect(),
        16 => data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect(),
        24 => data.chunks_exact(3).map(|c| i16::from_le_bytes([c[1], c[2]])).collect(),
        32 => data.chunks_exact(4).map(|c| i16::from_le_bytes([c[2], c[3]])).collect(),
        other => return Err(format!("{other}-bit samples are not supported")),
    };
    let whole = samples.len() / channels as usize * channels as usize;
    let mut samples = samples;
    samples.truncate(whole);
    Ok(Wav { channels, sample_rate, samples })
}

/// A 16-bit PCM wav (used by tests and by the probe's sample dump).
pub fn build(channels: u16, sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_16_bit() {
        let s: Vec<i16> = vec![0, 1, -1, i16::MAX, i16::MIN, 1234];
        let w = parse(&build(2, 48_000, &s)).unwrap();
        assert_eq!((w.channels, w.sample_rate), (2, 48_000));
        assert_eq!(w.samples, s);
    }

    #[test]
    fn other_bit_depths_are_scaled_to_16_bit() {
        let mk = |bits: u16, data: &[u8]| {
            let mut v = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
            v.extend(16u32.to_le_bytes());
            v.extend(1u16.to_le_bytes());
            v.extend(1u16.to_le_bytes());
            v.extend(8000u32.to_le_bytes());
            v.extend((8000 * bits as u32 / 8).to_le_bytes());
            v.extend((bits / 8).to_le_bytes());
            v.extend(bits.to_le_bytes());
            v.extend(b"data");
            v.extend((data.len() as u32).to_le_bytes());
            v.extend_from_slice(data);
            v
        };
        assert_eq!(parse(&mk(8, &[128, 255, 0])).unwrap().samples, vec![0, 127 << 8, -128 << 8]);
        assert_eq!(parse(&mk(24, &[0x00, 0x34, 0x12, 0x00, 0x00, 0x80])).unwrap().samples, vec![0x1234, i16::MIN]);
        assert_eq!(parse(&mk(32, &[0, 0, 0x78, 0x56])).unwrap().samples, vec![0x5678]);
    }

    #[test]
    fn odd_chunks_unknown_chunks_and_a_cut_off_data_chunk_are_handled() {
        let mut w = build(1, 22_050, &[1, 2, 3, 4]);
        // a LIST chunk with an odd length before data
        let data_at = w.windows(4).position(|x| x == b"data").unwrap();
        let mut extra = b"LIST".to_vec();
        extra.extend(3u32.to_le_bytes());
        extra.extend([1, 2, 3, 0]);
        w.splice(data_at..data_at, extra);
        assert_eq!(parse(&w).unwrap().samples, vec![1, 2, 3, 4]);
        // data chunk longer than the file: take what is there, whole frames only
        let mut cut = build(2, 44_100, &[1, 2, 3, 4]);
        cut.truncate(cut.len() - 3);
        assert_eq!(parse(&cut).unwrap().samples, vec![1, 2]);
    }

    #[test]
    fn rejects_what_it_cannot_play() {
        assert!(parse(b"OggS....").is_err());
        assert!(parse(b"RIFF\x04\0\0\0WAVE").unwrap_err().contains("fmt"));
        let mut f = build(1, 8000, &[0]);
        f[20] = 3; // float
        assert!(parse(&f).unwrap_err().contains("not plain PCM"));
        let mut z = build(1, 8000, &[0]);
        z[22] = 0; // zero channels
        assert!(parse(&z).unwrap_err().contains("implausible"));
    }
}
