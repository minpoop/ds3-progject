//! Start-up of the hook DLL: load the config, install every hook from the sheet, prove they work, or close
//! the game. Runs on its own thread (never under the loader lock).

use crate::generated::hook_specs;
use crate::state::{self, State, DENIED_ADDR, DENIED_NAME, MODULE, REDIRECTED};
use ashen_common::{config::HookConfig, logging::Logger, redirect::Redirector, VERSION};
use core::ffi::c_void;
use core::sync::atomic::Ordering::{Relaxed, SeqCst};
use minhook::{MinHook, MH_STATUS};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, LoadLibraryW};
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentProcess, GetCurrentProcessId, Sleep, TerminateProcess};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetWindowTextW};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn dll_path() -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(MODULE.load(SeqCst), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        None
    } else {
        Some(PathBuf::from(OsString::from_wide(&buf[..n])))
    }
}

/// Where the explanation goes when the game is closed: next to the log if we got that far, else next to the DLL.
fn fatal_file() -> PathBuf {
    if let Some(p) = state::FATAL_PATH.get() {
        return p.clone();
    }
    dll_path().and_then(|p| p.parent().map(|d| d.join("FATAL.txt"))).unwrap_or_else(|| PathBuf::from("FATAL.txt"))
}

/// Close the game rather than let it run without the sandbox. The launcher shows the reason (FATAL.txt) to the
/// player once the game process has gone; the game itself must not keep running while a dialog waits.
fn fatal(msg: &str) -> ! {
    if let Some(st) = state::get() {
        st.logger.log(&format!("FATAL: {msg}"));
    }
    let _ = std::fs::write(fatal_file(), format!("Ashen Marine closed Dark Souls III because its protections could not be put in place:\r\n\r\n{msg}\r\n\r\nYour real save was not touched.\r\n"));
    unsafe {
        TerminateProcess(GetCurrentProcess(), 3);
        loop {
            Sleep(1000);
        }
    }
}

pub unsafe extern "system" fn thread_main(_: *mut c_void) -> u32 {
    if let Err(msg) = run() {
        fatal(&msg);
    }
    0
}

unsafe fn run() -> Result<(), String> {
    let dll = dll_path().ok_or("cannot determine the hook DLL's own path")?;
    let dir = dll.parent().ok_or("the hook DLL has no folder")?.to_path_buf();
    let cfg_path = dir.join("config").join("ashenmarine.json");
    let cfg = HookConfig::load(&cfg_path).map_err(|e| format!("cannot read the config {}: {e}", cfg_path.display()))?;
    if let Some(dir) = Path::new(&cfg.log_file).parent() {
        let _ = state::FATAL_PATH.set(dir.join("FATAL.txt"));
    }
    if cfg.real_save_dir.trim().is_empty() || cfg.sandbox_save_dir.trim().is_empty() {
        return Err("the config does not name the real and private save folders".into());
    }
    if cfg.real_save_dir.trim_end_matches(['\\', '/']).eq_ignore_ascii_case(cfg.sandbox_save_dir.trim_end_matches(['\\', '/'])) {
        return Err("the private save folder is the same as the real one".into());
    }
    if !Path::new(&cfg.sandbox_save_dir).is_dir() {
        return Err(format!("the private save folder {} does not exist", cfg.sandbox_save_dir));
    }

    let logger = Logger::open(Path::new(&cfg.log_file), "hook");
    logger.log(&format!("hook v{VERSION} loaded in pid {} ({})", GetCurrentProcessId(), std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()));
    logger.log(&format!("config: real save {:?} -> private copy {:?}; block_network={}", cfg.real_save_dir, cfg.sandbox_save_dir, cfg.block_network));
    let redirector = Redirector::new(&cfg.real_save_dir, &cfg.sandbox_save_dir);
    let block_network = cfg.block_network;
    let cfg_for_tests = cfg.clone();
    let suffix = cfg.window_suffix.clone();
    state::set(State { cfg, redirector, logger })?;
    let st = state::get().ok_or("state vanished")?;

    // Make sure every module we hook is loaded; a missing optional one is skipped, a missing required one is fatal.
    let specs = hook_specs();
    let mut modules: Vec<&str> = specs.iter().flat_map(|s| [s.module, s.fallback_module]).filter(|m| !m.is_empty()).collect();
    modules.sort_unstable();
    modules.dedup();
    let mut missing: Vec<&str> = vec![];
    for m in &modules {
        if LoadLibraryW(wide(m).as_ptr()).is_null() {
            st.logger.log(&format!("module {m} could not be loaded"));
            missing.push(m);
        }
    }

    let (mut installed, mut skipped) = (0u32, 0u32);
    for spec in &specs {
        if spec.system == "offline_guard" && !block_network {
            skipped += 1;
            st.logger.log(&format!("skip [{}]: block_network=false (test control run)", spec.id));
            continue;
        }
        if missing.contains(&spec.module) {
            if spec.optional {
                skipped += 1;
                st.logger.log(&format!("skip [{}]: {} not available (optional)", spec.id, spec.module));
                continue;
            }
            return Err(format!("{} is not available but {}!{} must be hooked", spec.module, spec.module, spec.function));
        }
        let mut result = MinHook::create_hook_api(spec.module, spec.function, spec.detour).map(|o| (o, spec.module));
        if matches!(result, Err(MH_STATUS::MH_ERROR_FUNCTION_NOT_FOUND)) && !spec.fallback_module.is_empty() && !missing.contains(&spec.fallback_module) {
            result = MinHook::create_hook_api(spec.fallback_module, spec.function, spec.detour).map(|o| (o, spec.fallback_module));
        }
        match result {
            Ok((original, module)) => {
                spec.original.store(original, SeqCst);
                installed += 1;
                if module != spec.module {
                    st.logger.log(&format!("hooked [{}] in {} (not exported by {})", spec.id, module, spec.module));
                }
            }
            Err(MH_STATUS::MH_ERROR_FUNCTION_NOT_FOUND) if spec.optional => {
                skipped += 1;
                st.logger.log(&format!("skip [{}]: {} does not exist here (optional)", spec.id, spec.function));
            }
            Err(e) => return Err(format!("could not hook {}!{} for [{}]: {e:?}", spec.module, spec.function, spec.id)),
        }
    }
    MinHook::enable_all_hooks().map_err(|e| format!("could not enable the hooks: {e:?}"))?;
    st.logger.log(&format!("{installed} hooks enabled, {skipped} skipped"));

    // Prove it, in this very process, before the game gets anywhere near its save.
    crate::selftest::redirect(&cfg_for_tests)?;
    st.logger.log("redirect self-test passed: a file only in the private copy is visible through the real save path");
    if block_network {
        let msg = crate::selftest::network()?;
        st.logger.log(&msg);
    }
    st.logger.log(&format!("READY: save redirect {} -> {}; network blocked: {}", cfg_for_tests.real_save_dir, cfg_for_tests.sandbox_save_dir, block_network));

    let h = CreateThread(core::ptr::null(), 0, Some(marker_thread), suffix_ptr(suffix), 0, core::ptr::null_mut());
    if h.is_null() {
        st.logger.log("could not start the window-marker thread (cosmetic)");
    } else {
        windows_sys::Win32::Foundation::CloseHandle(h);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------- window marker + stats

fn suffix_ptr(s: String) -> *const c_void {
    Box::into_raw(Box::new(wide(&s))) as *const c_void
}

struct MarkerCtx<'a> {
    pid: u32,
    suffix: &'a [u16], // without NUL
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> i32 {
    let ctx = &*(lparam as *const MarkerCtx);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != ctx.pid || IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let mut buf = [0u16; 512];
    let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if n <= 0 || n as usize + ctx.suffix.len() + 1 >= buf.len() {
        return 1;
    }
    let title = &buf[..n as usize];
    if title.ends_with(ctx.suffix) {
        return 1;
    }
    let mut new_title = title.to_vec();
    new_title.extend_from_slice(ctx.suffix);
    new_title.push(0);
    SetWindowTextW(hwnd, new_title.as_ptr());
    1
}

unsafe extern "system" fn marker_thread(param: *mut c_void) -> u32 {
    let suffix: &'static Vec<u16> = &*(param as *const Vec<u16>);
    let suffix_no_nul = &suffix[..suffix.len().saturating_sub(1)];
    let ctx = MarkerCtx { pid: GetCurrentProcessId(), suffix: suffix_no_nul };
    let mut last = (0u64, 0u64, 0u64);
    let mut tick: u32 = 0;
    loop {
        Sleep(if tick < 120 { 1000 } else { 5000 });
        if !suffix_no_nul.is_empty() {
            EnumWindows(Some(enum_cb), &ctx as *const MarkerCtx as LPARAM);
        }
        let cur = (REDIRECTED.load(Relaxed), DENIED_ADDR.load(Relaxed), DENIED_NAME.load(Relaxed));
        if cur != last && tick % 15 == 0 {
            if let Some(st) = state::get() {
                st.logger.log(&format!("STATS redirected={} denied_addr={} denied_name={}", cur.0, cur.1, cur.2));
            }
            last = cur;
        }
        tick = tick.wrapping_add(1);
    }
}
