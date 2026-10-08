//! Optional in-game features. EVERYTHING here is optional: if anything is unexpected it logs why and stops, and the
//! sandbox (save redirect + offline guard) keeps working. Nothing in this module may close the game.
mod audio;
mod equip;
mod gametask;
mod input;
mod keydump;
mod memscan;
mod probe;
mod sfx;
mod version;

use crate::state;
use ashen_common::config::HookConfig;
use core::ffi::c_void;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::CreateThread;

/// Called once the sandbox is proven (after the READY line). Starts whatever the config switched on.
pub fn start() {
    let Some(st) = state::get() else { return };
    if st.cfg.features.probe {
        st.logger.log("features: read-only game probe is on (private test kit)");
        spawn(&st.logger, "probe", probe_thread);
    }
    if st.cfg.features.sounds {
        st.logger.log("features: Space Marine 2 sounds are on");
        spawn(&st.logger, "sounds", sfx_thread);
    }
}

fn spawn(log: &ashen_common::logging::Logger, what: &str, body: unsafe extern "system" fn(*mut c_void) -> u32) {
    unsafe {
        let h = CreateThread(core::ptr::null(), 0, Some(body), core::ptr::null(), 0, core::ptr::null_mut());
        if h.is_null() {
            log.log(&format!("features: could not start the {what} thread"));
        } else {
            CloseHandle(h);
        }
    }
}

/// Run `body` with a copy of the config; a panic must never unwind into the game: catch it here, log it, and leave the
/// game running.
fn guarded(what: &str, body: impl FnOnce(HookConfig) + std::panic::UnwindSafe) -> u32 {
    let Some(st) = state::get() else { return 0 };
    let cfg = st.cfg.clone();
    if let Err(p) = std::panic::catch_unwind(move || body(cfg)) {
        let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into());
        st.logger.log(&format!("features: the {what} thread crashed ({msg}); the game keeps running"));
    }
    0
}

unsafe extern "system" fn probe_thread(_: *mut c_void) -> u32 {
    guarded("probe", probe::thread_body)
}

unsafe extern "system" fn sfx_thread(_: *mut c_void) -> u32 {
    guarded("sounds", sfx::thread_body)
}
