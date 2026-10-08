//! Ashen Marine: logic shared by the launcher, the hook DLL and the setup step.
//! Everything here is plain Rust that runs and is tested on any OS.

pub mod config;
pub mod fmg;
pub mod generated;
pub mod logging;
pub mod mixer;
pub mod soundset;
pub mod triggers;
pub mod wav;
pub mod weapons;
pub mod netguard;
pub mod paths;
pub mod redirect;
pub mod save;
pub mod steam;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Appended to the game window title so the player (and a screenshot) can tell this is the sandbox.
pub const WINDOW_SUFFIX: &str = " - Ashen Marine (offline copy)";

/// Steam app ids (sheet: games.steam_app_id).
pub const DS3_APP_ID: u32 = 374320;
pub const SM2_APP_ID: u32 = 2183900;
