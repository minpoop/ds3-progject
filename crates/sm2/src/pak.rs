//! Space Marine 2 `.pak` archives are standard zip files (stored and deflate entries). This module only ever
//! OPENS files for reading; it has no code path that writes, renames or deletes anything.
use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use zip::{CompressionMethod, ZipArchive};

/// A read-only window onto part of a stream. Used for the stored (uncompressed) zip files that sit inside a pak.
pub struct Section<R: Read + Seek> {
    inner: R,
    start: u64,
    len: u64,
    pos: u64,
}

impl<R: Read + Seek> Section<R> {
    pub fn new(inner: R, start: u64, len: u64) -> Self {
        Section { inner, start, len, pos: 0 }
    }
    pub fn len(&self) -> u64 { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }
}

impl<R: Read + Seek> Read for Section<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let want = buf.len().min((self.len - self.pos) as usize);
        self.inner.seek(SeekFrom::Start(self.start + self.pos))?;
        let n = self.inner.read(&mut buf[..want])?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for Section<R> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.len as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start of section"));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

/// A stored zip opened in place inside a pak (the sound paks hold their wem files this way).
pub type NestedZip = ZipArchive<Section<File>>;

/// What is known about one entry without reading its data.
#[derive(Debug, Clone)]
pub struct EntryInfo {
    pub pak: PathBuf,
    pub name: String,
    pub size: u64,
    pub compressed_size: u64,
    pub stored: bool,
}

struct Pak {
    path: PathBuf,
    archive: ZipArchive<File>,
}

/// Every `.pak` of an install, opened read-only, with one name index across all of them.
#[derive(Default)]
pub struct PakSet {
    paks: Vec<Pak>,
    /// lower-case name -> (pak, entry index); the first pak (sorted path order) wins on duplicates
    index: HashMap<String, (usize, usize)>,
    /// paks that could not be opened as zip (path, reason): reported, never fatal
    pub skipped: Vec<(PathBuf, String)>,
    pub duplicate_names: usize,
}

fn key(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

impl PakSet {
    pub fn new() -> Self { Self::default() }

    /// Open every `*.pak` below `dir` (recursively), in sorted path order.
    pub fn open_dir(dir: &Path) -> Result<PakSet> {
        let files = list_paks(dir).with_context(|| format!("listing {}", dir.display()))?;
        Self::open_files(&files)
    }

    pub fn open_files(files: &[PathBuf]) -> Result<PakSet> {
        let mut set = PakSet::new();
        for f in files {
            if let Err(e) = set.add(f) {
                set.skipped.push((f.clone(), format!("{e:#}")));
            }
        }
        Ok(set)
    }

    pub fn add(&mut self, path: &Path) -> Result<()> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let archive = ZipArchive::new(file).with_context(|| format!("{} is not a readable zip", path.display()))?;
        let pak_idx = self.paks.len();
        for i in 0..archive.len() {
            let Some(name) = archive.name_for_index(i) else { continue };
            if name.ends_with('/') {
                continue;
            }
            match self.index.entry(key(name)) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert((pak_idx, i));
                }
                // the earlier pak (sorted path order) keeps the name
                std::collections::hash_map::Entry::Occupied(_) => self.duplicate_names += 1,
            }
        }
        self.paks.push(Pak { path: path.to_path_buf(), archive });
        Ok(())
    }

    pub fn pak_count(&self) -> usize { self.paks.len() }
    pub fn entry_count(&self) -> usize { self.index.len() }
    pub fn paks(&self) -> impl Iterator<Item = &Path> { self.paks.iter().map(|p| p.path.as_path()) }

    pub fn contains(&self, name: &str) -> bool { self.index.contains_key(&key(name)) }

    /// All names (original spelling) for which `pred` returns true, sorted.
    pub fn find(&self, mut pred: impl FnMut(&str) -> bool) -> Vec<String> {
        let mut out = Vec::new();
        for (k, &(p, i)) in &self.index {
            if pred(k) {
                if let Some(n) = self.paks[p].archive.name_for_index(i) {
                    out.push(n.to_string());
                }
            }
        }
        out.sort();
        out
    }

    pub fn info(&mut self, name: &str) -> Result<EntryInfo> {
        let &(p, i) = self.index.get(&key(name)).ok_or_else(|| anyhow!("{name}: not in any pak"))?;
        let pak = &mut self.paks[p];
        let e = pak.archive.by_index_raw(i)?;
        Ok(EntryInfo {
            pak: pak.path.clone(),
            name: e.name().to_string(),
            size: e.size(),
            compressed_size: e.compressed_size(),
            stored: e.compression() == CompressionMethod::Stored,
        })
    }

    /// Read a whole entry. `max` guards against surprising sizes.
    pub fn read(&mut self, name: &str, max: u64) -> Result<Vec<u8>> {
        let &(p, i) = self.index.get(&key(name)).ok_or_else(|| anyhow!("{name}: not in any pak"))?;
        let mut e = self.paks[p].archive.by_index(i).with_context(|| format!("opening {name}"))?;
        if e.size() > max {
            bail!("{name}: {} bytes is more than the {max}-byte limit", e.size());
        }
        let mut out = Vec::with_capacity(e.size() as usize);
        e.read_to_end(&mut out).with_context(|| format!("reading {name}"))?;
        Ok(out)
    }

    pub fn read_text(&mut self, name: &str, max: u64) -> Result<String> {
        let bytes = self.read(name, max)?;
        Ok(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string())
    }

    /// Open a stored (uncompressed) zip that lives inside a pak, without extracting it.
    pub fn open_nested(&mut self, name: &str) -> Result<NestedZip> {
        let &(p, i) = self.index.get(&key(name)).ok_or_else(|| anyhow!("{name}: not in any pak"))?;
        let pak = &mut self.paks[p];
        // data_start is only known once the local header has been read
        let (start, len, stored) = {
            let e = pak.archive.by_index_raw(i)?;
            (e.data_start(), e.size(), e.compression() == CompressionMethod::Stored)
        };
        if !stored {
            bail!("{name}: nested zip is compressed, only stored zips can be opened in place");
        }
        let start = match start {
            Some(s) => s,
            None => {
                // by_index (not raw) forces the header read
                let e = pak.archive.by_index(i)?;
                e.data_start().ok_or_else(|| anyhow!("{name}: unknown data offset"))?
            }
        };
        let file = File::open(&pak.path).with_context(|| format!("reopen {}", pak.path.display()))?;
        let section = Section::new(file, start, len);
        ZipArchive::new(section).with_context(|| format!("{name} is not a zip"))
    }
}

/// Every `*.pak` below `dir`, sorted (recursive).
pub fn list_paks(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_paks(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_paks(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            collect_paks(&path, out)?;
        } else if ty.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pak")) {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn make_zip(path: &Path, entries: &[(&str, &[u8], bool)]) {
        let mut z = ZipWriter::new(File::create(path).unwrap());
        for (name, data, stored) in entries {
            let method = if *stored { CompressionMethod::Stored } else { CompressionMethod::Deflated };
            z.start_file(*name, SimpleFileOptions::default().compression_method(method)).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn reads_stored_and_deflated_entries_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let big: Vec<u8> = (0..5000u32).flat_map(|i| (i % 7) as u8..=(i % 7) as u8).collect();
        make_zip(&dir.path().join("a.pak"), &[("pct/Tex_d_0.pct_mip", &big, false), ("ssl/x.cls", b"hello", true)]);
        let mut set = PakSet::open_dir(dir.path()).unwrap();
        assert_eq!(set.pak_count(), 1);
        assert_eq!(set.entry_count(), 2);
        assert!(set.contains("PCT/tex_d_0.PCT_MIP"));
        assert_eq!(set.read("pct/tex_d_0.pct_mip", 1 << 20).unwrap(), big);
        assert_eq!(set.read_text("ssl/x.cls", 100).unwrap(), "hello");
        assert!(set.info("ssl/x.cls").unwrap().stored);
        assert!(!set.info("pct/tex_d_0.pct_mip").unwrap().stored);
        assert!(set.read("nope", 10).is_err());
        assert!(set.read("pct/tex_d_0.pct_mip", 10).is_err(), "size limit must apply");
    }

    #[test]
    fn opens_a_stored_zip_inside_a_pak_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let inner_path = dir.path().join("inner.zip");
        make_zip(&inner_path, &[("12345.wem", b"RIFFwemdata", false), ("67890.wem", b"RIFFother", true)]);
        let inner = std::fs::read(&inner_path).unwrap();
        make_zip(&dir.path().join("sound.pak"), &[("pad.bin", &[7u8; 333], true), ("sounds/wpn.zip", &inner, true)]);
        let mut set = PakSet::open_dir(dir.path()).unwrap();
        let mut nested = set.open_nested("sounds/wpn.zip").unwrap();
        assert_eq!(nested.len(), 2);
        let mut buf = Vec::new();
        nested.by_name("12345.wem").unwrap().read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"RIFFwemdata");
        buf.clear();
        nested.by_name("67890.wem").unwrap().read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"RIFFother");
    }

    #[test]
    fn a_broken_pak_is_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.pak"), b"this is not a zip").unwrap();
        make_zip(&dir.path().join("good.pak"), &[("a.txt", b"x", true)]);
        let set = PakSet::open_dir(dir.path()).unwrap();
        assert_eq!(set.pak_count(), 1);
        assert_eq!(set.skipped.len(), 1);
        assert!(set.contains("a.txt"));
    }

    #[test]
    fn section_reader_seeks_within_bounds() {
        let data: Vec<u8> = (0..100u8).collect();
        let mut s = Section::new(io::Cursor::new(data), 10, 20);
        let mut b = [0u8; 5];
        s.seek(SeekFrom::Start(3)).unwrap();
        s.read_exact(&mut b).unwrap();
        assert_eq!(b, [13, 14, 15, 16, 17]);
        s.seek(SeekFrom::End(-2)).unwrap();
        let mut rest = Vec::new();
        s.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, [28, 29]);
        assert!(s.seek(SeekFrom::Current(-1000)).is_err());
    }
}
