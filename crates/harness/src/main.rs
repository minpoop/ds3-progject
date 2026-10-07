//! Test harness: see `win.rs`. It only does anything on Windows (or under Wine).
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    win::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("ashen-harness runs on Windows or under Wine only");
}
