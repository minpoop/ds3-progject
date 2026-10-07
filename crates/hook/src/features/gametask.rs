//! Hooking a function into the game's own per-frame task list, once the game has finished starting. Shared by every
//! feature that has to read game state each frame.
use super::memscan;
use ashen_common::logging::Logger;
use darksouls3::sprj::{SprjTaskGroupIndex, SprjTaskImp};
use darksouls3::util::system::wait_for_system_init;
use fromsoftware_shared::{Program, RecurringTaskHandle, SharedTaskImpExt};
use pelite::pe64::Pe;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Wait for the game to finish starting, then call `frame` on the game's own thread at the start of every frame.
/// The returned handle must be kept alive for the rest of the process (dropping it only sets a flag the game ignores).
pub fn register(log: &Arc<Logger>, what: &str, mut frame: impl FnMut() + Send + 'static) -> Result<RecurringTaskHandle<usize>, String> {
    // The crate's own wait spins a CPU core; poll the same flag ourselves with sleeps, then let the crate confirm it.
    let flag_va = Program::current().rva_to_va(darksouls3::rva::get().global_hinstance).map_err(|e| format!("bad address table: {e}"))? as usize;
    let waited = Instant::now();
    loop {
        if memscan::read(flag_va, 8).is_some_and(|b| b.iter().any(|&x| x != 0)) {
            break;
        }
        if waited.elapsed() > Duration::from_secs(180) {
            return Err("the game did not finish starting within 3 minutes".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    wait_for_system_init(&Program::current(), Duration::from_secs(30)).map_err(|e| format!("the game did not finish starting: {e}"))?;
    log.log(&format!("game systems are initialised ({:.1}s after {what} started)", waited.elapsed().as_secs_f32()));
    let task = SprjTaskImp::wait_for_instance(Duration::from_secs(60)).map_err(|e| format!("no task manager: {e}"))?;
    log.log(&format!("task manager found; registering the per-frame task of {what}"));
    Ok(task.run_recurring(move |_: &usize| frame(), SprjTaskGroupIndex::FrameBegin))
}
