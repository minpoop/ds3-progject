//! A writer of synthetic BND4 files in every format variant the reader knows (names or not, ids or not, long offsets or
//! not, compression fields or not, with and without the hash table). Written from the layout facts in [`crate::bnd4`].
use crate::bnd4::Format;
use crate::hash::path_hash;

#[derive(Clone, Debug)]
pub struct FileSpec {
    pub id: i32,
    pub name: String,
    pub data: Vec<u8>,
    /// The decoded flag bits (`0x02` normally; set bit 0 to mark the file compressed on its own).
    pub flags: u8,
}

#[derive(Clone, Debug)]
pub struct Bnd4Spec {
    /// The format byte as stored in the file (`0x74` for Dark Souls III's text files).
    pub raw_format: u8,
    pub unicode: bool,
    pub hash_table: bool,
    pub alignment: u64,
    /// Pad the end of the file to the alignment, too.
    pub tail_padding: bool,
    pub version: [u8; 8],
    pub files: Vec<FileSpec>,
}

impl Bnd4Spec {
    pub fn new(raw_format: u8) -> Bnd4Spec {
        Bnd4Spec { raw_format, unicode: true, hash_table: true, alignment: 0x10, tail_padding: false, version: *b"07D7R6\0\0", files: Vec::new() }
    }

    pub fn file(mut self, id: i32, name: &str, data: &[u8]) -> Bnd4Spec {
        self.files.push(FileSpec { id, name: name.to_string(), data: data.to_vec(), flags: 0x02 });
        self
    }

    pub fn format(&self) -> Format {
        Format::from_raw(self.raw_format, false)
    }

    fn pad(out: &mut Vec<u8>, align: u64) {
        while out.len() as u64 % align != 0 {
            out.push(0);
        }
    }

    /// The file's bytes.
    pub fn build(&self) -> Vec<u8> {
        let format = self.format();
        let header_len = format.file_header_len();
        let n = self.files.len();
        let mut out = vec![0u8; 0x40];
        out[..4].copy_from_slice(b"BND4");
        out[10] = 1; // not bit big endian
        out[0x0C..0x10].copy_from_slice(&(n as i32).to_le_bytes());
        out[0x10..0x18].copy_from_slice(&0x40i64.to_le_bytes());
        out[0x18..0x20].copy_from_slice(&self.version);
        out[0x20..0x28].copy_from_slice(&(header_len as i64).to_le_bytes());
        out[0x30] = u8::from(self.unicode);
        out[0x31] = self.raw_format;
        out[0x32] = if self.hash_table { 4 } else { 0 };

        // file headers (sizes, offsets and name offsets are filled in below)
        let first_header = out.len();
        for f in &self.files {
            let mut h = vec![0u8; header_len];
            h[0] = f.flags.reverse_bits();
            h[4..8].copy_from_slice(&(-1i32).to_le_bytes());
            let mut cursor = 16 + if format.has_compression_field() { 8 } else { 0 } + if format.long_offsets() { 8 } else { 4 };
            if format.has_ids() {
                h[cursor..cursor + 4].copy_from_slice(&f.id.to_le_bytes());
                cursor += 4;
            }
            if format.has_names() {
                cursor += 4;
            }
            if format.0 == Format::NAMES1 {
                h[cursor..cursor + 4].copy_from_slice(&f.id.to_le_bytes());
            }
            out.extend(h);
        }
        // names
        let mut name_offsets = Vec::new();
        if format.has_names() {
            for f in &self.files {
                name_offsets.push(out.len() as u32);
                if self.unicode {
                    for u in f.name.encode_utf16().chain(Some(0)) {
                        out.extend(u.to_le_bytes());
                    }
                } else {
                    out.extend(f.name.bytes());
                    out.push(0);
                }
            }
        }
        // hash table
        let mut table_offset = 0u64;
        if self.hash_table {
            Self::pad(&mut out, 8);
            table_offset = out.len() as u64;
            let mut groups = (n / 7) as u32;
            while !is_prime(groups) {
                groups += 1;
            }
            let mut buckets: Vec<Vec<(u32, i32)>> = vec![Vec::new(); groups as usize];
            for (i, f) in self.files.iter().enumerate() {
                let hash = path_hash(&f.name);
                buckets[(hash % groups) as usize].push((hash, i as i32));
            }
            for b in &mut buckets {
                b.sort();
            }
            let hashes_offset = table_offset + 16 + u64::from(groups) * 8;
            out.extend((hashes_offset as i64).to_le_bytes());
            out.extend(groups.to_le_bytes());
            out.extend([0x10, 8, 8, 0]);
            let mut index = 0i32;
            for b in &buckets {
                out.extend((b.len() as i32).to_le_bytes());
                out.extend(index.to_le_bytes());
                index += b.len() as i32;
            }
            for (hash, i) in buckets.iter().flatten() {
                out.extend(hash.to_le_bytes());
                out.extend(i.to_le_bytes());
            }
        }
        let headers_end = out.len() as u64;
        out[0x28..0x30].copy_from_slice(&(headers_end as i64).to_le_bytes());
        out[0x38..0x40].copy_from_slice(&(table_offset as i64).to_le_bytes());

        // data
        let mut placed = Vec::new();
        for f in &self.files {
            if !f.data.is_empty() {
                Self::pad(&mut out, self.alignment);
            }
            placed.push(out.len() as u64);
            out.extend(&f.data);
        }
        if self.tail_padding {
            Self::pad(&mut out, self.alignment);
        }
        // fill in the headers
        for (i, f) in self.files.iter().enumerate() {
            let at = first_header + i * header_len;
            out[at + 8..at + 16].copy_from_slice(&(f.data.len() as i64).to_le_bytes());
            let mut cursor = at + 16;
            if format.has_compression_field() {
                out[cursor..cursor + 8].copy_from_slice(&(f.data.len() as i64).to_le_bytes());
                cursor += 8;
            }
            if format.long_offsets() {
                out[cursor..cursor + 8].copy_from_slice(&(placed[i] as i64).to_le_bytes());
                cursor += 8;
            } else {
                out[cursor..cursor + 4].copy_from_slice(&(placed[i] as u32).to_le_bytes());
                cursor += 4;
            }
            if format.has_ids() {
                cursor += 4;
            }
            if format.has_names() {
                out[cursor..cursor + 4].copy_from_slice(&name_offsets[i].to_le_bytes());
            }
        }
        out
    }
}

fn is_prime(p: u32) -> bool {
    p >= 2 && (2..p).take_while(|d| d * d <= p).all(|d| p % d != 0)
}

/// A small sample: five files with names like the game's (`N:\...\x.fmg`), different sizes, one of them empty.
pub fn sample_files() -> Vec<(i32, String, Vec<u8>)> {
    vec![
        (10, "N:\\FDP\\data\\INTERROOT_win64\\msg\\ENGLISH\\GoodsName.fmg".to_string(), (0..37u32).map(|i| (i * 5 + 1) as u8).collect()),
        (11, "N:\\FDP\\data\\INTERROOT_win64\\msg\\ENGLISH\\WeaponName.fmg".to_string(), (0..100u32).map(|i| (i * 3 + 2) as u8).collect()),
        (12, "N:\\FDP\\data\\INTERROOT_win64\\msg\\ENGLISH\\WeaponInfo.fmg".to_string(), (0..16u32).map(|i| (i + 100) as u8).collect()),
        (13, "N:\\FDP\\data\\INTERROOT_win64\\msg\\ENGLISH\\Empty.fmg".to_string(), Vec::new()),
        (14, "N:\\FDP\\data\\INTERROOT_win64\\msg\\ENGLISH\\WeaponCaption.fmg".to_string(), (0..250u32).map(|i| (i * 7 + 9) as u8).collect()),
    ]
}

pub fn sample(raw_format: u8, hash_table: bool) -> Vec<u8> {
    let mut spec = Bnd4Spec::new(raw_format);
    spec.hash_table = hash_table;
    for (id, name, data) in sample_files() {
        spec = spec.file(id, &name, &data);
    }
    spec.build()
}
