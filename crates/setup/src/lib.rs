//! Ashen Marine setup step: reads the player's own Space Marine 2 install (read-only).
//! `probe` looks around and writes a report; `prepare` turns the game's sound events into plain `.wav` files.
//! The parts both need (finding the game, opening its archives) live here.
pub mod prepare;
pub mod probe;
pub mod report;

use crate::report::Report;
use anyhow::{anyhow, bail, Result};
use ashen_common::{steam, SM2_APP_ID};
use ashen_sm2::pak::{self, PakSet};
use std::path::{Path, PathBuf};

/// The folder the program lives in. Everything it writes goes next to it unless told otherwise.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

/// The text file next to the program where the player can name the Space Marine 2 folder when Steam does not know it.
pub fn hint_file() -> PathBuf {
    exe_dir().join("sm2-folder.txt")
}

/// The folder on the first line of `sm2-folder.txt` in `dir` (quotes around it are fine), if there is such a file.
pub fn sm2_hint(dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dir.join("sm2-folder.txt")).ok()?;
    let line = text.lines().next().unwrap_or("").trim_start_matches('\u{feff}').trim().trim_matches('"').trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// Where Space Marine 2 is, and Steam's build id if that is known: the folder the player gave, else the one Steam
/// reports. A given folder inside a Steam library still gets its build id from the library's `appmanifest` file.
pub fn find_sm2(given: Option<&Path>) -> Result<(PathBuf, Option<String>)> {
    match given {
        Some(p) => Ok((p.to_path_buf(), build_id_beside(p))),
        None => steam::locate(SM2_APP_ID).ok_or_else(|| {
            anyhow!(
                "Space Marine 2 was not found through Steam. If it is installed somewhere else, put its folder on the first line of the text file  {}  \
                 (for example D:\\Games\\Space Marine 2) and start this again. Or start this program with  --sm2 \"<its folder>\"",
                hint_file().display()
            )
        }),
    }
}

/// Steam's build id from `steamapps/appmanifest_<id>.acf`, when the game folder is `steamapps/common/<name>`.
fn build_id_beside(folder: &Path) -> Option<String> {
    let steamapps = folder.parent()?.parent()?;
    let text = std::fs::read_to_string(steamapps.join(format!("appmanifest_{SM2_APP_ID}.acf"))).ok()?;
    steam::buildid_from_acf(&text)
}

/// Open every `.pak` of the install, read-only. They are all zip files; one that cannot be opened is reported and skipped.
pub fn open_paks(rep: &mut Report, root: &Path) -> Result<PakSet> {
    let mut dir = None;
    for rel in ["client_pc/root/paks/client", "client_pc/root/paks"] {
        let p = rel.split('/').fold(root.to_path_buf(), |p, part| p.join(part)); // one kind of slash in what the player reads
        if p.is_dir() && pak::list_paks(&p).map(|v| !v.is_empty()).unwrap_or(false) {
            dir = Some(p);
            break;
        }
    }
    let Some(dir) = dir else { bail!("no .pak files found below {}\\client_pc\\root\\paks", root.display()) };
    let files = pak::list_paks(&dir)?;
    rep.say(format!("  {} archives below {}", files.len(), dir.display()));
    let mut set = PakSet::new();
    let total: u64 = files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
    rep.say(format!("  together {}", human(total)));
    for (i, f) in files.iter().enumerate() {
        if let Err(e) = set.add(f) {
            set.skipped.push((f.clone(), format!("{e:#}")));
        }
        if (i + 1) % 10 == 0 || i + 1 == files.len() {
            rep.progress(&format!("    opened {}/{} ({:.0}s)", i + 1, files.len(), rep.elapsed()));
        }
    }
    rep.say(format!("  opened {} archives, {} names in total, {} duplicate names", set.pak_count(), set.entry_count(), set.duplicate_names));
    for (p, why) in &set.skipped {
        rep.say(format!("  SKIPPED {}: {why}", p.display()));
    }
    Ok(set)
}

pub fn human(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

/// Identifier-like words of a text (letters, digits, underscore), 5..=80 characters.
pub fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| (5..=80).contains(&w.len()) && w.bytes().any(|b| b.is_ascii_alphabetic()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hint_file_names_the_folder_on_its_first_line() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(sm2_hint(t.path()), None, "no file");
        let f = t.path().join("sm2-folder.txt");
        std::fs::write(&f, "").unwrap();
        assert_eq!(sm2_hint(t.path()), None, "empty file");
        std::fs::write(&f, "\"D:\\Games\\Space Marine 2\"\r\nthis second line is ignored\r\n").unwrap();
        assert_eq!(sm2_hint(t.path()), Some(PathBuf::from("D:\\Games\\Space Marine 2")));
        std::fs::write(&f, "\u{feff}  /games/sm2  \n").unwrap();
        assert_eq!(sm2_hint(t.path()), Some(PathBuf::from("/games/sm2")), "a byte order mark and spaces are fine");
        std::fs::write(&f, "\n/games/sm2\n").unwrap();
        assert_eq!(sm2_hint(t.path()), None, "only the first line counts");
    }

    #[test]
    fn a_given_folder_is_used_as_it_is_and_the_build_id_comes_from_the_steam_manifest() {
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("steamapps/common/Space Marine 2");
        std::fs::create_dir_all(&game).unwrap();
        assert_eq!(find_sm2(Some(&game)).unwrap(), (game.clone(), None), "no manifest, no build id");
        std::fs::write(t.path().join(format!("steamapps/appmanifest_{SM2_APP_ID}.acf")), "\"AppState\"\n{\n\"installdir\"\t\"Space Marine 2\"\n\"buildid\"\t\"25098992\"\n}").unwrap();
        assert_eq!(find_sm2(Some(&game)).unwrap(), (game, Some("25098992".to_string())));
    }

    #[test]
    fn sizes_and_words() {
        assert_eq!(human(10), "10 B");
        assert_eq!(human(1536), "1.5 KB");
        assert_eq!(words("a wpn_melee_x, 12345 ab; Foo.Bar_baz").collect::<Vec<_>>(), vec!["wpn_melee_x", "Bar_baz"]);
    }
}
