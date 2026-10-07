//! Path tokens used in the design sheets (`{appdata}`, `{game}`, `{me2}`, `{data}`, `{sm2}`) and the
//! file rules generated from `design/sheets/files.json`.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    ReadWrite,
}

/// One row of the files sheet.
#[derive(Clone, Copy, Debug)]
pub struct FileRule {
    pub id: &'static str,
    pub path: &'static str,
    pub purpose: &'static str,
    pub access: Access,
    pub system: &'static str,
    pub guarded: bool,
    pub milestone: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Roots {
    pub appdata: Option<PathBuf>,
    pub game: Option<PathBuf>,
    pub me2: Option<PathBuf>,
    pub data: Option<PathBuf>,
    pub sm2: Option<PathBuf>,
}

impl Roots {
    /// Expand a sheet path such as `{data}/logs/hook.log`.
    pub fn expand(&self, template: &str) -> Result<PathBuf, String> {
        let end = match (template.starts_with('{'), template.find('}')) {
            (true, Some(e)) => e,
            _ => return Err(format!("path {template:?} does not start with a {{token}}")),
        };
        let token = &template[1..end];
        let rest = template[end + 1..].trim_start_matches(['/', '\\']);
        let root = match token {
            "appdata" => &self.appdata,
            "game" => &self.game,
            "me2" => &self.me2,
            "data" => &self.data,
            "sm2" => &self.sm2,
            other => return Err(format!("unknown path token {{{other}}} in {template:?}")),
        };
        let root = root.as_ref().ok_or_else(|| format!("root {{{token}}} is not known (needed by {template:?})"))?;
        Ok(if rest.is_empty() { root.clone() } else { root.join(rest) })
    }

    /// Resolve a row of the files sheet by its id.
    pub fn file(&self, id: &str) -> Result<PathBuf, String> {
        let rule = crate::generated::files::FILES.iter().find(|r| r.id == id).ok_or_else(|| format!("no files-sheet row with id {id:?}"))?;
        self.expand(rule.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::files::{self, FILES};

    fn roots() -> Roots {
        Roots {
            appdata: Some("/ad".into()),
            game: Some("/game".into()),
            me2: Some("/me2".into()),
            data: Some("/data".into()),
            sm2: None,
        }
    }

    #[test]
    fn expands_tokens() {
        let r = roots();
        assert_eq!(r.expand("{data}/logs/hook.log").unwrap(), PathBuf::from("/data/logs/hook.log"));
        assert_eq!(r.expand("{appdata}/DarkSoulsIII").unwrap(), PathBuf::from("/ad/DarkSoulsIII"));
        assert_eq!(r.expand("{data}").unwrap(), PathBuf::from("/data"));
        assert!(r.expand("{sm2}/x").is_err(), "unset root is an error, not a silent empty path");
        assert!(r.expand("{nope}/x").is_err());
        assert!(r.expand("plain/path").is_err());
    }

    #[test]
    fn every_milestone_one_file_row_resolves() {
        let r = roots();
        for f in FILES.iter().filter(|f| f.milestone <= 1) {
            r.expand(f.path).unwrap_or_else(|e| panic!("{}: {e}", f.id));
        }
        assert_eq!(r.file(files::HOOK_LOG).unwrap(), PathBuf::from("/data/logs/hook.log"));
    }

    #[test]
    fn guarded_files_are_never_writable() {
        for f in FILES {
            assert!(!(f.guarded && f.access != Access::Read), "{} is guarded but not read-only", f.id);
        }
    }
}
