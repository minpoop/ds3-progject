//! Which Dark Souls III is this? The typed structures and function addresses of the `darksouls3` crate are only valid
//! for the exact builds it lists, so every feature that touches game memory checks this first (the crate itself would
//! panic on an unknown build).
use core::ffi::c_void;
use pelite::pe64::{Pe, PeView};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameVersion {
    pub product: String,
    /// e.g. "1.15.2.0"
    pub version: String,
    /// Windows primary language id: 0x09 English, 0x11 Japanese
    pub lang: u16,
}

pub fn detect() -> Result<GameVersion, String> {
    unsafe {
        let base = GetModuleHandleW(core::ptr::null()) as *const c_void as *const u8;
        if base.is_null() {
            return Err("the game's main module was not found".into());
        }
        let view = PeView::module(base);
        let resources = view.resources().map_err(|e| format!("no resources in the game's exe: {e}"))?;
        let info = resources.version_info().map_err(|e| format!("no version information in the game's exe: {e}"))?;
        let v = info.fixed().ok_or("the game's exe has no fixed version block")?.dwProductVersion;
        let language = *info.translation().first().ok_or("the game's exe names no language")?;
        let mut product = String::new();
        info.strings(language, |k, val| {
            if k == "ProductName" {
                product = val.to_string();
            }
        });
        Ok(GameVersion { product, version: format!("{}.{}.{}.{}", v.Major, v.Minor, v.Patch, v.Build), lang: language.lang_id & 0x03FF })
    }
}

/// The builds the `darksouls3` crate 0.14 knows: 1.15.2.0 (world-wide, English) and 1.15.2.1 (Japan).
pub fn supported(v: &GameVersion) -> bool {
    matches!((v.lang, v.version.as_str()), (0x09, "1.15.2.0") | (0x11, "1.15.2.1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(lang: u16, version: &str) -> GameVersion {
        GameVersion { product: "DARK SOULS III".into(), version: version.into(), lang }
    }

    #[test]
    fn only_the_listed_builds_are_supported() {
        assert!(supported(&v(0x09, "1.15.2.0")));
        assert!(supported(&v(0x11, "1.15.2.1")));
        assert!(!supported(&v(0x09, "1.15.2.1")));
        assert!(!supported(&v(0x09, "1.15.1.0")));
        assert!(!supported(&v(0x07, "1.15.2.0")));
    }
}
