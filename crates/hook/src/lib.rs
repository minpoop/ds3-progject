//! Ashen Marine hook DLL. ModEngine2 loads it into Dark Souls III (`external_dlls`). It:
//!  * redirects every file operation under the game's real save folder to the mashup's private copy,
//!  * refuses every non-loopback network destination (the game stays offline),
//!  * closes the game instead of running unprotected if it cannot do either.
//!
//! The hooks are generated from `design/sheets/hooks.json` (`generated.rs`); the logic they call is in
//! `guard.rs`; hand-written special cases are in `custom.rs`.
#![cfg(windows)]
#![allow(clippy::missing_safety_doc)]

mod custom;
mod generated;
mod guard;
mod init;
mod selftest;
mod state;

use core::ffi::c_void;
use windows_sys::Win32::Foundation::{CloseHandle, HINSTANCE};
use windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::CreateThread;

#[no_mangle]
pub unsafe extern "system" fn DllMain(hinst: HINSTANCE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        DisableThreadLibraryCalls(hinst);
        state::MODULE.store(hinst, core::sync::atomic::Ordering::SeqCst);
        // Never do real work under the loader lock: hand over to a thread.
        let h = CreateThread(core::ptr::null(), 0, Some(init::thread_main), core::ptr::null(), 0, core::ptr::null_mut());
        if !h.is_null() {
            CloseHandle(h);
        }
    }
    1
}
