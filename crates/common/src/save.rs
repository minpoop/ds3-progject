//! The player's real Dark Souls III save is sacred. This module copies it, fingerprints it, backs it up and,
//! if anything about it changed during a session, puts it back. It never deletes a file in the real save.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    /// Path relative to the folder, with `/` separators, original case.
    pub rel: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

/// Fingerprint of a folder: lowercase relative path -> file info.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub files: BTreeMap<String, FileInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub modified: Vec<String>,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.modified.is_empty() && self.removed.is_empty() && self.added.is_empty()
    }
}

pub fn sha256_file(path: &Path) -> io::Result<[u8; 32]> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

fn walk(root: &Path, dir: &Path, out: &mut Manifest) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let path = entry.path();
        if ty.is_dir() {
            walk(root, &path, out)?;
        } else if ty.is_file() {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let size = entry.metadata()?.len();
            out.files.insert(rel.to_lowercase(), FileInfo { rel, size, sha256: sha256_file(&path)? });
        }
    }
    Ok(())
}

/// Fingerprint a folder. A folder that does not exist has an empty manifest.
pub fn manifest(dir: &Path) -> io::Result<Manifest> {
    let mut m = Manifest::default();
    if dir.is_dir() {
        walk(dir, dir, &mut m)?;
    }
    Ok(m)
}

pub fn diff(before: &Manifest, after: &Manifest) -> Diff {
    let mut d = Diff::default();
    for (k, b) in &before.files {
        match after.files.get(k) {
            None => d.removed.push(b.rel.clone()),
            Some(a) if a.sha256 != b.sha256 || a.size != b.size => d.modified.push(b.rel.clone()),
            _ => {}
        }
    }
    for (k, a) in &after.files {
        if !before.files.contains_key(k) {
            d.added.push(a.rel.clone());
        }
    }
    d
}

/// Copy a folder tree (never moves). Returns the number of files copied. The destination is created.
pub fn copy_tree(src: &Path, dst: &Path) -> io::Result<u64> {
    fs::create_dir_all(dst)?;
    let mut n = 0;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            n += copy_tree(&entry.path(), &to)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), &to)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Full copy of the real save into `<backups_root>/<tag>`. `None` if there is no save to back up.
pub fn make_backup(real: &Path, backups_root: &Path, tag: &str) -> io::Result<Option<PathBuf>> {
    if !real.is_dir() {
        return Ok(None);
    }
    let dest = backups_root.join(tag);
    if dest.exists() {
        return Ok(Some(dest));
    }
    copy_tree(real, &dest)?;
    Ok(Some(dest))
}

/// Keep only the newest `keep` folders named `<prefix>*` (names sort by time). Other folders are never touched.
pub fn prune_backups(backups_root: &Path, prefix: &str, keep: usize) -> io::Result<Vec<String>> {
    let mut names: Vec<String> = match fs::read_dir(backups_root) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(prefix))
            .collect(),
        Err(_) => return Ok(vec![]),
    };
    names.sort();
    let mut removed = vec![];
    while names.len() > keep {
        let n = names.remove(0);
        fs::remove_dir_all(backups_root.join(&n))?;
        removed.push(n);
    }
    Ok(removed)
}

/// Put back every file that was modified or removed, from the backup. Files that were *added* are left alone
/// (and reported by the caller): this function never deletes anything from the real save.
pub fn restore_changed(backup: &Path, real: &Path, d: &Diff) -> io::Result<Vec<String>> {
    let mut restored = vec![];
    for rel in d.modified.iter().chain(d.removed.iter()) {
        let from = backup.join(rel);
        let to = real.join(rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&from, &to)?;
        restored.push(rel.clone());
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, bytes: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    fn fake_save(root: &Path) {
        write(&root.join("0110000100000001/DS30000.sl2"), b"character data");
        write(&root.join("0110000100000001/DS30000.sl2.bak"), b"older data");
        write(&root.join("GraphicsConfig.xml"), b"<x/>");
    }

    #[test]
    fn manifest_and_diff() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real");
        fake_save(&real);
        let a = manifest(&real).unwrap();
        assert_eq!(a.files.len(), 3);
        assert!(diff(&a, &manifest(&real).unwrap()).is_empty());

        write(&real.join("0110000100000001/DS30000.sl2"), b"changed");
        fs::remove_file(real.join("GraphicsConfig.xml")).unwrap();
        write(&real.join("new.txt"), b"n");
        let d = diff(&a, &manifest(&real).unwrap());
        assert_eq!(d.modified, vec!["0110000100000001/DS30000.sl2"]);
        assert_eq!(d.removed, vec!["GraphicsConfig.xml"]);
        assert_eq!(d.added, vec!["new.txt"]);
    }

    #[test]
    fn missing_folder_is_an_empty_manifest() {
        let t = tempfile::tempdir().unwrap();
        assert!(manifest(&t.path().join("nope")).unwrap().files.is_empty());
    }

    #[test]
    fn copy_never_moves_and_is_exact() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real");
        let sb = t.path().join("sandbox/DarkSoulsIII");
        fake_save(&real);
        let before = manifest(&real).unwrap();
        assert_eq!(copy_tree(&real, &sb).unwrap(), 3);
        assert_eq!(manifest(&real).unwrap(), before, "source untouched");
        assert!(diff(&before, &manifest(&sb).unwrap()).is_empty(), "copy is identical");
    }

    #[test]
    fn backup_restore_roundtrip_never_deletes() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real");
        let backups = t.path().join("backups");
        fake_save(&real);
        let before = manifest(&real).unwrap();
        let b = make_backup(&real, &backups, "session-20261007-000000").unwrap().unwrap();

        // something goes wrong during a session
        write(&real.join("0110000100000001/DS30000.sl2"), b"corrupted");
        fs::remove_file(real.join("GraphicsConfig.xml")).unwrap();
        write(&real.join("stray.txt"), b"stray");

        let d = diff(&before, &manifest(&real).unwrap());
        let restored = restore_changed(&b, &real, &d).unwrap();
        assert_eq!(restored.len(), 2);
        let after = manifest(&real).unwrap();
        let d2 = diff(&before, &after);
        assert!(d2.modified.is_empty() && d2.removed.is_empty(), "everything that existed is back");
        assert_eq!(d2.added, vec!["stray.txt"], "added files are reported, never deleted");
        assert!(real.join("stray.txt").exists());
    }

    #[test]
    fn no_save_means_no_backup() {
        let t = tempfile::tempdir().unwrap();
        assert!(make_backup(&t.path().join("none"), &t.path().join("b"), "x").unwrap().is_none());
    }

    #[test]
    fn prune_keeps_newest_sessions_and_the_original() {
        let t = tempfile::tempdir().unwrap();
        let b = t.path();
        for n in ["original", "session-1", "session-2", "session-3", "session-4", "other"] {
            fs::create_dir_all(b.join(n)).unwrap();
        }
        let removed = prune_backups(b, "session-", 2).unwrap();
        assert_eq!(removed, vec!["session-1", "session-2"]);
        assert!(b.join("original").exists() && b.join("other").exists());
        assert!(b.join("session-3").exists() && b.join("session-4").exists());
    }
}
