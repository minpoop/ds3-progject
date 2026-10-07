//! Optional in-game features. EVERYTHING here is optional: if anything is unexpected it logs why and stops, and the
//! sandbox (save redirect + offline guard) keeps working. Nothing in this module may close the game.
mod memscan;
mod probe;
mod version;

use crate::state;
use core::ffi::c_void;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::CreateThread;

/// Called once the sandbox is proven (after the READY line). Starts whatever the config switched on.
pub fn start() {
    let Some(st) = state::get() else { return };
    if !st.cfg.features.probe {
        return;
    }
    st.logger.log("features: read-only game probe is on (private test kit)");
    unsafe {
        let h = CreateThread(core::ptr::null(), 0, Some(probe_thread), core::ptr::null(), 0, core::ptr::null_mut());
        if h.is_null() {
            st.logger.log("features: could not start the probe thread");
        } else {
            CloseHandle(h);
        }
    }
}

unsafe extern "system" fn probe_thread(_: *mut c_void) -> u32 {
    let Some(st) = state::get() else { return 0 };
    let cfg = st.cfg.clone();
    // a panic must never unwind into the game: catch it here, log it, and leave the game running
    if let Err(p) = std::panic::catch_unwind(move || probe::thread_body(cfg)) {
        let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into());
        st.logger.log(&format!("features: the probe thread crashed ({msg}); the game keeps running"));
    }
    0
}
