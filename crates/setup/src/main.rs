//! ashenmarine-setup: reads the player's own Space Marine 2 install (read-only).
//!
//!   ashenmarine-setup probe [--sm2 "<folder>"] [--out "<folder>"]
//!
//! `probe` is the default when no command is given (so a double-click works).
use ashen_setup::probe;

use std::path::PathBuf;
use std::process::ExitCode;

fn usage() {
    println!("ashenmarine-setup probe [--sm2 \"<Space Marine 2 folder>\"] [--out \"<report folder>\"]");
    println!("  Looks at your Space Marine 2 install (read-only) and writes probe-report.txt and some preview pictures.");
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1).peekable();
    let mut opts = probe::Opts { sm2: None, out: PathBuf::new() };
    let mut out: Option<PathBuf> = None;
    if args.peek().is_some_and(|a| !a.starts_with('-')) {
        let cmd = args.next().unwrap();
        if cmd != "probe" {
            eprintln!("unknown command {cmd:?}");
            usage();
            return ExitCode::from(64);
        }
    }
    while let Some(a) = args.next() {
        match a.as_str() {
            "--sm2" => opts.sm2 = args.next().map(PathBuf::from),
            "--out" => out = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                usage();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown option {other:?}");
                usage();
                return ExitCode::from(64);
            }
        }
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| PathBuf::from("."));
    opts.out = out.unwrap_or_else(|| exe_dir.join("probe-out"));
    if opts.sm2.is_none() {
        // optional hint for a game that is not in a normal Steam library: first line of sm2-folder.txt next to the exe
        if let Ok(text) = std::fs::read_to_string(exe_dir.join("sm2-folder.txt")) {
            let line = text.lines().next().unwrap_or("").trim().trim_matches('"').to_string();
            if !line.is_empty() {
                opts.sm2 = Some(PathBuf::from(line));
            }
        }
    }
    if probe::run(&opts) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}
