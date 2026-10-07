//! The few things that differ per OS. On Windows they use the real APIs; elsewhere they are inert stubs so the
//! launcher still compiles (and its logic can be unit-tested) off Windows.

#[cfg(windows)]
mod imp {
    use core::ffi::c_void;
    use std::path::PathBuf;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::Shell::{FOLDERID_RoamingAppData, SHGetKnownFolderPath};
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// `%APPDATA%` as Windows itself reports it (honours redirected known folders).
    pub fn known_appdata() -> Option<PathBuf> {
        unsafe {
            let mut p: *mut u16 = core::ptr::null_mut();
            let hr = SHGetKnownFolderPath(&FOLDERID_RoamingAppData, 0, core::ptr::null_mut(), &mut p);
            if hr != 0 || p.is_null() {
                return std::env::var_os("APPDATA").map(PathBuf::from);
            }
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(core::slice::from_raw_parts(p, n));
            CoTaskMemFree(p as *const c_void);
            Some(PathBuf::from(s))
        }
    }

    /// Is a process with this executable name running?
    pub fn process_running(exe: &str) -> bool {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap as isize == -1 {
                return false;
            }
            let mut entry: PROCESSENTRY32W = core::mem::zeroed();
            entry.dwSize = core::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = false;
            let mut ok = Process32FirstW(snap, &mut entry);
            while ok != 0 {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(exe) {
                    found = true;
                    break;
                }
                ok = Process32NextW(snap, &mut entry);
            }
            CloseHandle(snap);
            found
        }
    }

    /// Only one launcher at a time per user. Returns false if another one already holds the lock.
    /// The handle is deliberately never closed: the lock lasts until this process exits.
    pub fn single_instance() -> bool {
        unsafe {
            let h = CreateMutexW(core::ptr::null(), 1, wide("Local\\AshenMarineLauncher").as_ptr());
            if h.is_null() {
                return true; // cannot tell: do not block the player
            }
            GetLastError() != 183 // ERROR_ALREADY_EXISTS
        }
    }

    pub fn message_box(title: &str, text: &str, error: bool) {
        unsafe {
            MessageBoxW(core::ptr::null_mut(), wide(text).as_ptr(), wide(title).as_ptr(), MB_OK | if error { MB_ICONERROR } else { MB_ICONINFORMATION });
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::PathBuf;

    pub fn known_appdata() -> Option<PathBuf> {
        None
    }
    pub fn process_running(_exe: &str) -> bool {
        false
    }
    pub fn single_instance() -> bool {
        true
    }
    pub fn message_box(title: &str, text: &str, _error: bool) {
        eprintln!("[{title}] {text}");
    }
}

pub use imp::*;
