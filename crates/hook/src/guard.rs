//! The logic the generated detours call (see `generated.rs`). Kept small and allocation-light: these run on
//! the game's own threads, inside file and network calls.

use crate::state::{self, ARCHIVE_OPENS, DENIED_ADDR, DENIED_NAME, REDIRECTED};
use ashen_common::netguard;
use core::sync::atomic::Ordering::Relaxed;
use std::cell::Cell;

thread_local! {
    /// Set while a hook is working, so anything it calls (logging) cannot recurse into another hook.
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

// ---------------------------------------------------------------------------------------- last error

pub fn set_last_error_wsa(code: i32) {
    unsafe { windows_sys::Win32::Networking::WinSock::WSASetLastError(code) }
}

pub fn set_last_error_win32(code: u32) {
    unsafe { windows_sys::Win32::Foundation::SetLastError(code) }
}

// ---------------------------------------------------------------------------------------- reading strings

const MAX_UNITS: usize = 32_768;

unsafe fn wide_len(p: *const u16) -> usize {
    let mut n = 0;
    while n < MAX_UNITS && *p.add(n) != 0 {
        n += 1;
    }
    n
}

unsafe fn read_wide(p: *const u16) -> String {
    String::from_utf16_lossy(core::slice::from_raw_parts(p, wide_len(p)))
}

unsafe fn read_ansi(p: *const u8) -> String {
    let mut n = 0;
    while n < 1024 && *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf8_lossy(core::slice::from_raw_parts(p, n)).into_owned()
}

// ---------------------------------------------------------------------------------------- file redirect

/// The file name of a path that ends in `.bhd` or `.bdt` (any case), else `None`. No allocation unless it is one: this runs
/// for every file the game opens.
pub fn archive_leaf(units: &[u16]) -> Option<String> {
    const BHD: [u16; 4] = [b'.' as u16, b'b' as u16, b'h' as u16, b'd' as u16];
    const BDT: [u16; 4] = [b'.' as u16, b'b' as u16, b'd' as u16, b't' as u16];
    let lower = |u: u16| if (b'A' as u16..=b'Z' as u16).contains(&u) { u + 32 } else { u };
    let last = units.get(units.len().checked_sub(4)?..)?;
    let tail = [lower(*last.first()?), lower(*last.get(1)?), lower(*last.get(2)?), lower(*last.get(3)?)];
    if tail != BHD && tail != BDT {
        return None;
    }
    let start = units.iter().rposition(|u| *u == b'\\' as u16 || *u == b'/' as u16).map_or(0, |i| i + 1);
    Some(String::from_utf16_lossy(units.get(start..)?))
}

/// The first time the game opens an archive file, say when (relative to the hook loading): it tells whether the keys and
/// tables of contents the collector looks for can still be in memory by then.
fn note_archive_open(st: &state::State, units: &[u16], id: &'static str) {
    use std::sync::Mutex;
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let Some(leaf) = archive_leaf(units) else { return };
    let Ok(mut seen) = SEEN.lock() else { return };
    let lower = leaf.to_lowercase();
    if seen.contains(&lower) {
        return;
    }
    seen.push(lower);
    ARCHIVE_OPENS.fetch_add(1, Relaxed);
    let t = state::T0.get().map_or(0, |t| t.elapsed().as_millis());
    st.logger.log(&format!("ARCHIVE opened by the game [{id}]: {leaf} (+{t} ms after the hook loaded; {} different archive files so far)", seen.len()));
}

/// If the path points into the game's real save folder, returns the NUL-terminated path inside the private copy.
pub unsafe fn redirect_wide(p: *const u16, id: &'static str) -> Option<Vec<u16>> {
    if p.is_null() {
        return None;
    }
    let st = state::get()?;
    if BUSY.with(|b| b.replace(true)) {
        return None;
    }
    let n = wide_len(p);
    let result = if n == MAX_UNITS {
        None
    } else {
        let units = core::slice::from_raw_parts(p, n);
        note_archive_open(st, units, id);
        st.redirector.redirect(units).map(|mut out| {
            let k = REDIRECTED.fetch_add(1, Relaxed) + 1;
            if k <= 60 || k % 500 == 0 {
                st.logger.log(&format!("REDIRECT #{k} [{id}] {} -> {}", String::from_utf16_lossy(units), String::from_utf16_lossy(&out)));
            }
            out.push(0);
            out
        })
    };
    BUSY.with(|b| b.set(false));
    result
}

/// The pointer to hand to the real function: the redirected buffer if there is one, else the original.
pub fn ptr_or(r: &Option<Vec<u16>>, orig: *const u16) -> *const u16 {
    match r {
        Some(v) => v.as_ptr(),
        None => orig,
    }
}

// ---------------------------------------------------------------------------------------- network

pub unsafe fn describe_sockaddr(p: *const u8) -> String {
    if p.is_null() {
        return "(null)".into();
    }
    let family = core::ptr::read_unaligned(p as *const u16);
    match family {
        netguard::AF_INET => {
            let port = u16::from_be_bytes([*p.add(2), *p.add(3)]);
            format!("{}.{}.{}.{}:{}", *p.add(4), *p.add(5), *p.add(6), *p.add(7), port)
        }
        netguard::AF_INET6 => {
            let port = u16::from_be_bytes([*p.add(2), *p.add(3)]);
            let mut s = String::from("[");
            for i in 0..8 {
                if i > 0 {
                    s.push(':');
                }
                s.push_str(&format!("{:x}", u16::from_be_bytes([*p.add(8 + i * 2), *p.add(9 + i * 2)])));
            }
            format!("{s}]:{port}")
        }
        f => format!("(family {f})"),
    }
}

/// True when this socket address must be refused (anything that is not loopback). Logs and counts denials.
pub unsafe fn addr_denied(id: &'static str, p: *const u8) -> bool {
    let st = match state::get() {
        Some(s) => s,
        None => return false,
    };
    if !st.cfg.block_network || !netguard::sockaddr_denied_ptr(p) {
        return false;
    }
    if BUSY.with(|b| b.replace(true)) {
        return true;
    }
    let n = DENIED_ADDR.fetch_add(1, Relaxed) + 1;
    if n <= 200 || n % 100 == 0 {
        st.logger.log(&format!("DENY #{n} [{id}] address {}", describe_sockaddr(p)));
    }
    BUSY.with(|b| b.set(false));
    true
}

fn name_denied(id: &'static str, name: String) -> bool {
    let st = match state::get() {
        Some(s) => s,
        None => return false,
    };
    if !st.cfg.block_network || netguard::name_is_allowed(&name) {
        return false;
    }
    if BUSY.with(|b| b.replace(true)) {
        return true;
    }
    let n = DENIED_NAME.fetch_add(1, Relaxed) + 1;
    if n <= 200 || n % 100 == 0 {
        st.logger.log(&format!("DENY #{n} [{id}] host name {name:?}"));
    }
    BUSY.with(|b| b.set(false));
    true
}

pub unsafe fn name_denied_utf16(id: &'static str, p: *const u16) -> bool {
    if p.is_null() {
        return false;
    }
    name_denied(id, read_wide(p))
}

pub unsafe fn name_denied_ansi(id: &'static str, p: *const u8) -> bool {
    if p.is_null() {
        return false;
    }
    name_denied(id, read_ansi(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn only_archive_files_are_recognised_and_by_their_last_part() {
        assert_eq!(archive_leaf(&w(r"I:\SteamLibrary\steamapps\common\DARK SOULS III\Game\Data1.bhd")).as_deref(), Some("Data1.bhd"));
        assert_eq!(archive_leaf(&w("C:/Games/DS3/Game/DLC2.BDT")).as_deref(), Some("DLC2.BDT"));
        assert_eq!(archive_leaf(&w("Data0.bhd")).as_deref(), Some("Data0.bhd"));
        for other in ["", "bhd", ".bhd.", r"C:\x\Data0.bhd.bak", r"C:\x\DarkSoulsIII.exe", r"C:\Users\me\AppData\Roaming\DarkSoulsIII\DS30000.sl2", "bdt"] {
            assert_eq!(archive_leaf(&w(other)), None, "{other:?}");
        }
    }
}
