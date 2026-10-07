//! Process-wide state of the hook DLL.

use ashen_common::{config::HookConfig, logging::Logger, redirect::Redirector};
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU64};
use std::path::PathBuf;
use std::sync::OnceLock;

pub struct State {
    pub cfg: HookConfig,
    pub redirector: Redirector,
    pub logger: Logger,
}

static STATE: OnceLock<State> = OnceLock::new();

/// Where FATAL.txt goes: set as soon as the config names the log folder, before any validation can fail.
pub static FATAL_PATH: OnceLock<PathBuf> = OnceLock::new();

/// This DLL's module handle, set in DllMain.
pub static MODULE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

pub static REDIRECTED: AtomicU64 = AtomicU64::new(0);
pub static DENIED_ADDR: AtomicU64 = AtomicU64::new(0);
pub static DENIED_NAME: AtomicU64 = AtomicU64::new(0);

pub fn set(state: State) -> Result<(), String> {
    STATE.set(state).map_err(|_| "hook state was already initialised".to_string())
}

/// `None` until the config is loaded. Hooks are installed only after this is set.
pub fn get() -> Option<&'static State> {
    STATE.get()
}
