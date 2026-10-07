//! Reading Steam's own files to find an installed game: `libraryfolders.vdf` and `appmanifest_<id>.acf`.
//! Pure text parsing so it is testable anywhere; finding Steam's root (the registry) is the caller's job.

use std::fs;
use std::path::{Path, PathBuf};

/// Value of the first `"key"  "value"` pair for `key` at or after `from`. Unescapes `\\` and `\"`.
fn quoted_value(text: &str, key: &str, from: usize) -> Option<(String, usize)> {
    let needle = format!("\"{}\"", key);
    let at = text[from..].find(&needle)? + from + needle.len();
    let rest = &text[at..];
    let open = rest.find('"')? + 1;
    let mut out = String::new();
    let mut chars = rest[open..].char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some((_, n)) => out.push(n),
                None => break,
            },
            '"' => return Some((out, at + open + i + 1)),
            _ => out.push(c),
        }
    }
    None
}

/// All `"path"` entries of `libraryfolders.vdf`.
pub fn library_paths_from_vdf(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut from = 0;
    while let Some((v, next)) = quoted_value(text, "path", from) {
        out.push(v);
        from = next;
    }
    out
}

pub fn installdir_from_acf(text: &str) -> Option<String> {
    quoted_value(text, "installdir", 0).map(|(v, _)| v)
}

pub fn buildid_from_acf(text: &str) -> Option<String> {
    quoted_value(text, "buildid", 0).map(|(v, _)| v)
}

/// Every Steam library: Steam's own root plus those listed in `libraryfolders.vdf`.
pub fn libraries(steam_root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![steam_root.to_path_buf()];
    if let Ok(text) = fs::read_to_string(steam_root.join("steamapps").join("libraryfolders.vdf")) {
        for p in library_paths_from_vdf(&text) {
            let pb = PathBuf::from(p);
            if !libs.contains(&pb) {
                libs.push(pb);
            }
        }
    }
    libs
}

/// The install folder of a Steam app, if it is installed in one of the libraries.
pub fn find_game(libs: &[PathBuf], app_id: u32) -> Option<(PathBuf, Option<String>)> {
    for lib in libs {
        let acf = lib.join("steamapps").join(format!("appmanifest_{app_id}.acf"));
        if let Ok(text) = fs::read_to_string(&acf) {
            if let Some(dir) = installdir_from_acf(&text) {
                let path = lib.join("steamapps").join("common").join(dir);
                if path.is_dir() {
                    return Some((path, buildid_from_acf(&text)));
                }
            }
        }
    }
    None
}

/// Steam's install folder from the registry (Windows only; `None` elsewhere or if Steam is not installed).
#[cfg(windows)]
pub fn registry_root() -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    unsafe {
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let rc = RegGetValueW(
            HKEY_CURRENT_USER,
            wide("Software\\Valve\\Steam").as_ptr(),
            wide("SteamPath").as_ptr(),
            RRF_RT_REG_SZ,
            core::ptr::null_mut(),
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            &mut size,
        );
        if rc != 0 {
            return None;
        }
        let n = (size as usize / 2).saturating_sub(1).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..n]);
        if s.is_empty() {
            None
        } else {
            Some(PathBuf::from(s.replace('/', "\\")))
        }
    }
}

#[cfg(not(windows))]
pub fn registry_root() -> Option<PathBuf> {
    None
}

/// Where Steam may live: the registry answer first, then the usual Program Files folders.
pub fn candidate_roots(registry: Option<PathBuf>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = registry.into_iter().collect();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(pf) = std::env::var_os(var) {
            let p = PathBuf::from(pf).join("Steam");
            if !roots.contains(&p) {
                roots.push(p);
            }
        }
    }
    roots
}

/// Find an installed Steam game by app id across every Steam library. Returns its folder and Steam's build id.
pub fn locate(app_id: u32) -> Option<(PathBuf, Option<String>)> {
    for root in candidate_roots(registry_root()) {
        if let Some(found) = find_game(&libraries(&root), app_id) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const VDF: &str = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"374320"		"52000000000"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"apps"
		{
			"2183900"		"75000000000"
		}
	}
}"#;

    #[test]
    fn parses_library_paths() {
        assert_eq!(library_paths_from_vdf(VDF), vec![r"C:\Program Files (x86)\Steam", r"D:\SteamLibrary"]);
    }

    #[test]
    fn parses_acf() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"2183900\"\n\t\"installdir\"\t\t\"Space Marine 2\"\n\t\"buildid\"\t\t\"15432198\"\n}";
        assert_eq!(installdir_from_acf(acf).as_deref(), Some("Space Marine 2"));
        assert_eq!(buildid_from_acf(acf).as_deref(), Some("15432198"));
        assert_eq!(installdir_from_acf("nothing here"), None);
    }

    #[test]
    fn finds_a_game_in_a_second_library() {
        let t = tempfile::tempdir().unwrap();
        let steam = t.path().join("Steam");
        let lib2 = t.path().join("Lib2");
        fs::create_dir_all(steam.join("steamapps")).unwrap();
        fs::create_dir_all(lib2.join("steamapps/common/DARK SOULS III")).unwrap();
        let vdf = format!("\"libraryfolders\"\n{{\n\"0\"\n{{\n\"path\"\t\"{}\"\n}}\n\"1\"\n{{\n\"path\"\t\"{}\"\n}}\n}}", steam.to_string_lossy().replace('\\', "\\\\"), lib2.to_string_lossy().replace('\\', "\\\\"));
        fs::write(steam.join("steamapps/libraryfolders.vdf"), vdf).unwrap();
        fs::write(lib2.join("steamapps/appmanifest_374320.acf"), "\"AppState\"\n{\n\"installdir\"\t\"DARK SOULS III\"\n\"buildid\"\t\"123\"\n}").unwrap();
        let libs = libraries(&steam);
        assert_eq!(libs.len(), 2);
        let (path, build) = find_game(&libs, 374320).unwrap();
        assert!(path.ends_with("DARK SOULS III"));
        assert_eq!(build.as_deref(), Some("123"));
        assert!(find_game(&libs, 2183900).is_none());
    }
}
