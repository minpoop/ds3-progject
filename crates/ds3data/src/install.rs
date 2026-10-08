//! A whole Dark Souls III install, opened read-only: the program file (for the archive keys), every archive in the `Game`
//! folder with a hash index, and look-ups by path.
//!
//! Nothing below the game folder is created, changed or deleted: the files are only opened for reading.
use crate::archive::{Archive, ArchiveError};
use crate::bhd5::Entry;
use crate::hash::path_hash;
use crate::keys::{io_reason, scan_reader, RsaPublicKey};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The game's program file, which holds the archive keys as plain text.
pub const EXE_NAME: &str = "DarkSoulsIII.exe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// `DarkSoulsIII.exe` is neither in `<root>/Game` nor in `<root>`.
    NoExe,
    /// A file or folder could not be read (`what` says which; no paths in messages).
    Io { what: &'static str, reason: String },
    /// No archive has a file with this path hash.
    NotInArchives { path: String },
    Archive(ArchiveError),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::NoExe => write!(f, "{EXE_NAME} was not found (looked in the Game folder and in the folder itself)"),
            InstallError::Io { what, reason } => write!(f, "cannot read the {what}: {reason}"),
            InstallError::NotInArchives { path } => write!(f, "{path} is not in any archive"),
            InstallError::Archive(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<ArchiveError> for InstallError {
    fn from(e: ArchiveError) -> Self {
        InstallError::Archive(e)
    }
}

/// What the program file gave: size, SHA-256 and the keys that are in it as PEM text.
#[derive(Debug, Clone)]
pub struct ExeInfo {
    pub size: u64,
    /// Lower-case hex.
    pub sha256: String,
    pub keys: Vec<RsaPublicKey>,
    /// `RSA PUBLIC KEY` blocks that were looked at, and how many of them were not usable keys.
    pub pem_blocks: usize,
    pub pem_rejected: usize,
}

impl ExeInfo {
    /// Reads the file once (in chunks) for its size, SHA-256 and PEM keys.
    pub fn scan(exe: &Path) -> Result<ExeInfo, InstallError> {
        let file = std::fs::File::open(exe).map_err(|e| InstallError::Io { what: "program file", reason: io_reason(&e) })?;
        let scan = scan_reader(file).map_err(|e| InstallError::Io { what: "program file", reason: io_reason(&e) })?;
        Ok(ExeInfo { size: scan.size, sha256: crate::util::hex(&scan.sha256), keys: scan.pem.keys, pem_blocks: scan.pem.blocks, pem_rejected: scan.pem.rejected })
    }
}

/// One `.bhd` of the `Game` folder: the archive if it could be opened, else why not.
#[derive(Debug)]
pub struct ArchiveSlot {
    /// File name without extension, e.g. `Data0`.
    pub name: String,
    pub bhd_size: u64,
    /// Size of the partner `.bdt`, if there is one.
    pub bdt_size: Option<u64>,
    pub archive: Result<Archive, ArchiveError>,
}

/// A file found in an archive.
#[derive(Debug, Clone, Copy)]
pub struct Hit<'a> {
    pub archive: &'a Archive,
    pub entry: &'a Entry,
}

impl Hit<'_> {
    /// The name of the archive the file is in, e.g. `Data0`.
    pub fn archive_name(&self) -> &str {
        self.archive.name()
    }
}

/// The folder that holds the program file: `<root>/Game` for the Steam folder, or `root` itself.
pub fn find_game_dir(root: &Path) -> Result<PathBuf, InstallError> {
    let game = find_child(root, "Game");
    for dir in [game.as_deref(), Some(root)].into_iter().flatten() {
        if find_child(dir, EXE_NAME).is_some_and(|p| p.is_file()) {
            return Ok(dir.to_path_buf());
        }
    }
    Err(InstallError::NoExe)
}

/// `dir/name`, found without regard to upper and lower case.
fn find_child(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name)).map(|e| e.path())
}

/// `name` without its `.ext` (any case of `ext`), if it has that extension.
fn stem_with_ext<'a>(name: &'a str, ext: &str) -> Option<&'a str> {
    let split = name.len().checked_sub(ext.len() + 1)?;
    if !name.is_char_boundary(split) {
        return None;
    }
    let (stem, dot_ext) = name.split_at(split);
    dot_ext.strip_prefix('.').is_some_and(|e| e.eq_ignore_ascii_case(ext)).then_some(stem)
}

/// The program file of an install.
pub fn exe_path(game_dir: &Path) -> PathBuf {
    find_child(game_dir, EXE_NAME).unwrap_or_else(|| game_dir.join(EXE_NAME))
}

/// An opened install.
#[derive(Debug)]
pub struct Ds3Install {
    game_dir: PathBuf,
    exe: ExeInfo,
    keys: Vec<RsaPublicKey>,
    slots: Vec<ArchiveSlot>,
}

impl Ds3Install {
    /// Opens the install at `root` (the Steam folder that holds `Game/DarkSoulsIII.exe`, or the `Game` folder itself):
    /// reads the program file once for its keys, then opens every `.bhd` with its `.bdt`, trying the keys of the program
    /// file and `extra_keys` on each. Archives that cannot be opened are kept in the list with the reason.
    pub fn open(root: &Path, extra_keys: &[RsaPublicKey]) -> Result<Ds3Install, InstallError> {
        Ds3Install::open_with_progress(root, extra_keys, &mut |_| {})
    }

    /// [`Ds3Install::open`], telling `progress` what it is doing.
    pub fn open_with_progress(root: &Path, extra_keys: &[RsaPublicKey], progress: &mut dyn FnMut(&str)) -> Result<Ds3Install, InstallError> {
        let game_dir = find_game_dir(root)?;
        progress("reading the program file for the archive keys");
        let exe = ExeInfo::scan(&exe_path(&game_dir))?;
        Ds3Install::open_with_exe(game_dir, exe, extra_keys, progress)
    }

    /// Opens the archives of `game_dir` when the program file has been scanned already.
    pub fn open_with_exe(game_dir: PathBuf, exe: ExeInfo, extra_keys: &[RsaPublicKey], progress: &mut dyn FnMut(&str)) -> Result<Ds3Install, InstallError> {
        let mut keys: Vec<RsaPublicKey> = exe.keys.clone();
        for k in extra_keys {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
        let listing = std::fs::read_dir(&game_dir).map_err(|e| InstallError::Io { what: "game folder", reason: io_reason(&e) })?;
        let mut files: Vec<(String, PathBuf)> = listing.filter_map(|e| e.ok()).map(|e| (e.file_name().to_string_lossy().to_string(), e.path())).collect();
        files.sort_by_key(|(name, _)| name.to_lowercase());
        let mut slots = Vec::new();
        let bhds: Vec<(&PathBuf, &str)> = files.iter().filter_map(|(name, path)| stem_with_ext(name, "bhd").map(|stem| (path, stem))).collect();
        for (i, (bhd, stem)) in bhds.iter().enumerate() {
            let stem = stem.to_string();
            let partner = files.iter().find(|(n, _)| stem_with_ext(n, "bdt").is_some_and(|s| s.eq_ignore_ascii_case(&stem))).map(|(_, p)| p.clone());
            let bhd_size = std::fs::metadata(bhd).map(|m| m.len()).unwrap_or(0);
            let bdt_size = partner.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len());
            let started = Instant::now();
            let archive = match &partner {
                Some(bdt) => Archive::open(bhd, bdt, &keys),
                None => Err(ArchiveError::Io { what: ".bdt file", reason: "no such file".to_string() }),
            };
            progress(&match &archive {
                Ok(a) => format!("opened {stem} ({}/{}): {} files, {:.1}s", i + 1, bhds.len(), a.entries().len(), started.elapsed().as_secs_f32()),
                Err(e) => format!("could not open {stem} ({}/{}): {e}", i + 1, bhds.len()),
            });
            slots.push(ArchiveSlot { name: stem, bhd_size, bdt_size, archive });
        }
        Ok(Ds3Install { game_dir, exe, keys, slots })
    }

    /// The folder that holds the program file and the archives.
    pub fn game_dir(&self) -> &Path {
        &self.game_dir
    }

    pub fn exe(&self) -> &ExeInfo {
        &self.exe
    }

    /// Every key that was tried: the program file's, then the extra ones.
    pub fn keys(&self) -> &[RsaPublicKey] {
        &self.keys
    }

    /// All `.bhd` files, in name order, opened or not.
    pub fn archives(&self) -> &[ArchiveSlot] {
        &self.slots
    }

    /// The archives that could be opened, in name order.
    pub fn open_archives(&self) -> impl Iterator<Item = &Archive> + '_ {
        self.slots.iter().filter_map(|s| s.archive.as_ref().ok())
    }

    /// Every file whose path hashes like `path` (archives in name order; the 32-bit hash is not unique).
    pub fn lookup(&self, path: &str) -> Vec<Hit<'_>> {
        self.lookup_hash(path_hash(path))
    }

    pub fn lookup_hash(&self, hash: u32) -> Vec<Hit<'_>> {
        self.open_archives().flat_map(|archive| archive.find(hash).map(move |entry| Hit { archive, entry })).collect()
    }

    /// The bytes of one hit.
    pub fn read_hit(&self, hit: &Hit<'_>) -> Result<Vec<u8>, InstallError> {
        Ok(hit.archive.read(hit.entry)?)
    }

    /// The bytes of the first file with this path hash.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, InstallError> {
        let hits = self.lookup(path);
        let first = hits.first().ok_or_else(|| InstallError::NotInArchives { path: path.to_string() })?;
        self.read_hit(first)
    }
}
