//! Wwise "modified Vorbis" (`.wem`, format tag 0xFFFF) -> standard Vorbis packets -> PCM.
//!
//! Wwise strips the Vorbis headers of its audio: the setup header refers to codebooks by number (a fixed library is
//! compiled into the Wwise runtime), and the audio packets drop the packet-type bit and the window-shape bits. To play
//! such a file the standard headers and packets have to be rebuilt. This is a Rust re-implementation of the algorithm
//! of **ww2ogg** by hcs (Adam Gashlin), which is distributed under the BSD-3-Clause license:
//!
//! > Copyright (c) 2002, Xiph.org Foundation
//! > Copyright (c) 2009-2016, Adam Gashlin
//!
//! (the full license text is shipped as `data/ww2ogg-COPYING.txt`). The codebook library
//! `data/packed_codebooks_aoTuV_603.bin` is ww2ogg's own data file, derived from the aoTuV/libvorbis codebooks (also
//! BSD licensed). Decoding the rebuilt packets is done by the `lewton` crate (MIT OR Apache-2.0).
//!
//! Only the variant Space Marine 2 uses is implemented: `fmt ` chunk of 0x42 bytes (the `vorb` block inside it),
//! 2-byte packet headers, external codebooks, stripped setup header, modified audio packets.
use crate::wem::{Pcm, WemInfo};
use anyhow::{anyhow, bail, ensure, Result};

static CODEBOOKS_AOTUV_603: &[u8] = include_bytes!("../data/packed_codebooks_aoTuV_603.bin");

// ------------------------------------------------------------------------------------------------ bit streams

/// Vorbis bit order: least significant bit of every byte first.
struct BitReader<'a> {
    data: &'a [u8],
    bits: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader { data, bits: 0 }
    }

    fn bit(&mut self) -> Result<bool> {
        let byte = *self.data.get(self.bits / 8).ok_or_else(|| anyhow!("ran out of bits"))?;
        let v = (byte >> (self.bits % 8)) & 1 != 0;
        self.bits += 1;
        Ok(v)
    }

    /// `n` bits (0..=32), the first one read being the least significant.
    fn read(&mut self, n: u32) -> Result<u32> {
        let mut v = 0u32;
        for i in 0..n {
            if self.bit()? {
                v |= 1u32 << i;
            }
        }
        Ok(v)
    }

    fn bits_read(&self) -> usize {
        self.bits
    }
}

struct BitWriter {
    out: Vec<u8>,
    cur: u8,
    nbits: u32,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter { out: Vec::new(), cur: 0, nbits: 0 }
    }

    fn put(&mut self, value: u32, bits: u32) {
        for i in 0..bits {
            if (value >> i) & 1 != 0 {
                self.cur |= 1 << self.nbits;
            }
            self.nbits += 1;
            if self.nbits == 8 {
                self.out.push(self.cur);
                self.cur = 0;
                self.nbits = 0;
            }
        }
    }

    fn put_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.put(b as u32, 8);
        }
    }

    /// Finish the packet: pad the last byte with zero bits.
    fn finish(mut self) -> Vec<u8> {
        if self.nbits != 0 {
            self.out.push(self.cur);
        }
        self.out
    }
}

/// Number of bits needed to hold `v` (Vorbis `ilog`).
fn ilog(mut v: u32) -> u32 {
    let mut r = 0;
    while v != 0 {
        r += 1;
        v >>= 1;
    }
    r
}

/// Number of values a type-1 lookup table needs (from Tremor, as used by ww2ogg).
fn book_maptype1_quantvals(entries: u32, dimensions: u32) -> Result<u32> {
    if dimensions == 0 {
        bail!("codebook with 0 dimensions");
    }
    let bits = ilog(entries);
    let mut vals: u64 = (entries >> (((bits.max(1) - 1) * (dimensions - 1)) / dimensions)) as u64;
    for _ in 0..64 {
        let (mut acc, mut acc1) = (1u128, 1u128);
        for _ in 0..dimensions {
            acc *= vals as u128;
            acc1 *= vals as u128 + 1;
        }
        if acc <= entries as u128 && acc1 > entries as u128 {
            return Ok(vals as u32);
        }
        if acc > entries as u128 {
            vals = vals.saturating_sub(1);
        } else {
            vals += 1;
        }
    }
    bail!("could not size the lookup table")
}

// ------------------------------------------------------------------------------------------------ codebooks

/// ww2ogg's `packed_codebooks*.bin`: the codebooks back to back, then a table of u32 offsets; the last 4 bytes of the
/// file give the position of that table.
struct CodebookLibrary {
    data: &'static [u8],
    offsets: Vec<usize>,
}

impl CodebookLibrary {
    fn new(data: &'static [u8]) -> Result<Self> {
        ensure!(data.len() > 8, "codebook library is empty");
        let offset_offset = u32::from_le_bytes(data[data.len() - 4..].try_into().unwrap()) as usize;
        ensure!(offset_offset < data.len() && (data.len() - offset_offset) % 4 == 0, "codebook library is damaged");
        let offsets: Vec<usize> = data[offset_offset..].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap()) as usize).collect();
        ensure!(offsets.iter().all(|&o| o <= offset_offset), "codebook library offsets are out of range");
        Ok(CodebookLibrary { data: &data[..offset_offset], offsets })
    }

    fn count(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    fn get(&self, i: usize) -> Option<&[u8]> {
        if i >= self.count() {
            return None;
        }
        self.data.get(self.offsets[i]..self.offsets[i + 1])
    }

    /// Rebuild codebook `id` of the library as a standard Vorbis codebook.
    fn rebuild(&self, id: usize, out: &mut BitWriter) -> Result<()> {
        let cb = self.get(id).ok_or_else(|| anyhow!("invalid codebook id {id}"))?;
        let mut bis = BitReader::new(cb);
        rebuild_codebook(&mut bis, cb.len(), out)
    }
}

/// Convert one stripped codebook into the standard layout. `cb_size` is the number of bytes it must use up exactly.
fn rebuild_codebook(bis: &mut BitReader, cb_size: usize, bos: &mut BitWriter) -> Result<()> {
    // IN: 4 bit dimensions, 14 bit entry count.  OUT: 24 bit "BCV", 16 bit dimensions, 24 bit entry count
    let dimensions = bis.read(4)?;
    let entries = bis.read(14)?;
    bos.put(0x564342, 24);
    bos.put(dimensions, 16);
    bos.put(entries, 24);

    let ordered = bis.bit()?;
    bos.put(ordered as u32, 1);
    if ordered {
        let initial_length = bis.read(5)?;
        bos.put(initial_length, 5);
        let mut current = 0u32;
        while current < entries {
            let bits = ilog(entries - current);
            let number = bis.read(bits)?;
            bos.put(number, bits);
            current += number;
        }
        ensure!(current <= entries, "current_entry out of range");
    } else {
        let codeword_length_length = bis.read(3)?;
        let sparse = bis.bit()?;
        ensure!(codeword_length_length != 0 && codeword_length_length <= 5, "nonsense codeword length");
        bos.put(sparse as u32, 1);
        for _ in 0..entries {
            let mut present = true;
            if sparse {
                let p = bis.bit()?;
                bos.put(p as u32, 1);
                present = p;
            }
            if present {
                let len = bis.read(codeword_length_length)?;
                bos.put(len, 5);
            }
        }
    }

    let lookup_type = bis.read(1)?;
    bos.put(lookup_type, 4);
    match lookup_type {
        0 => {}
        1 => {
            let (min, max) = (bis.read(32)?, bis.read(32)?);
            let value_length = bis.read(4)?;
            let sequence_flag = bis.read(1)?;
            bos.put(min, 32);
            bos.put(max, 32);
            bos.put(value_length, 4);
            bos.put(sequence_flag, 1);
            let quantvals = book_maptype1_quantvals(entries, dimensions)?;
            for _ in 0..quantvals {
                let val = bis.read(value_length + 1)?;
                bos.put(val, value_length + 1);
            }
        }
        2 => bail!("didn't expect lookup type 2"),
        _ => bail!("invalid lookup type"),
    }

    // it must have used exactly the bytes of the library entry (if all bits of the last byte are used there is one extra 0 byte)
    ensure!(cb_size == 0 || bis.bits_read() / 8 + 1 == cb_size, "codebook size mismatch ({} bytes used, {} expected)", bis.bits_read() / 8 + 1, cb_size);
    Ok(())
}

// ------------------------------------------------------------------------------------------------ the wem

/// What the `fmt ` chunk (and the optional `vorb` chunk) of a Wwise Vorbis file say.
#[derive(Debug, Clone)]
pub struct VorbisInfo {
    pub channels: u16,
    pub sample_rate: u32,
    pub avg_bytes_per_sec: u32,
    /// exact number of samples per channel
    pub sample_count: u32,
    pub mod_packets: bool,
    pub no_granule: bool,
    pub blocksize_0_pow: u8,
    pub blocksize_1_pow: u8,
    pub setup_packet_offset: usize,
    pub first_audio_packet_offset: usize,
}

/// The rebuilt standard Vorbis stream: three header packets, then the audio packets.
#[derive(Debug, Clone)]
pub struct VorbisStream {
    pub info: VorbisInfo,
    pub ident: Vec<u8>,
    pub comment: Vec<u8>,
    pub setup: Vec<u8>,
    pub audio: Vec<Vec<u8>>,
}

fn u16le(b: &[u8], at: usize) -> Result<u32> {
    Ok(u16::from_le_bytes(b.get(at..at + 2).ok_or_else(|| anyhow!("file truncated"))?.try_into().unwrap()) as u32)
}
fn u32le(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(b.get(at..at + 4).ok_or_else(|| anyhow!("file truncated"))?.try_into().unwrap()))
}

/// One packet of the data chunk: (payload start, payload length, next packet start), offsets relative to the data chunk.
fn packet_at(data: &[u8], offset: usize, no_granule: bool) -> Result<(usize, usize, usize)> {
    let header = if no_granule { 2 } else { 6 };
    let size = u16le(data, offset)? as usize;
    let start = offset + header;
    ensure!(start + size <= data.len(), "packet runs past the end of the data");
    Ok((start, size, start + size))
}

impl VorbisInfo {
    pub fn parse(wem: &[u8], w: &WemInfo) -> Result<VorbisInfo> {
        ensure!(w.format_tag == 0xFFFF, "not a Wwise Vorbis file (format tag 0x{:04X})", w.format_tag);
        ensure!(w.channels >= 1 && w.channels <= 8, "unsupported channel count {}", w.channels);
        // where the `vorb` block is: inside the 0x42-byte fmt chunk, or a chunk of its own
        let (vorb_off, vorb_size): (usize, Option<usize>) = match w.vorb {
            Some((o, n)) => (o, Some(n)),
            None => {
                ensure!(w.fmt_size == 0x42, "expected a 0x42-byte fmt chunk when there is no vorb chunk (got {})", w.fmt_size);
                (w.fmt_offset + 0x18, None)
            }
        };
        let size_class = vorb_size.unwrap_or(0x2A);
        let sample_count = u32le(wem, vorb_off)?;
        let (no_granule, mod_packets, setup_at, first_audio_at);
        match size_class {
            0x2A => {
                no_granule = true;
                let mod_signal = u32le(wem, vorb_off + 4)?;
                // 0xD9, 0xCB, 0xBC, 0xB2 mean modified packets; 0x4A, 0x4B, 0x69, 0x70 do not
                mod_packets = !matches!(mod_signal, 0x4A | 0x4B | 0x69 | 0x70);
                setup_at = u32le(wem, vorb_off + 0x10)? as usize;
                first_audio_at = u32le(wem, vorb_off + 0x14)? as usize;
            }
            0x32 | 0x34 => {
                no_granule = false;
                mod_packets = false;
                setup_at = u32le(wem, vorb_off + 0x18)? as usize;
                first_audio_at = u32le(wem, vorb_off + 0x1C)? as usize;
            }
            other => bail!("unsupported vorb block size 0x{other:X} (an older Wwise file layout)"),
        }
        let uid_at = if size_class == 0x2A { vorb_off + 0x24 } else { vorb_off + 0x2C };
        let _uid = u32le(wem, uid_at)?;
        let b0 = *wem.get(uid_at + 4).ok_or_else(|| anyhow!("file truncated"))?;
        let b1 = *wem.get(uid_at + 5).ok_or_else(|| anyhow!("file truncated"))?;
        ensure!((6..=13).contains(&b0) && (6..=13).contains(&b1) && b0 <= b1, "implausible block sizes 2^{b0} / 2^{b1}");
        Ok(VorbisInfo {
            channels: w.channels,
            sample_rate: w.sample_rate,
            avg_bytes_per_sec: w.avg_bytes_per_sec,
            sample_count,
            mod_packets,
            no_granule,
            blocksize_0_pow: b0,
            blocksize_1_pow: b1,
            setup_packet_offset: setup_at,
            first_audio_packet_offset: first_audio_at,
        })
    }
}

fn vorbis_packet_header(w: &mut BitWriter, kind: u8) {
    w.put(kind as u32, 8);
    w.put_bytes(b"vorbis");
}

/// Convert a Wwise Vorbis `.wem` into standard Vorbis header and audio packets.
pub fn to_vorbis_packets(wem: &[u8]) -> Result<VorbisStream> {
    let w = WemInfo::parse(wem)?;
    let info = VorbisInfo::parse(wem, &w)?;
    let data = wem.get(w.data_offset..w.data_offset + w.data_len).ok_or_else(|| anyhow!("data chunk is cut off"))?;
    let lib = CodebookLibrary::new(CODEBOOKS_AOTUV_603)?;
    let channels = info.channels as u32;

    // --- identification header
    let mut id = BitWriter::new();
    vorbis_packet_header(&mut id, 1);
    id.put(0, 32); // version
    id.put(channels, 8);
    id.put(info.sample_rate, 32);
    id.put(0, 32); // bitrate max
    id.put(info.avg_bytes_per_sec.wrapping_mul(8), 32); // nominal
    id.put(0, 32); // bitrate min
    id.put(info.blocksize_0_pow as u32, 4);
    id.put(info.blocksize_1_pow as u32, 4);
    id.put(1, 1); // framing
    let ident = id.finish();

    // --- comment header: no comments
    let mut cm = BitWriter::new();
    vorbis_packet_header(&mut cm, 3);
    let vendor = b"Ashen Marine (Wwise Vorbis rebuilt as in ww2ogg)";
    cm.put(vendor.len() as u32, 32);
    cm.put_bytes(vendor);
    cm.put(0, 32); // user comment count
    cm.put(1, 1); // framing
    let comment = cm.finish();

    // --- setup header, rebuilt from the stripped one
    let (setup_start, setup_size, setup_next) = packet_at(data, info.setup_packet_offset, info.no_granule)?;
    if !info.no_granule {
        ensure!(u32le(data, info.setup_packet_offset + 2)? == 0, "setup packet granule != 0");
    }
    let setup_payload = &data[setup_start..setup_start + setup_size];
    let mut ss = BitReader::new(setup_payload);
    let mut os = BitWriter::new();
    vorbis_packet_header(&mut os, 5);

    let codebook_count_less1 = ss.read(8)?;
    os.put(codebook_count_less1, 8);
    let codebook_count = codebook_count_less1 + 1;
    for _ in 0..codebook_count {
        let codebook_id = ss.read(10)? as usize;
        lib.rebuild(codebook_id, &mut os).map_err(|e| anyhow!("codebook {codebook_id}: {e}"))?;
    }
    // time domain transforms (placeholder)
    os.put(0, 6);
    os.put(0, 16);

    // floors
    let floor_count_less1 = ss.read(6)?;
    os.put(floor_count_less1, 6);
    let floor_count = floor_count_less1 + 1;
    for _ in 0..floor_count {
        os.put(1, 16); // always floor type 1
        let partitions = ss.read(5)?;
        os.put(partitions, 5);
        let mut partition_class = Vec::with_capacity(partitions as usize);
        let mut maximum_class = 0u32;
        for _ in 0..partitions {
            let c = ss.read(4)?;
            os.put(c, 4);
            partition_class.push(c);
            maximum_class = maximum_class.max(c);
        }
        let mut class_dims = Vec::with_capacity(maximum_class as usize + 1);
        for _ in 0..=maximum_class {
            let dims_less1 = ss.read(3)?;
            os.put(dims_less1, 3);
            class_dims.push(dims_less1 + 1);
            let subclasses = ss.read(2)?;
            os.put(subclasses, 2);
            if subclasses != 0 {
                let masterbook = ss.read(8)?;
                os.put(masterbook, 8);
                ensure!(masterbook < codebook_count, "invalid floor1 masterbook");
            }
            for _ in 0..(1u32 << subclasses) {
                let book_plus1 = ss.read(8)?;
                os.put(book_plus1, 8);
                ensure!((book_plus1 as i64 - 1) < codebook_count as i64, "invalid floor1 subclass book");
            }
        }
        let multiplier_less1 = ss.read(2)?;
        os.put(multiplier_less1, 2);
        let rangebits = ss.read(4)?;
        os.put(rangebits, 4);
        for &cls in &partition_class {
            for _ in 0..class_dims[cls as usize] {
                let x = ss.read(rangebits)?;
                os.put(x, rangebits);
            }
        }
    }

    // residues
    let residue_count_less1 = ss.read(6)?;
    os.put(residue_count_less1, 6);
    let residue_count = residue_count_less1 + 1;
    for _ in 0..residue_count {
        let residue_type = ss.read(2)?;
        os.put(residue_type, 16);
        ensure!(residue_type <= 2, "invalid residue type");
        let begin = ss.read(24)?;
        let end = ss.read(24)?;
        let partition_size_less1 = ss.read(24)?;
        let classifications_less1 = ss.read(6)?;
        let classbook = ss.read(8)?;
        os.put(begin, 24);
        os.put(end, 24);
        os.put(partition_size_less1, 24);
        os.put(classifications_less1, 6);
        os.put(classbook, 8);
        let classifications = classifications_less1 + 1;
        ensure!(classbook < codebook_count, "invalid residue classbook");
        let mut cascade = Vec::with_capacity(classifications as usize);
        for _ in 0..classifications {
            let low = ss.read(3)?;
            os.put(low, 3);
            let flag = ss.read(1)?;
            os.put(flag, 1);
            let mut high = 0;
            if flag != 0 {
                high = ss.read(5)?;
                os.put(high, 5);
            }
            cascade.push(high * 8 + low);
        }
        for &c in &cascade {
            for k in 0..8 {
                if c & (1 << k) != 0 {
                    let book = ss.read(8)?;
                    os.put(book, 8);
                    ensure!(book < codebook_count, "invalid residue book");
                }
            }
        }
    }

    // mappings
    let mapping_count_less1 = ss.read(6)?;
    os.put(mapping_count_less1, 6);
    let mapping_count = mapping_count_less1 + 1;
    for _ in 0..mapping_count {
        os.put(0, 16); // always mapping type 0
        let submaps_flag = ss.read(1)?;
        os.put(submaps_flag, 1);
        let mut submaps = 1;
        if submaps_flag != 0 {
            let less1 = ss.read(4)?;
            os.put(less1, 4);
            submaps = less1 + 1;
        }
        let square_polar_flag = ss.read(1)?;
        os.put(square_polar_flag, 1);
        if square_polar_flag != 0 {
            let steps_less1 = ss.read(8)?;
            os.put(steps_less1, 8);
            let bits = ilog(channels.saturating_sub(1));
            for _ in 0..=steps_less1 {
                let magnitude = ss.read(bits)?;
                let angle = ss.read(bits)?;
                os.put(magnitude, bits);
                os.put(angle, bits);
                ensure!(angle != magnitude && magnitude < channels && angle < channels, "invalid coupling");
            }
        }
        let reserved = ss.read(2)?; // a rare reserved field not removed by Wwise
        os.put(reserved, 2);
        ensure!(reserved == 0, "mapping reserved field nonzero");
        if submaps > 1 {
            for _ in 0..channels {
                let mux = ss.read(4)?;
                os.put(mux, 4);
                ensure!(mux < submaps, "mapping_mux >= submaps");
            }
        }
        for _ in 0..submaps {
            let time_config = ss.read(8)?;
            os.put(time_config, 8);
            let floor_number = ss.read(8)?;
            os.put(floor_number, 8);
            ensure!(floor_number < floor_count, "invalid floor mapping");
            let residue_number = ss.read(8)?;
            os.put(residue_number, 8);
            ensure!(residue_number < residue_count, "invalid residue mapping");
        }
    }

    // modes
    let mode_count_less1 = ss.read(6)?;
    os.put(mode_count_less1, 6);
    let mode_count = mode_count_less1 + 1;
    let mode_bits = ilog(mode_count - 1);
    let mut mode_blockflag = Vec::with_capacity(mode_count as usize);
    for _ in 0..mode_count {
        let block_flag = ss.read(1)?;
        os.put(block_flag, 1);
        mode_blockflag.push(block_flag != 0);
        os.put(0, 16); // window type
        os.put(0, 16); // transform type
        let mapping = ss.read(8)?;
        os.put(mapping, 8);
        ensure!(mapping < mapping_count, "invalid mode mapping");
    }
    os.put(1, 1); // framing
    let setup = os.finish();

    ensure!((ss.bits_read() + 7) / 8 == setup_size, "didn't read exactly the setup packet ({} of {} bytes)", (ss.bits_read() + 7) / 8, setup_size);
    ensure!(setup_next == info.first_audio_packet_offset, "the first audio packet doesn't follow the setup packet");

    // --- audio packets
    let mut audio = Vec::new();
    let mut prev_blockflag = false;
    let mut offset = info.first_audio_packet_offset;
    while offset < data.len() {
        let header = if info.no_granule { 2 } else { 6 };
        ensure!(offset + header <= data.len(), "packet header truncated");
        let (start, size, next) = packet_at(data, offset, info.no_granule)?;
        offset = next;
        if size == 0 {
            continue;
        }
        let payload = &data[start..start + size];
        let mut out = BitWriter::new();
        if info.mod_packets {
            ensure!(!mode_blockflag.is_empty(), "no modes were loaded");
            // rebuild the packet type bit and the window flags that Wwise removed
            out.put(0, 1);
            let mut ps = BitReader::new(payload);
            let mode_number = ps.read(mode_bits)? as usize;
            ensure!(mode_number < mode_blockflag.len(), "mode number {mode_number} out of range");
            out.put(mode_number as u32, mode_bits);
            let remainder = ps.read(8 - mode_bits)?;
            if mode_blockflag[mode_number] {
                // long window: look at the next packet to learn its window type
                let mut next_blockflag = false;
                if next + header <= data.len() {
                    let (nstart, nsize, _) = packet_at(data, next, info.no_granule)?;
                    if nsize > 0 {
                        let mut ns = BitReader::new(&data[nstart..nstart + nsize]);
                        let next_mode = ns.read(mode_bits)? as usize;
                        next_blockflag = *mode_blockflag.get(next_mode).ok_or_else(|| anyhow!("next mode number {next_mode} out of range"))?;
                    }
                }
                out.put(prev_blockflag as u32, 1);
                out.put(next_blockflag as u32, 1);
            }
            prev_blockflag = mode_blockflag[mode_number];
            out.put(remainder, 8 - mode_bits);
            out.put_bytes(&payload[1..]);
        } else {
            out.put_bytes(payload);
        }
        audio.push(out.finish());
    }

    Ok(VorbisStream { info, ident, comment, setup, audio })
}

/// Decode a Wwise Vorbis `.wem` into PCM (16-bit, interleaved), trimmed to the sample count the file declares.
pub fn decode(wem: &[u8]) -> Result<Pcm> {
    use lewton::audio::{read_audio_packet, PreviousWindowRight};
    use lewton::header::{read_header_comment, read_header_ident, read_header_setup};

    let stream = to_vorbis_packets(wem)?;
    let ident = read_header_ident(&stream.ident).map_err(|e| anyhow!("identification header: {e:?}"))?;
    let _comment = read_header_comment(&stream.comment).map_err(|e| anyhow!("comment header: {e:?}"))?;
    let setup = read_header_setup(&stream.setup, ident.audio_channels, (ident.blocksize_0, ident.blocksize_1)).map_err(|e| anyhow!("setup header: {e:?}"))?;
    let channels = stream.info.channels as usize;
    let mut pwr = PreviousWindowRight::new();
    let mut samples: Vec<i16> = Vec::with_capacity(stream.info.sample_count as usize * channels);
    for (i, packet) in stream.audio.iter().enumerate() {
        let decoded = read_audio_packet(&ident, &setup, packet, &mut pwr).map_err(|e| anyhow!("audio packet {i}: {e:?}"))?;
        let frames = decoded.first().map_or(0, |c| c.len());
        for f in 0..frames {
            for ch in decoded.iter().take(channels) {
                samples.push(ch[f]);
            }
        }
    }
    let wanted = stream.info.sample_count as usize * channels;
    if wanted > 0 && samples.len() > wanted {
        samples.truncate(wanted);
    }
    Ok(Pcm { channels: stream.info.channels, sample_rate: stream.info.sample_rate, samples })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_streams_are_lsb_first_and_round_trip() {
        let mut w = BitWriter::new();
        w.put(0b101, 3);
        w.put(0xABC, 12);
        w.put(1, 1);
        w.put(0xDEADBEEF, 32);
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read(3).unwrap(), 0b101);
        assert_eq!(r.read(12).unwrap(), 0xABC);
        assert_eq!(r.read(1).unwrap(), 1);
        assert_eq!(r.read(32).unwrap(), 0xDEADBEEF);
        // the first byte holds the first bits in its low end
        assert_eq!(bytes[0] & 0b111, 0b101);
        assert!(BitReader::new(&[]).read(1).is_err());
    }

    #[test]
    fn ilog_and_quantvals_match_the_reference_values() {
        assert_eq!((ilog(0), ilog(1), ilog(2), ilog(255), ilog(256)), (0, 1, 2, 8, 9));
        // vals^dims <= entries < (vals+1)^dims
        for (entries, dims) in [(8u32, 3u32), (100, 2), (4096, 4), (5, 1), (1000, 3)] {
            let v = book_maptype1_quantvals(entries, dims).unwrap() as u64;
            assert!(v.pow(dims) <= entries as u64 && (v + 1).pow(dims) > entries as u64, "{entries} {dims} -> {v}");
        }
        assert!(book_maptype1_quantvals(10, 0).is_err());
    }

    #[test]
    fn the_codebook_library_loads_and_every_codebook_rebuilds_to_a_standard_one() {
        let lib = CodebookLibrary::new(CODEBOOKS_AOTUV_603).unwrap();
        assert!(lib.count() > 500, "{}", lib.count());
        assert!(lib.get(lib.count()).is_none());
        for id in 0..lib.count() {
            let mut out = BitWriter::new();
            lib.rebuild(id, &mut out).unwrap_or_else(|e| panic!("codebook {id}: {e}"));
            let bytes = out.finish();
            // every standard codebook starts with the 24-bit sync pattern "BCV"
            assert_eq!(&bytes[..3], b"BCV", "codebook {id}");
        }
    }

    #[test]
    fn rejects_files_that_are_not_wwise_vorbis() {
        let pcm = crate::wem::test_wem(1, 1, 22050, 16, &[], &[0u8; 100]);
        assert!(to_vorbis_packets(&pcm).unwrap_err().to_string().contains("not a Wwise Vorbis"));
        let odd = crate::wem::test_wem(0xFFFF, 1, 48000, 0, &[0u8; 8], &[1, 2, 3, 4]);
        assert!(to_vorbis_packets(&odd).is_err());
    }
}
