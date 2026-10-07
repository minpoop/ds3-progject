//! Path redirection: send everything under the game's real save folder to the mashup's private copy.
//!
//! Works on UTF-16 code units so it can run inside file-API hooks: the common case ("not our folder")
//! allocates nothing. Comparison is case-insensitive, treats `/` as `\`, understands the `\\?\` and
//! `\??\` prefixes, and only matches on a path-component boundary (`DarkSoulsIII2` is not a match for
//! `DarkSoulsIII`).

#[derive(Clone, Debug)]
pub struct Redirector {
    real_fold: Vec<u16>,
    sandbox: Vec<u16>,
}

const BACKSLASH: u16 = b'\\' as u16;
const SLASH: u16 = b'/' as u16;

fn is_sep(u: u16) -> bool {
    u == BACKSLASH || u == SLASH
}

fn fold(u: u16) -> u16 {
    if u < 128 {
        let b = u as u8;
        return b.to_ascii_lowercase() as u16;
    }
    match char::from_u32(u as u32) {
        Some(c) => {
            let mut it = c.to_lowercase();
            match (it.next(), it.next()) {
                (Some(l), None) if (l as u32) < 0x10000 => l as u16,
                _ => u,
            }
        }
        None => u, // lone surrogate: compare as is
    }
}

/// Normalise a directory: backslashes, no `\\?\` prefix, no trailing separator.
fn normalise_dir(dir: &str) -> Vec<u16> {
    let mut v: Vec<u16> = dir.encode_utf16().map(|u| if u == SLASH { BACKSLASH } else { u }).collect();
    let (pfx, _) = split_prefix(&v);
    let n = pfx.len();
    v.drain(..n);
    while v.last() == Some(&BACKSLASH) {
        v.pop();
    }
    v
}

/// Returns (prefix, rest) where prefix is `\\?\` or `\??\` if present.
fn split_prefix(p: &[u16]) -> (&[u16], &[u16]) {
    if p.len() >= 4 {
        let a = (p[0] == BACKSLASH || p[0] == SLASH) && (p[1] == BACKSLASH || p[1] == SLASH || p[1] == b'?' as u16);
        let b = p[2] == b'?' as u16 && is_sep(p[3]);
        let c = p[1] == b'?' as u16 && p[2] == b'?' as u16 && is_sep(p[3]);
        if (a && b) || (p[0] == BACKSLASH && c) {
            return p.split_at(4);
        }
    }
    (&p[..0], p)
}

impl Redirector {
    pub fn new(real_dir: &str, sandbox_dir: &str) -> Self {
        let real = normalise_dir(real_dir);
        let real_fold = real.iter().map(|&u| fold(u)).collect();
        Redirector { real_fold, sandbox: normalise_dir(sandbox_dir) }
    }

    /// If `path` is the real folder or inside it, returns the same path inside the private copy.
    pub fn redirect(&self, path: &[u16]) -> Option<Vec<u16>> {
        let (pfx, rest) = split_prefix(path);
        let n = self.real_fold.len();
        if n == 0 || rest.len() < n {
            return None;
        }
        for i in 0..n {
            let c = rest[i];
            let c = if c == SLASH { BACKSLASH } else { fold(c) };
            if c != self.real_fold[i] {
                return None;
            }
        }
        let tail = &rest[n..];
        if !(tail.is_empty() || is_sep(tail[0])) {
            return None;
        }
        let mut out = Vec::with_capacity(pfx.len() + self.sandbox.len() + tail.len());
        out.extend_from_slice(pfx);
        out.extend_from_slice(&self.sandbox);
        if pfx.is_empty() {
            // Win32 treats `/` as `\` in ordinary paths, so give the private copy one consistent separator.
            out.extend(tail.iter().map(|&u| if u == SLASH { BACKSLASH } else { u }));
        } else {
            // In `\\?\` paths a `/` is a literal character: keep whatever the game wrote.
            out.extend_from_slice(tail);
        }
        Some(out)
    }

    /// Convenience for tests and the launcher.
    pub fn redirect_str(&self, s: &str) -> Option<String> {
        let w: Vec<u16> = s.encode_utf16().collect();
        self.redirect(&w).map(|v| String::from_utf16_lossy(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> Redirector {
        Redirector::new(r"C:\Users\Ash\AppData\Roaming\DarkSoulsIII", r"D:\Melty\managed\ashenmarine\save\DarkSoulsIII")
    }

    #[test]
    fn redirects_files_and_the_folder_itself() {
        let r = r();
        assert_eq!(
            r.redirect_str(r"C:\Users\Ash\AppData\Roaming\DarkSoulsIII\0110000100000001\DS30000.sl2").unwrap(),
            r"D:\Melty\managed\ashenmarine\save\DarkSoulsIII\0110000100000001\DS30000.sl2"
        );
        assert_eq!(r.redirect_str(r"C:\Users\Ash\AppData\Roaming\DarkSoulsIII").unwrap(), r"D:\Melty\managed\ashenmarine\save\DarkSoulsIII");
        assert_eq!(r.redirect_str(r"C:\Users\Ash\AppData\Roaming\DarkSoulsIII\").unwrap(), r"D:\Melty\managed\ashenmarine\save\DarkSoulsIII\");
    }

    #[test]
    fn ignores_everything_else() {
        let r = r();
        assert!(r.redirect_str(r"C:\Users\Ash\AppData\Roaming\DarkSoulsIII2\x").is_none(), "component boundary");
        assert!(r.redirect_str(r"C:\Users\Ash\AppData\Roaming\Other\DarkSoulsIII\x").is_none());
        assert!(r.redirect_str(r"C:\Users\Ash\AppData\Roaming").is_none(), "parent folder");
        assert!(r.redirect_str(r"C:\Program Files (x86)\Steam\steamapps\common\DARK SOULS III\Game\Data0.bdt").is_none());
        assert!(r.redirect_str("DS30000.sl2").is_none(), "relative");
        assert!(r.redirect_str("").is_none());
    }

    #[test]
    fn case_slashes_and_prefixes() {
        let r = r();
        assert_eq!(
            r.redirect_str("c:/users/ASH/appdata/roaming/darksoulsiii/GraphicsConfig.xml").unwrap(),
            r"D:\Melty\managed\ashenmarine\save\DarkSoulsIII\GraphicsConfig.xml"
        );
        assert_eq!(
            r.redirect_str(r"\\?\C:\Users\Ash\AppData\Roaming\DarkSoulsIII\a.bak").unwrap(),
            r"\\?\D:\Melty\managed\ashenmarine\save\DarkSoulsIII\a.bak",
            "extended-length prefix is kept"
        );
        assert_eq!(
            r.redirect_str(r"\??\C:\Users\Ash\AppData\Roaming\DarkSoulsIII\a").unwrap(),
            r"\??\D:\Melty\managed\ashenmarine\save\DarkSoulsIII\a"
        );
    }

    #[test]
    fn config_dirs_may_have_trailing_slashes_and_prefixes() {
        let r = Redirector::new(r"\\?\C:\Users\Ash\AppData\Roaming\DarkSoulsIII\", r"D:/Melty/save/DarkSoulsIII/");
        assert_eq!(r.redirect_str(r"C:\Users\Ash\AppData\Roaming\DarkSoulsIII\x.sl2").unwrap(), r"D:\Melty\save\DarkSoulsIII\x.sl2");
    }

    #[test]
    fn non_ascii_user_names() {
        let r = Redirector::new(r"C:\Users\Jürgen\AppData\Roaming\DarkSoulsIII", r"D:\s");
        assert!(r.redirect_str(r"C:\USERS\JÜRGEN\AppData\Roaming\DarkSoulsIII\x").is_some());
        assert!(r.redirect_str(r"C:\Users\Jurgen\AppData\Roaming\DarkSoulsIII\x").is_none());
    }

    #[test]
    fn empty_config_never_matches() {
        let r = Redirector::new("", r"D:\s");
        assert!(r.redirect_str(r"C:\anything").is_none());
    }
}
